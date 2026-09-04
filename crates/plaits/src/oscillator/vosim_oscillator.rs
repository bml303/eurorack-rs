//! `plaits/dsp/oscillator/vosim_oscillator.h` -- two sinewave formants
//! multiplied by, and phase-locked to, a carrier (VOSIM voice synthesis).

use stmlib::parameter_interpolator::ParameterInterpolator;

use super::oscillator::MAX_FREQUENCY;
use super::sine_oscillator::sine;

#[derive(Debug, Clone, Copy, Default)]
pub struct VosimOscillator {
    carrier_phase: f32,
    formant_1_phase: f32,
    formant_2_phase: f32,
    carrier_frequency: f32,
    formant_1_frequency: f32,
    formant_2_frequency: f32,
    carrier_shape: f32,
}

impl VosimOscillator {
    pub fn init(&mut self) {
        *self = Self::default();
    }

    pub fn render(
        &mut self,
        mut carrier_frequency: f32,
        mut formant_frequency_1: f32,
        mut formant_frequency_2: f32,
        carrier_shape: f32,
        out: &mut [f32],
    ) {
        if carrier_frequency >= MAX_FREQUENCY {
            carrier_frequency = MAX_FREQUENCY;
        }
        if formant_frequency_1 >= MAX_FREQUENCY {
            formant_frequency_1 = MAX_FREQUENCY;
        }
        if formant_frequency_2 >= MAX_FREQUENCY {
            formant_frequency_2 = MAX_FREQUENCY;
        }

        let size = out.len();
        let mut f0m = ParameterInterpolator::new(&mut self.carrier_frequency, carrier_frequency, size);
        let mut f1m = ParameterInterpolator::new(&mut self.formant_1_frequency, formant_frequency_1, size);
        let mut f2m = ParameterInterpolator::new(&mut self.formant_2_frequency, formant_frequency_2, size);
        let mut shape_m = ParameterInterpolator::new(&mut self.carrier_shape, carrier_shape, size);

        for o in out.iter_mut() {
            let _f0 = f0m.next();
            let f1 = f1m.next();
            let f2 = f2m.next();

            self.carrier_phase += carrier_frequency;
            if self.carrier_phase >= 1.0 {
                self.carrier_phase -= 1.0;
                let reset_time = self.carrier_phase / carrier_frequency;
                self.formant_1_phase = reset_time * f1;
                self.formant_2_phase = reset_time * f2;
            } else {
                self.formant_1_phase += f1;
                if self.formant_1_phase >= 1.0 {
                    self.formant_1_phase -= 1.0;
                }
                self.formant_2_phase += f2;
                if self.formant_2_phase >= 1.0 {
                    self.formant_2_phase -= 1.0;
                }
            }

            let carrier = sine(self.carrier_phase * 0.5 + 0.25) + 1.0;
            let reset_phase = 0.75 - 0.25 * shape_m.next();
            let reset_amplitude = sine(reset_phase);
            let formant_0 = sine(self.formant_1_phase + reset_phase) - reset_amplitude;
            let formant_1 = sine(self.formant_2_phase + reset_phase) - reset_amplitude;
            *o = carrier * (formant_0 + formant_1) * 0.25 + reset_amplitude;
        }
    }
}
