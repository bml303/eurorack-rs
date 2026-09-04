//! `plaits/dsp/engine/noise_engine.h` -- two clocked-noise sources through a
//! multimode (LP<->HP) filter and a pair of band-pass filters.

use stmlib::fdsp::sqrt;
use stmlib::filter::{FilterMode, FrequencyApproximation, Svf};
use stmlib::parameter_interpolator::ParameterInterpolator;
use stmlib::units::semitones_to_ratio;

use crate::dsp::MAX_BLOCK_SIZE;
use crate::engine::{note_to_frequency, trigger_state, Engine, EngineParameters, PostProcessingSettings};
use crate::noise::ClockedNoise;

#[derive(Default)]
pub struct NoiseEngine {
    clocked_noise: [ClockedNoise; 2],
    lp_hp_filter: Svf,
    bp_filter: [Svf; 2],
    previous_f0: f32,
    previous_f1: f32,
    previous_q: f32,
    previous_mode: f32,
    temp_buffer: [f32; MAX_BLOCK_SIZE],
}

impl Engine for NoiseEngine {
    fn init(&mut self) {
        self.clocked_noise[0].init();
        self.clocked_noise[1].init();
        self.lp_hp_filter.init();
        self.bp_filter[0].init();
        self.bp_filter[1].init();
        self.previous_f0 = 0.0;
        self.previous_f1 = 0.0;
        self.previous_q = 0.0;
        self.previous_mode = 0.0;
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
        let f0 = note_to_frequency(parameters.note);
        let f1 = note_to_frequency(parameters.note + parameters.harmonics * 48.0 - 24.0);
        let clock_lowest_note = if parameters.trigger & trigger_state::UNPATCHED != 0 {
            0.0
        } else {
            -24.0
        };
        let clock_f = note_to_frequency(parameters.timbre * (128.0 - clock_lowest_note) + clock_lowest_note);
        let q = 0.5 * semitones_to_ratio(parameters.morph * 120.0);
        let sync = parameters.trigger & trigger_state::RISING_EDGE != 0;

        self.clocked_noise[0].render(sync, clock_f, aux);
        self.clocked_noise[1].render(sync, clock_f * f1 / f0, &mut self.temp_buffer[..size]);

        let mut f0_modulation = ParameterInterpolator::new(&mut self.previous_f0, f0, size);
        let mut f1_modulation = ParameterInterpolator::new(&mut self.previous_f1, f1, size);
        let mut q_modulation = ParameterInterpolator::new(&mut self.previous_q, q, size);
        let mut mode_modulation = ParameterInterpolator::new(&mut self.previous_mode, parameters.harmonics, size);

        for i in 0..size {
            let f0 = f0_modulation.next();
            let f1 = f1_modulation.next();
            let q = q_modulation.next();
            let gain = 1.0 / sqrt((0.5 + q) * 40.0 * f0);
            self.lp_hp_filter.set_f_q(f0, q, FrequencyApproximation::Accurate);
            self.bp_filter[0].set_f_q(f0, q, FrequencyApproximation::Accurate);
            self.bp_filter[1].set_f_q(f1, q, FrequencyApproximation::Accurate);

            let input_1 = aux[i] * gain;
            let input_2 = self.temp_buffer[i] * gain;

            let in_arr = [input_1];
            let mut out_arr = [0.0f32];
            self.lp_hp_filter
                .process_multimode_lp_to_hp(&in_arr, &mut out_arr, mode_modulation.next());
            out[i] = out_arr[0];

            aux[i] = self.bp_filter[0].process(FilterMode::BandPass, input_1)
                + self.bp_filter[1].process(FilterMode::BandPass, input_2);
        }
        already_enveloped
    }

    fn post_processing_settings(&self) -> PostProcessingSettings {
        PostProcessingSettings {
            out_gain: -1.0,
            aux_gain: -1.0,
            already_enveloped: false,
        }
    }
}
