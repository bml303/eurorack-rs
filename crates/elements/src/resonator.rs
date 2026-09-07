//! `elements/dsp/resonator.{h,cc}` -- the modal resonator: a bank of up to 64
//! band-pass "modes" plus 8 banded-waveguide "bowed" modes with their own delay
//! lines, all summed through a frequency-domain pickup (the position comb is
//! applied as per-mode amplitudes from a [`CosineOscillator`]).

use alloc::boxed::Box;
use alloc::vec::Vec;

use stmlib::DelayLine;
use stmlib::filter::{FilterMode, FrequencyApproximation, Svf};
use stmlib::{CosineOscillator, CosineOscillatorMode};

use crate::resources::{LUT_4_DECADES, LUT_STIFFNESS};

/// `stmlib::Interpolate(table, index, size)` with the C's implicit "read one
/// past the end, multiplied by a zero fraction" clamped into bounds: the MI
/// resource tables are `size + 1` entries and the engines index them at the
/// full-scale value.
#[inline]
fn interpolate(table: &[f32], index: f32, size: f32) -> f32 {
    let scaled = index.clamp(0.0, 1.0) * size;
    let last = table.len() - 1;
    let integral = (scaled as usize).min(last);
    let fractional = scaled - integral as f32;
    let a = table[integral];
    let b = table[(integral + 1).min(last)];
    a + (b - a) * fractional
}

pub const MAX_MODES: usize = 64;
pub const MAX_BOWED_MODES: usize = 8;
pub const MAX_DELAY_LINE_SIZE: usize = 1024;

/// `elements::Resonator`.
pub struct Resonator {
    frequency: f32,
    geometry: f32,
    brightness: f32,
    position: f32,
    previous_position: f32,
    damping: f32,

    modulation_frequency: f32,
    modulation_offset: f32,
    lfo_phase: f32,

    bow_signal: f32,

    resolution: usize,

    f: [Svf; MAX_MODES],
    f_bow: [Svf; MAX_BOWED_MODES],
    d_bow: Box<[DelayLine<MAX_DELAY_LINE_SIZE>]>,

    clock_divider: usize,
}

impl Default for Resonator {
    fn default() -> Self {
        Self::new()
    }
}

impl Resonator {
    pub fn new() -> Self {
        let d_bow: Vec<DelayLine<MAX_DELAY_LINE_SIZE>> =
            (0..MAX_BOWED_MODES).map(|_| DelayLine::default()).collect();
        let mut r = Self {
            frequency: 220.0 / crate::dsp::SAMPLE_RATE,
            geometry: 0.25,
            brightness: 0.5,
            position: 0.999,
            previous_position: 0.0,
            damping: 0.3,
            modulation_frequency: 0.0,
            modulation_offset: 0.0,
            lfo_phase: 0.0,
            bow_signal: 0.0,
            resolution: MAX_MODES,
            f: [Svf::default(); MAX_MODES],
            f_bow: [Svf::default(); MAX_BOWED_MODES],
            d_bow: d_bow.into_boxed_slice(),
            clock_divider: 0,
        };
        r.init();
        r
    }

    /// `Init`.
    pub fn init(&mut self) {
        for f in self.f.iter_mut() {
            f.init();
        }
        for f in self.f_bow.iter_mut() {
            f.init();
        }
        for d in self.d_bow.iter_mut() {
            d.init();
        }
        self.frequency = 220.0 / crate::dsp::SAMPLE_RATE;
        self.geometry = 0.25;
        self.brightness = 0.5;
        self.damping = 0.3;
        self.position = 0.999;
        self.previous_position = 0.0;
        self.resolution = MAX_MODES;
        self.lfo_phase = 0.0;
        self.clock_divider = 0;
        self.bow_signal = 0.0;
    }

    #[inline]
    pub fn set_frequency(&mut self, frequency: f32) {
        self.frequency = frequency;
    }
    #[inline]
    pub fn set_geometry(&mut self, geometry: f32) {
        self.geometry = geometry;
    }
    #[inline]
    pub fn set_brightness(&mut self, brightness: f32) {
        self.brightness = brightness;
    }
    #[inline]
    pub fn set_damping(&mut self, damping: f32) {
        self.damping = damping;
    }
    #[inline]
    pub fn set_position(&mut self, position: f32) {
        self.position = position;
    }
    #[inline]
    pub fn set_resolution(&mut self, resolution: usize) {
        self.resolution = resolution.min(MAX_MODES);
    }
    #[inline]
    pub fn set_modulation_frequency(&mut self, modulation_frequency: f32) {
        self.modulation_frequency = modulation_frequency;
    }
    #[inline]
    pub fn set_modulation_offset(&mut self, modulation_offset: f32) {
        self.modulation_offset = modulation_offset;
    }

