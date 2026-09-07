//! `rings/dsp/resonator.{h,cc}` -- the modal resonator: a bank of up to 64
//! band-pass modes, the even and odd modes summed to the two outputs, with a
//! [`CosineOscillator`]-driven pickup comb applied in the frequency domain.

use stmlib::filter::{FilterMode, FrequencyApproximation, Svf};
use stmlib::parameter_interpolator::ParameterInterpolator;
use stmlib::{CosineOscillator, CosineOscillatorMode};

use crate::dsp::SAMPLE_RATE;
use crate::resources::{LUT_4_DECADES, LUT_STIFFNESS};

pub const MAX_MODES: usize = 64;

/// `stmlib::Interpolate` with the C's "read one past a `size + 1` table,
/// multiplied by a zero fraction" clamped in bounds (as in `mi-elements`).
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

/// `rings::Resonator`.
pub struct Resonator {
    frequency: f32,
    structure: f32,
    brightness: f32,
    position: f32,
    previous_position: f32,
    damping: f32,
    resolution: usize,
    f: [Svf; MAX_MODES],
}

impl Default for Resonator {
    fn default() -> Self {
        Self::new()
    }
}

impl Resonator {
    pub fn new() -> Self {
        let mut r = Self {
            frequency: 220.0 / SAMPLE_RATE,
            structure: 0.25,
            brightness: 0.5,
            position: 0.999,
            previous_position: 0.0,
            damping: 0.3,
            resolution: MAX_MODES,
            f: [Svf::default(); MAX_MODES],
        };
        r.init();
        r
    }

    /// `Init`.
    pub fn init(&mut self) {
        for f in self.f.iter_mut() {
            f.init();
        }
        self.frequency = 220.0 / SAMPLE_RATE;
        self.structure = 0.25;
        self.brightness = 0.5;
        self.damping = 0.3;
        self.position = 0.999;
        self.previous_position = 0.0;
        self.resolution = MAX_MODES;
    }

    #[inline]
    pub fn set_frequency(&mut self, frequency: f32) {
        self.frequency = frequency;
    }
    #[inline]
    pub fn set_structure(&mut self, structure: f32) {
        self.structure = structure;
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

    /// `set_resolution` -- must be even (modes are summed two at a time).
    #[inline]
    pub fn set_resolution(&mut self, resolution: i32) {
        let resolution = resolution - (resolution & 1);
        self.resolution = (resolution.max(0) as usize).min(MAX_MODES);
    }

    fn compute_filters(&mut self) -> usize {
        let mut stiffness = interpolate(&LUT_STIFFNESS, self.structure, 256.0);
        let mut harmonic = self.frequency;
        let mut stretch_factor = 1.0f32;
        let mut q = 500.0 * interpolate(&LUT_4_DECADES, self.damping, 256.0);

        let mut brightness_attenuation = 1.0 - self.structure;
        brightness_attenuation *= brightness_attenuation;
        brightness_attenuation *= brightness_attenuation;
        brightness_attenuation *= brightness_attenuation;
        let brightness = self.brightness * (1.0 - 0.2 * brightness_attenuation);
        let mut q_loss = brightness * (2.0 - brightness) * 0.85 + 0.15;
        let q_loss_damping_rate = self.structure * (2.0 - self.structure) * 0.1;

        let mut num_modes = 0;
        for i in 0..MAX_MODES.min(self.resolution) {
            let mut partial_frequency = harmonic * stretch_factor;
            if partial_frequency >= 0.49 {
                partial_frequency = 0.49;
            } else {
                num_modes = i + 1;
            }
            self.f[i].set_f_q(
                partial_frequency,
                1.0 + partial_frequency * q,
                FrequencyApproximation::Fast,
            );
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

    /// `Process(in, out, aux, size)` -- odd modes -> `out`, even modes -> `aux`.
    pub fn process(&mut self, input: &[f32], out: &mut [f32], aux: &mut [f32], size: usize) {
        let num_modes = self.compute_filters();

        let mut position =
            ParameterInterpolator::new(&mut self.previous_position, self.position, size);

        for s in 0..size {
            let mut amplitudes =
                CosineOscillator::new(CosineOscillatorMode::Approximate, position.next());

            let input_sample = input[s] * 0.125;
            let mut odd = 0.0f32;
            let mut even = 0.0f32;
            amplitudes.start();
            let mut i = 0;
            while i < num_modes {
                odd += amplitudes.next() * self.f[i].process(FilterMode::BandPass, input_sample);
                i += 1;
                even += amplitudes.next() * self.f[i].process(FilterMode::BandPass, input_sample);
                i += 1;
            }
            out[s] = odd;
            aux[s] = even;
        }
    }
}
