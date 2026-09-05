//! `plaits/dsp/engine/bass_drum_engine.h` -- an 808-style analog bass drum
//! (`out`, overdriven) alongside a naive synthetic one (`aux`).

use crate::drums::{AnalogBassDrum, SyntheticBassDrum};
use crate::engine::{
    note_to_frequency, trigger_state, Engine, EngineParameters, PostProcessingSettings,
};
use crate::fx::Overdrive;

#[derive(Default, Debug)]
pub struct BassDrumEngine {
    analog_bass_drum: AnalogBassDrum,
    synthetic_bass_drum: SyntheticBassDrum,
    overdrive: Overdrive,
}

impl Engine for BassDrumEngine {
    fn init(&mut self) {
        self.analog_bass_drum.init();
        self.synthetic_bass_drum.init();
        self.overdrive.init();
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

        let attack_fm_amount = (parameters.harmonics * 4.0).min(1.0);
        let self_fm_amount = (parameters.harmonics * 4.0 - 1.0).min(1.0).max(0.0);
        let drive = (parameters.harmonics * 2.0 - 1.0).max(0.0) * (1.0 - 16.0 * f0).max(0.0);

        let sustain = parameters.trigger & trigger_state::UNPATCHED != 0;

        self.analog_bass_drum.render(
            sustain,
            parameters.trigger & trigger_state::RISING_EDGE != 0,
            parameters.accent,
            f0,
            parameters.timbre,
            parameters.morph,
            attack_fm_amount,
            self_fm_amount,
            out,
        );

        self.overdrive.process(0.5 + 0.5 * drive, out);

        self.synthetic_bass_drum.render(
            sustain,
            parameters.trigger & trigger_state::RISING_EDGE != 0,
            parameters.accent,
            f0,
            parameters.timbre,
            parameters.morph,
            if sustain {
                parameters.harmonics
            } else {
                0.4 - 0.25 * parameters.morph * parameters.morph
            },
            (parameters.harmonics * 2.0).min(1.0),
            (parameters.harmonics * 2.0 - 1.0).max(0.0),
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
