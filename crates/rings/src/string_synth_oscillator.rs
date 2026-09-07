//! `rings/dsp/string_synth_oscillator.h` -- a PolyBLEP square/saw pair with a
//! per-shape one-pole integrator, for the string-synth's stacked harmonics.

use stmlib::parameter_interpolator::ParameterInterpolator;
use stmlib::polyblep::{next_blep_sample, this_blep_sample};

/// `rings::OscillatorShape`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OscillatorShape {
    BrightSquare,
    #[allow(dead_code)]
    Square,
    DarkSquare,
    #[allow(dead_code)]
    Triangle,
}

/// `rings::StringSynthOscillator`.
#[derive(Debug, Clone)]
pub struct StringSynthOscillator {
    high: bool,
    phase: f32,
    phase_increment: f32,
    next_sample: f32,
    next_sample_saw: f32,
    filter_state: f32,
    gain: f32,
    gain_saw: f32,
}

impl Default for StringSynthOscillator {
    fn default() -> Self {
        Self::new()
    }
}

impl StringSynthOscillator {
    pub fn new() -> Self {
        Self {
            high: false,
            phase: 0.0,
            phase_increment: 0.01,
            next_sample: 0.0,
            next_sample_saw: 0.0,
            filter_state: 0.0,
            gain: 0.0,
            gain_saw: 0.0,
        }
    }

    /// `Init`.
    pub fn init(&mut self) {
        *self = Self::new();
    }

    /// `Render<shape, interpolate_pitch>(target_increment, target_gain,
    /// target_gain_saw, out, size)` -- accumulates into `out`.
    pub fn render(
        &mut self,
        shape: OscillatorShape,
        interpolate_pitch: bool,
        target_increment: f32,
        mut target_gain: f32,
        target_gain_saw: f32,
        out: &mut [f32],
        size: usize,
    ) {
        // Cut harmonics above ~12 kHz, low-pass those above ~8 kHz.
        if target_increment >= 0.17 {
            target_gain *= 1.0 - (target_increment - 0.17) * 12.5;
            if target_increment >= 0.25 {
                return;
            }
        }

        let mut phase = self.phase;
        let mut phase_increment =
            ParameterInterpolator::new(&mut self.phase_increment, target_increment, size);
        let mut gain = ParameterInterpolator::new(&mut self.gain, target_gain, size);
        let mut gain_saw = ParameterInterpolator::new(&mut self.gain_saw, target_gain_saw, size);

        let mut next_sample = self.next_sample;
        let mut next_sample_saw = self.next_sample_saw;
        let mut filter_state = self.filter_state;
        let mut high = self.high;

        for o in out.iter_mut().take(size) {
            let mut this_sample = next_sample;
            let mut this_sample_saw = next_sample_saw;
            next_sample = 0.0;
            next_sample_saw = 0.0;

            let increment = if interpolate_pitch {
                phase_increment.next()
            } else {
                target_increment
            };
            phase += increment;

            const PW: f32 = 0.5;

            if !high && phase >= PW {
                let t = (phase - PW) / increment;
                this_sample += this_blep_sample(t);
                next_sample += next_blep_sample(t);
                high = true;
            }
            if phase >= 1.0 {
                phase -= 1.0;
                let t = phase / increment;
                let a = this_blep_sample(t);
                let b = next_blep_sample(t);
                this_sample -= a;
                next_sample -= b;
                this_sample_saw -= a;
                next_sample_saw -= b;
                high = false;
            }

            next_sample += if phase < PW { 0.0 } else { 1.0 };
            next_sample_saw += phase;

            let sample = match shape {
                OscillatorShape::Triangle => {
                    let integrator_coefficient = increment * 0.125;
                    this_sample = 64.0 * (this_sample - 0.5);
                    filter_state += integrator_coefficient * (this_sample - filter_state);
                    filter_state
                }
                OscillatorShape::DarkSquare => {
                    let integrator_coefficient = increment * 2.0;
                    this_sample = 4.0 * (this_sample - 0.5);
                    filter_state += integrator_coefficient * (this_sample - filter_state);
                    filter_state
                }
                OscillatorShape::BrightSquare => {
                    let integrator_coefficient = increment * 2.0;
                    this_sample = 2.0 * this_sample - 1.0;
                    filter_state += integrator_coefficient * (this_sample - filter_state);
                    (this_sample - filter_state) * 0.5
                }
                OscillatorShape::Square => 2.0 * this_sample - 1.0,
            };
            this_sample_saw = 2.0 * this_sample_saw - 1.0;

            *o += sample * gain.next() + this_sample_saw * gain_saw.next();
        }

        self.high = high;
        self.phase = phase;
        self.next_sample = next_sample;
        self.next_sample_saw = next_sample_saw;
        self.filter_state = filter_state;
    }
}
