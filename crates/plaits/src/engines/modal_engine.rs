//! `plaits/dsp/engine/modal_engine.h` -- one voice of modal (mallet)
//! synthesis.

use stmlib::fdsp::one_pole;

use crate::dsp::MAX_BLOCK_SIZE;
use crate::engine::{note_to_frequency, trigger_state, Engine, EngineParameters, PostProcessingSettings};
use crate::physical_modelling::ModalVoice;

#[derive(Default)]
pub struct ModalEngine {
    voice: ModalVoice,
    temp_buffer: [f32; MAX_BLOCK_SIZE],
    harmonics_lp: f32,
}

impl Engine for ModalEngine {
    fn init(&mut self) {
        self.harmonics_lp = 0.0;
        self.reset();
    }

    fn reset(&mut self) {
        self.voice.init();
    }

    fn load_user_data(&mut self, _user_data: Option<&'static [u8]>) {}

    fn render(
        &mut self,
        parameters: &EngineParameters,
        out: &mut [f32],
        aux: &mut [f32],
        already_enveloped: bool,
    ) -> bool {
        let size = out.len();
        out.fill(0.0);
        aux.fill(0.0);

        one_pole(&mut self.harmonics_lp, parameters.harmonics, 0.01);

        self.voice.render(
            parameters.trigger & trigger_state::UNPATCHED != 0,
            parameters.trigger & trigger_state::RISING_EDGE != 0,
            parameters.accent,
            note_to_frequency(parameters.note),
            self.harmonics_lp,
            parameters.timbre,
            parameters.morph,
            &mut self.temp_buffer[..size],
            out,
            aux,
        );
        already_enveloped
    }

    fn post_processing_settings(&self) -> PostProcessingSettings {
        PostProcessingSettings {
            out_gain: -1.0,
            aux_gain: 0.8,
            already_enveloped: true,
        }
    }
}
