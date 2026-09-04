//! `plaits/dsp/oscillator/grainlet_oscillator.h` -- a phase-distorted single
//! cycle sine ("grain") multiplied by a continuously running formant sine,
//! the grain synced to a main oscillator.

use stmlib::parameter_interpolator::ParameterInterpolator;
use stmlib::polyblep::{next_blep_sample, this_blep_sample};

use super::oscillator::MAX_FREQUENCY;
use super::sine_oscillator::sine;

#[derive(Debug, Clone, Copy, Default)]
pub struct GrainletOscillator {
    carrier_phase: f32,
    formant_phase: f32,
    next_sample: f32,
    carrier_frequency: f32,
    formant_frequency: f32,
    carrier_shape: f32,
    carrier_bleed: f32,
}

impl GrainletOscillator {
    pub fn init(&mut self) {
        *self = Self::default();
    }

    pub fn render(
        &mut self,
        mut carrier_frequency: f32,
        mut formant_frequency: f32,
        carrier_shape: f32,
        carrier_bleed: f32,
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
        let mut bleed_m = ParameterInterpolator::new(&mut self.carrier_bleed, carrier_bleed, size);

        let mut next_sample = self.next_sample;

        for o in out.iter_mut() {
            let mut this_sample = next_sample;
            next_sample = 0.0;

            let f0 = f0m.next();
            let f1 = f1m.next();

            self.carrier_phase += f0;
            let reset = self.carrier_phase >= 1.0;

            if reset {
                self.carrier_phase -= 1.0;
                let reset_time = self.carrier_phase / f0;
                let before = Self::grainlet(
                    1.0,
                    self.formant_phase + (1.0 - reset_time) * f1,
                    shape_m.subsample(1.0 - reset_time),
                    bleed_m.subsample(1.0 - reset_time),
                );
                let after = Self::grainlet(0.0, 0.0, shape_m.subsample(1.0), bleed_m.subsample(1.0));

                let discontinuity = after - before;
                this_sample += discontinuity * this_blep_sample(reset_time);
                next_sample += discontinuity * next_blep_sample(reset_time);
                self.formant_phase = reset_time * f1;
            } else {
                self.formant_phase += f1;
                if self.formant_phase >= 1.0 {
                    self.formant_phase -= 1.0;
                }
            }

            next_sample +=
                Self::grainlet(self.carrier_phase, self.formant_phase, shape_m.next(), bleed_m.next());
            *o = this_sample;
        }

        self.next_sample = next_sample;
    }

    fn carrier(phase: f32, shape: f32) -> f32 {
        let shape = shape * 3.0;
        let shape_integral = shape as i32;
        let shape_fractional = shape - shape_integral as f32;
        let t = 1.0 - shape_fractional;

        let mut phase = phase;
        if shape_integral == 0 {
            phase *= 1.0 + t * t * t * 15.0;
            if phase >= 1.0 {
                phase = 1.0;
            }
            phase += 0.75;
        } else if shape_integral == 1 {
            let breakpoint = 0.001 + 0.499 * t * t * t;
            if phase < breakpoint {
                phase *= 0.5 / breakpoint;
            } else {
                phase = 0.5 + (phase - breakpoint) * 0.5 / (1.0 - breakpoint);
            }
            phase += 0.75;
        } else {
            let t = 1.0 - t;
            phase = 0.25 + phase * (0.5 + t * t * t * 14.5);
            if phase >= 0.75 {
                phase = 0.75;
            }
        }
        (sine(phase) + 1.0) * 0.25
    }

    fn grainlet(carrier_phase: f32, formant_phase: f32, shape: f32, bleed: f32) -> f32 {
        let carrier = Self::carrier(carrier_phase, shape);
        let formant = sine(formant_phase);
        carrier * (formant + bleed) / (1.0 + bleed)
    }
}