    /// `BowTable(x, velocity)` -- the friction curve.
    #[inline]
    fn bow_table(x: f32, velocity: f32) -> f32 {
        let x = 0.13 * velocity - x;
        let mut bow = x * 6.0;
        bow = bow.abs() + 0.75;
        bow *= bow;
        bow *= bow;
        bow = 0.25 / bow;
        bow = bow.clamp(0.0025, 0.245);
        x * bow
    }

    /// `ComputeFilters` -- refresh the mode filters, return the number of modes
    /// still below Nyquist.
    fn compute_filters(&mut self) -> usize {
        self.clock_divider = self.clock_divider.wrapping_add(1);
        let mut stiffness = interpolate(&LUT_STIFFNESS, self.geometry, 256.0);
        let mut harmonic = self.frequency;
        let mut stretch_factor = 1.0f32;
        let mut q = 500.0 * interpolate(&LUT_4_DECADES, self.damping * 0.8, 256.0);

        let mut brightness_attenuation = 1.0 - self.geometry;
        brightness_attenuation *= brightness_attenuation;
        brightness_attenuation *= brightness_attenuation;
        brightness_attenuation *= brightness_attenuation;
        let brightness = self.brightness * (1.0 - 0.2 * brightness_attenuation);
        let mut q_loss = brightness * (2.0 - brightness) * 0.85 + 0.15;
        let q_loss_damping_rate = self.geometry * (2.0 - self.geometry) * 0.1;

        let mut num_modes = 0;
        for i in 0..MAX_MODES.min(self.resolution) {
            // Update the first 24 modes every time (2 kHz); refresh the rest at
            // half that rate.
            let update = i <= 24 || (i & 1) == (self.clock_divider & 1);
            let mut partial_frequency = harmonic * stretch_factor;
            if partial_frequency >= 0.49 {
                partial_frequency = 0.49;
            } else {
                num_modes = i + 1;
            }
            if update {
                self.f[i].set_f_q(
                    partial_frequency,
                    1.0 + partial_frequency * q,
                    FrequencyApproximation::Fast,
                );
                if i < MAX_BOWED_MODES {
                    let mut period = (1.0 / partial_frequency) as usize;
                    while period >= MAX_DELAY_LINE_SIZE {
                        period >>= 1;
                    }
                    self.d_bow[i].set_delay(period);
                    self.f_bow[i].set_g_q(self.f[i].g(), 1.0 + partial_frequency * 1500.0);
                }
            }
            stretch_factor += stiffness;
            if stiffness < 0.0 {
                stiffness *= 0.93;
            } else {
                stiffness *= 0.98;
            }
            q_loss += q_loss_damping_rate * (1.0 - q_loss);
            harmonic += self.frequency;
            q *= q_loss;
        }

        num_modes
    }

    /// `Process(bow_strength, in, center, sides, size)`.
    pub fn process(
        &mut self,
        bow_strength: &[f32],
        input: &[f32],
        center: &mut [f32],
        sides: &mut [f32],
        size: usize,
    ) {
        let num_modes = self.compute_filters();
        let num_banded_wg = MAX_BOWED_MODES.min(num_modes);
        let position_increment = (self.position - self.previous_position) / size as f32;

        for i in 0..size {
            // 0.5 Hz LFO modulating the stereo side channel's pickup position.
            self.lfo_phase += self.modulation_frequency;
            if self.lfo_phase >= 1.0 {
                self.lfo_phase -= 1.0;
            }
            self.previous_position += position_increment;
            let lfo = if self.lfo_phase > 0.5 {
                1.0 - self.lfo_phase
            } else {
                self.lfo_phase
            };

            let mut amplitudes =
                CosineOscillator::new(CosineOscillatorMode::Approximate, self.previous_position);
            let mut aux_amplitudes = CosineOscillator::new(
                CosineOscillatorMode::Approximate,
                self.modulation_offset + lfo,
            );

            let mut input_sample = input[i] * 0.125;
            let mut sum_center = 0.0f32;
            let mut sum_side = 0.0f32;

            // Apply the pickup comb in the frequency domain by weighting each
            // mode's amplitude, rather than as a delay-line comb (which smears
            // attacks and flanges under modulation).
            amplitudes.start();
            aux_amplitudes.start();
            for m in 0..num_modes {
                let s = self.f[m].process(FilterMode::BandPass, input_sample);
                sum_center += s * amplitudes.next();
                sum_side += s * aux_amplitudes.next();
            }
            sides[i] = sum_side - sum_center;

            // Render bowed modes.
            let mut bow_signal = 0.0f32;
            input_sample += self.bow_signal;
            amplitudes.start();
            for m in 0..num_banded_wg {
                let mut s = 0.99 * self.d_bow[m].read();
                bow_signal += s;
                s = self.f_bow[m].process(FilterMode::BandPassNormalized, input_sample + s);
                self.d_bow[m].write(s);
                sum_center += s * amplitudes.next() * 8.0;
            }
            self.bow_signal = Self::bow_table(bow_signal, bow_strength[i]);
            center[i] = sum_center;
        }
    }
}
