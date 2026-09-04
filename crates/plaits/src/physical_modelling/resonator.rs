//! `plaits/dsp/physical_modelling/resonator.h` -- a bank of resonant modes
//! (from Rings, with fixed excitation position), rendered `BATCH` filters at
//! a time.

use stmlib::cosine_oscillator::{CosineOscillator, CosineOscillatorMode};
use stmlib::fdsp::interpolate;
use stmlib::filter::{one_pole_tan, FilterMode, FrequencyApproximation};
use stmlib::units::semitones_to_ratio;

use crate::resources::LUT_STIFFNESS;

pub const MAX_NUM_MODES: usize = 24;
pub const MODE_BATCH_SIZE: usize = 4;

/// `ResonatorSvf<batch_size>` -- `BATCH` state-variable filters processed in
/// lock-step (one call per audio block instead of per mode).
#[derive(Debug, Clone, Copy)]
pub struct ResonatorSvf<const BATCH: usize> {
    state_1: [f32; BATCH],
    state_2: [f32; BATCH],
}

impl<const BATCH: usize> Default for ResonatorSvf<BATCH> {
    fn default() -> Self {
        Self {
            state_1: [0.0; BATCH],
            state_2: [0.0; BATCH],
        }
    }
}

impl<const BATCH: usize> ResonatorSvf<BATCH> {
    pub fn init(&mut self) {
        self.state_1 = [0.0; BATCH];
        self.state_2 = [0.0; BATCH];
    }

    /// `Process<mode, add>(f, q, gain, in, out, size)`.
    pub fn process(
        &mut self,
        mode: FilterMode,
        add: bool,
        f: &[f32; BATCH],
        q: &[f32; BATCH],
        gain: &[f32; BATCH],
        input: &[f32],
        out: &mut [f32],
    ) {
        let mut g = [0.0f32; BATCH];
        let mut r = [0.0f32; BATCH];
        let mut r_plus_g = [0.0f32; BATCH];
        let mut h = [0.0f32; BATCH];
        let mut state_1 = self.state_1;
        let mut state_2 = self.state_2;

        for i in 0..BATCH {
            g[i] = one_pole_tan(f[i], FrequencyApproximation::Fast);
            r[i] = 1.0 / q[i];
            h[i] = 1.0 / (1.0 + r[i] * g[i] + g[i] * g[i]);
            r_plus_g[i] = r[i] + g[i];
        }

        for (idx, &s_in) in input.iter().enumerate() {
            let mut s_out = 0.0f32;
            for i in 0..BATCH {
                let hp = (s_in - r_plus_g[i] * state_1[i] - state_2[i]) * h[i];
                let bp = g[i] * hp + state_1[i];
                state_1[i] = g[i] * hp + bp;
                let lp = g[i] * bp + state_2[i];
                state_2[i] = g[i] * bp + lp;
                s_out += gain[i] * (if mode == FilterMode::LowPass { lp } else { bp });
            }
            if add {
                out[idx] += s_out;
            } else {
                out[idx] = s_out;
            }
        }
        self.state_1 = state_1;
        self.state_2 = state_2;
    }
}

#[inline]
fn nth_harmonic_compensation(n: i32, mut stiffness: f32) -> f32 {
    let mut stretch_factor = 1.0f32;
    for _ in 0..n - 1 {
        stretch_factor += stiffness;
        if stiffness < 0.0 {
            stiffness *= 0.93;
        } else {
            stiffness *= 0.98;
        }
    }
    1.0 / stretch_factor
}

#[derive(Debug, Clone)]
pub struct Resonator {
    resolution: usize,
    mode_amplitude: [f32; MAX_NUM_MODES],
    mode_filters: [ResonatorSvf<MODE_BATCH_SIZE>; MAX_NUM_MODES / MODE_BATCH_SIZE],
}

impl Default for Resonator {
    fn default() -> Self {
        Self {
            resolution: 0,
            mode_amplitude: [0.0; MAX_NUM_MODES],
            mode_filters: [ResonatorSvf::default(); MAX_NUM_MODES / MODE_BATCH_SIZE],
        }
    }
}

impl Resonator {
    pub fn init(&mut self, position: f32, resolution: usize) {
        self.resolution = resolution.min(MAX_NUM_MODES);

        let mut amplitudes = CosineOscillator::new(CosineOscillatorMode::Approximate, position);

        for i in 0..resolution {
            self.mode_amplitude[i] = amplitudes.next() * 0.25;
        }

        for f in self.mode_filters.iter_mut() {
            f.init();
        }
    }

    pub fn process(
        &mut self,
        f0: f32,
        structure: f32,
        brightness: f32,
        damping: f32,
        input: &[f32],
        out: &mut [f32],
    ) {
        let stiffness = interpolate(&LUT_STIFFNESS, structure, 64.0);
        let f0 = f0 * nth_harmonic_compensation(3, stiffness);

        let mut harmonic = f0;
        let mut stretch_factor = 1.0f32;
        let mut stiffness = stiffness;
        let q_sqrt = semitones_to_ratio(damping * 79.7);
        let mut q = 500.0 * q_sqrt * q_sqrt;
        let mut brightness = brightness;
        brightness *= 1.0 - structure * 0.3;
        brightness *= 1.0 - damping * 0.3;
        let q_loss = brightness * (2.0 - brightness) * 0.85 + 0.15;

        let mut mode_q = [0.0f32; MODE_BATCH_SIZE];
        let mut mode_f = [0.0f32; MODE_BATCH_SIZE];
        let mut mode_a = [0.0f32; MODE_BATCH_SIZE];
        let mut batch_counter = 0usize;
        let mut batch_index = 0usize;

        for i in 0..self.resolution {
            let mut mode_frequency = harmonic * stretch_factor;
            if mode_frequency >= 0.499 {
                mode_frequency = 0.499;
            }
            let mode_attenuation = 1.0 - mode_frequency * 2.0;

            mode_f[batch_counter] = mode_frequency;
            mode_q[batch_counter] = 1.0 + mode_frequency * q;
            mode_a[batch_counter] = self.mode_amplitude[i] * mode_attenuation;
            batch_counter += 1;

            if batch_counter == MODE_BATCH_SIZE {
                batch_counter = 0;
                self.mode_filters[batch_index].process(
                    FilterMode::BandPass,
                    true,
                    &mode_f,
                    &mode_q,
                    &mode_a,
                    input,
                    out,
                );
                batch_index += 1;
            }

            stretch_factor += stiffness;
            if stiffness < 0.0 {
                stiffness *= 0.93;
            } else {
                stiffness *= 0.98;
            }
            harmonic += f0;
            q *= q_loss;
        }
    }
}
