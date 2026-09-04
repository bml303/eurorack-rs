//! `plaits/dsp/oscillator/z_oscillator.h` -- a sine formant multiplied by, and
//! sync'ed to, a carrier at twice its rate, with a `mode` control that
//! reshapes the formant's window.

use stmlib::parameter_interpolator::ParameterInterpolator;
use stmlib::polyblep::{next_blep_sample, this_blep_sample};

use super::oscillator::MAX_FREQUENCY;
use super::sine_oscillator::sine;

#[derive(Debug, Clone, Copy, Default)]
pub struct ZOscillator {
    carrier_phase: f32,
    discontinuity_phase: f32,
    formant_phase: f32,
    next_sample: f32,
    carrier_frequency: f32,
    formant_frequency: f32,
    carrier_shape: f32,
    mode: f32,
}

impl ZOscillator {
    pub fn init(&mut self) {
        *self = Self::default();
    }

    pub fn render(
        &mut self,
        mut carrier_frequency: f32,
        mut formant_frequency: f32,
        carrier_shape: f32,
        mode: f32,
        out: &mut [f32],
    ) {
        if carrier_frequency >= MAX_FREQUENCY * 0.5 {
            carrier_frequency = MAX_FREQUENCY * 0.5;
        }
        if formant_frequency >= MAX_FREQUENCY {
            formant_frequency = MAX_FREQUENCY;
        }

        let size = out.len();
        let mut f0m = ParameterInterpolator::new(&mut self.carrier_frequency, carrier_frequency, size);
        let mut f1m = ParameterInterpolator::new(&mut self.formant_frequency, formant_frequency, size);
        let mut shape_m = ParameterInterpolator::new(&mut self.carrier_shape, carrier_shape, size);
        let mut mode_m = ParameterInterpolator::new(&mut self.mode, mode, size);

        let mut next_sample = self.next_sample;

        for o in out.iter_mut() {
            let mut reset_time = 0.0f32;

            let mut this_sample = next_sample;
            next_sample = 0.0;

            let f0 = f0m.next();
            let f1 = f1m.next();

            self.discontinuity_phase += 2.0 * f0;
            self.carrier_phase += f0;
            let reset = self.discontinuity_phase >= 1.0;

            if reset {
                self.discontinuity_phase -= 1.0;
                reset_time = self.discontinuity_phase / (2.0 * f0);

                let carrier_phase_before = if self.carrier_phase >= 1.0 { 1.0 } else { 0.5 };
                let carrier_phase_after = if self.carrier_phase >= 1.0 { 0.0 } else { 0.5 };
                let before = Self::z(
                    carrier_phase_before,
                    1.0,
                    self.formant_phase + (1.0 - reset_time) * f1,
                    shape_m.subsample(1.0 - reset_time),
                    mode_m.subsample(1.0 - reset_time),
                );
                let after = Self::z(carrier_phase_after, 0.0, 0.0, shape_m.subsample(1.0), mode_m.subsample(1.0));

                let discontinuity = after - before;
                this_sample += discontinuity * this_blep_sample(reset_time);
                next_sample += discontinuity * next_blep_sample(reset_time);
                self.formant_phase = reset_time * f1;

                if self.carrier_phase > 1.0 {
                    self.carrier_phase = self.discontinuity_phase * 0.5;
                }
            } else {
                self.formant_phase += f1;
                if self.formant_phase >= 1.0 {
                    self.formant_phase -= 1.0;
                }
            }

            if self.carrier_phase >= 1.0 {
                self.carrier_phase -= 1.0;
            }

            next_sample += Self::z(
                self.carrier_phase,
                self.discontinuity_phase,
                self.formant_phase,
                shape_m.next(),
                mode_m.next(),
            );
            *o = this_sample;
        }

        self.next_sample = next_sample;
    }

    fn z(c: f32, d: f32, f: f32, shape: f32, mode: f32) -> f32 {
        let mut ramp_down = 0.5 * (1.0 + sine(0.5 * d + 0.25));

        let offset;
        let phase_shift;
        if mode < 0.333 {
            offset = 1.0;
            phase_shift = 0.25 + mode * 1.50;
        } else if mode < 0.666 {
            phase_shift = 0.7495 - (mode - 0.33) * 0.75;
            offset = -sine(phase_shift);
        } else {
            phase_shift = 0.7495 - (mode - 0.33) * 0.75;
            offset = 0.001;
        }

        let discontinuity = sine(f + phase_shift);
        let mut shape = shape;
        let contour = if shape < 0.5 {
            shape *= 2.0;
            if c >= 0.5 {
                ramp_down *= shape;
            }
            1.0 + (sine(c + 0.25) - 1.0) * shape
        } else {
            sine(c + shape * 0.5)
        };
        (ramp_down * (offset + discontinuity) - offset) * contour
    }
}
