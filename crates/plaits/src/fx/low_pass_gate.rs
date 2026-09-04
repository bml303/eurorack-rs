//! `plaits/dsp/fx/low_pass_gate.h` -- an approximate vactrol low-pass gate:
//! a one-knob gain + low-pass, with a bit of the dry signal bled back in at
//! high `hf_bleed` to fake the vactrol's imperfect filtering.

use stmlib::fdsp::clip16;
use stmlib::filter::{FilterMode, FrequencyApproximation, Svf};
use stmlib::parameter_interpolator::ParameterInterpolator;

#[derive(Debug, Clone, Copy, Default)]
pub struct LowPassGate {
    previous_gain: f32,
    filter: Svf,
}

impl LowPassGate {
    pub fn init(&mut self) {
        self.previous_gain = 0.0;
        self.filter.init();
    }

    /// In-place audio-rate processing.
    pub fn process(&mut self, gain: f32, frequency: f32, hf_bleed: f32, in_out: &mut [f32]) {
        let size = in_out.len();
        let mut gain_modulation = ParameterInterpolator::new(&mut self.previous_gain, gain, size);
        self.filter.set_f_q(frequency, 0.4, FrequencyApproximation::Dirty);
        for s in in_out.iter_mut() {
            let x = *s * gain_modulation.next();
            let lp = self.filter.process(FilterMode::LowPass, x);
            *s = lp + (x - lp) * hf_bleed;
        }
    }

    /// Processes into a strided `i16` output (the final DAC-facing stage).
    pub fn process_to_i16(
        &mut self,
        gain: f32,
        frequency: f32,
        hf_bleed: f32,
        input: &[f32],
        out: &mut [i16],
        stride: usize,
    ) {
        let size = input.len();
        let mut gain_modulation = ParameterInterpolator::new(&mut self.previous_gain, gain, size);
        self.filter.set_f_q(frequency, 0.4, FrequencyApproximation::Dirty);
        for (i, &x) in input.iter().enumerate() {
            let s = x * gain_modulation.next();
            let lp = self.filter.process(FilterMode::LowPass, s);
            out[i * stride] = clip16(1 + (lp + (s - lp) * hf_bleed) as i32) as i16;
        }
    }
}
