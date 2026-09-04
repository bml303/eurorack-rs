//! `plaits/dsp/engine2/virtual_analog_vcf_engine.h` -- a saw/PW-square VA
//! oscillator + sub-oscillator, mixed and pushed through a 2-stage SVF (a
//! resonant low-pass followed by a second, less resonant low-pass stage),
//! with the high-pass output going to `aux`.

use stmlib::fdsp::soft_clip;
use stmlib::filter::{FilterMode, FrequencyApproximation, Svf};
use stmlib::parameter_interpolator::ParameterInterpolator;
use stmlib::units::semitones_to_ratio;

use crate::engine::{note_to_frequency, Engine, EngineParameters, PostProcessingSettings};
use crate::oscillator::VariableShapeOscillator;

#[derive(Default)]
pub struct VirtualAnalogVcfEngine {
    svf: [Svf; 2],
    oscillator: VariableShapeOscillator,
    sub_oscillator: VariableShapeOscillator,

    previous_cutoff: f32,
    previous_stage2_gain: f32,
    previous_q: f32,
    previous_gain: f32,
    previous_sub_gain: f32,
}

impl Engine for VirtualAnalogVcfEngine {
    fn init(&mut self) {
        self.oscillator.init();
        self.sub_oscillator.init();

        self.svf[0].init();
        self.svf[1].init();

        self.previous_sub_gain = 0.0;
        self.previous_cutoff = 0.0;
        self.previous_stage2_gain = 0.0;
        self.previous_q = 0.0;
        self.previous_gain = 0.0;
    }

    fn reset(&mut self) {}

    fn load_user_data(&mut self, _user_data: Option<&'static [u8]>) {}

    fn render(
        &mut self,
        parameters: &EngineParameters,
        out: &mut [f32],
        aux: &mut [f32],
        already_enveloped: bool,
    ) -> bool {
        let size = out.len();
        // VA Oscillator (saw or PW square) + sub
        let f0 = note_to_frequency(parameters.note);

        let mut shape = (parameters.morph - 0.25) * 2.0 + 0.5;
        shape = shape.clamp(0.5, 1.0);

        let mut pw = (parameters.morph - 0.5) * 2.0 + 0.5;
        if parameters.morph > 0.75 {
            pw = 2.5 - parameters.morph * 2.0;
        }
        pw = pw.clamp(0.5, 0.98);

        let sub_gain = ((parameters.morph - 0.5).abs() - 0.3).max(0.0) * 5.0;

        self.oscillator.render(f0, pw, shape, out);
        self.sub_oscillator.render(f0 * 0.501, 0.5, 1.0, aux);

        let cutoff = f0 * semitones_to_ratio((parameters.timbre - 0.2) * 120.0);

        let mut stage2_gain = 1.0 - (parameters.harmonics - 0.4) * 4.0;
        stage2_gain = stage2_gain.clamp(0.0, 1.0);

        let resonance = 2.667 * ((parameters.harmonics - 0.5).abs() - 0.125).max(0.0);
        let resonance_sqr = resonance * resonance;
        let q = resonance_sqr * resonance_sqr * 48.0;
        let mut gain = (parameters.harmonics - 0.7) + 0.85;
        gain = gain.clamp(0.7 - resonance_sqr * 0.3, 1.0);

        let mut sub_gain_modulation = ParameterInterpolator::new(&mut self.previous_sub_gain, sub_gain, size);
        let mut cutoff_modulation = ParameterInterpolator::new(&mut self.previous_cutoff, cutoff, size);
        let mut stage2_gain_modulation =
            ParameterInterpolator::new(&mut self.previous_stage2_gain, stage2_gain, size);
        let mut q_modulation = ParameterInterpolator::new(&mut self.previous_q, q, size);
        let mut gain_modulation = ParameterInterpolator::new(&mut self.previous_gain, gain, size);

        for i in 0..size {
            let cutoff = cutoff_modulation.next().min(0.25);
            let q = q_modulation.next();
            let stage2_gain = stage2_gain_modulation.next();

            self.svf[0].set_f_q(cutoff, 0.5 + q, FrequencyApproximation::Fast);
            self.svf[1].set_f_q(cutoff, 0.5 + 0.025 * q, FrequencyApproximation::Fast);

            let gain = gain_modulation.next();
            let input = soft_clip((out[i] + aux[i] * sub_gain_modulation.next()) * gain);

            let (lp, hp) = self.svf[0].process_dual(FilterMode::LowPass, FilterMode::HighPass, input);

            let mut lp = soft_clip(lp * gain);
            lp += stage2_gain * (soft_clip(self.svf[1].process(FilterMode::LowPass, lp)) - lp);

            out[i] = lp;
            aux[i] = soft_clip(hp * gain);
        }

        already_enveloped
    }

    fn post_processing_settings(&self) -> PostProcessingSettings {
        PostProcessingSettings {
            out_gain: 1.0,
            aux_gain: 1.0,
            already_enveloped: false,
        }
    }
}
