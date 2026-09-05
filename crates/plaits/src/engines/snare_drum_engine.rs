//! `plaits/dsp/engine/snare_drum_engine.h` -- an 808-style analog snare
//! (`out`) alongside a naive synthetic (909-ish) one (`aux`).

use crate::drums::{AnalogSnareDrum, SyntheticSnareDrum};
use crate::engine::{
    note_to_frequency, trigger_state, Engine, EngineParameters, PostProcessingSettings,
};

#[derive(Default, Debug)]
pub struct SnareDrumEngine {
    analog_snare_drum: AnalogSnareDrum,
    synthetic_snare_drum: SyntheticSnareDrum,
}

impl Engine for SnareDrumEngine {
    fn init(&mut self) {
        self.analog_snare_drum.init();
        self.synthetic_snare_drum.init();
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
        let f0 = note_to_frequency(parameters.note);

        self.analog_snare_drum.render(
            parameters.trigger & trigger_state::UNPATCHED != 0,
            parameters.trigger & trigger_state::RISING_EDGE != 0,
            parameters.accent,
            f0,
            parameters.timbre,
            parameters.morph,
            parameters.harmonics,
            out,
        );

        self.synthetic_snare_drum.render(
            parameters.trigger & trigger_state::UNPATCHED != 0,
            parameters.trigger & trigger_state::RISING_EDGE != 0,
            parameters.accent,
            f0,
            parameters.timbre,
            parameters.morph,
            parameters.harmonics,
            aux,
        );
        already_enveloped
    }

    fn post_processing_settings(&self) -> PostProcessingSettings {
        PostProcessingSettings {
            out_gain: 0.8,
            aux_gain: 0.8,
            already_enveloped: true,
        }
    }
}
