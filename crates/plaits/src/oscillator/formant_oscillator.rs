//! `plaits/dsp/oscillator/formant_oscillator.h` -- a sine carrier reset every
//! cycle by a (usually higher) formant frequency, with an aliasing-free reset.

use stmlib::parameter_interpolator::ParameterInterpolator;
use stmlib::polyblep::{next_blep_sample, this_blep_sample};

use super::oscillator::MAX_FREQUENCY;
use super::sine_oscillator::sine;

#[derive(Debug, Clone, Copy, Default)]
pub struct FormantOscillator {
    carrier_phase: f32,
    formant_phase: f32,
    next_sample: f32,
    carrier_frequency: f32,
    formant_frequency: f32,
    phase_shift: f32,
}

impl FormantOscillator {
    pub fn init(&mut self) {
        self.carrier_phase = 0.0;
        self.formant_phase = 0.0;
        self.next_sample = 0.0;
        self.carrier_frequency = 0.0;
        self.formant_frequency = 0.01;
        self.phase_shift = 0.0;
    }

    pub fn render(
        &mut self,
        mut carrier_frequency: f32,
        mut formant_frequency: f32,
        phase_shift: f32,
        out: &mut [f32],
    ) {
        if carrier_frequency >= MAX_FREQUENCY {
            carrier_frequency = MAX_FREQUENCY;
        }
        if formant_frequency >= MAX_FREQUENCY {
            formant_frequency = MAX_FREQUENCY;
        }

        let size = out.len();
        let mut carrier_fm = ParameterInterpolator::new(&mut self.carrier_frequency, carrier_frequency, size);
        let mut formant_fm = ParameterInterpolator::new(&mut self.formant_frequency, formant_frequency, size);
        let mut pm = ParameterInterpolator::new(&mut self.phase_shift, phase_shift, size);

        let mut next_sample = self.next_sample;

        for o in out.iter_mut() {
            let mut this_sample = next_sample;
            next_sample = 0.0;

            let carrier_frequency = carrier_fm.next();
            let formant_frequency = formant_fm.next();

            self.carrier_phase += carrier_frequency;

            if self.carrier_phase >= 1.0 {
                self.carrier_phase -= 1.0;
                let reset_time = self.carrier_phase / carrier_frequency;

                let formant_phase_at_reset =
                    self.formant_phase + (1.0 - reset_time) * formant_frequency;
                let before = sine(formant_phase_at_reset + pm.subsample(1.0 - reset_time));
                let after = sine(0.0 + pm.subsample(1.0));
                let discontinuity = after - before;
                this_sample += discontinuity * this_blep_sample(reset_time);
                next_sample += discontinuity * next_blep_sample(reset_time);
                self.formant_phase = reset_time * formant_frequency;
            } else {
                self.formant_phase += formant_frequency;
                if self.formant_phase >= 1.0 {
                    self.formant_phase -= 1.0;
                }
            }

            let phase_shift = pm.next();
            next_sample += sine(self.formant_phase + phase_shift);

            *o = this_sample;
        }
        self.next_sample = next_sample;
    }
}
