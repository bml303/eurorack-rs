//! Stub for `plaits/dsp/engine/speech_engine.h` -- LPC-10/SAM-style word and
//! sentence synthesis plus a "naive" formant-filtered vowel mode.
//!
//! **Not ported.** The real engine pulls in `plaits/dsp/speech/*` (~3000
//! lines: an LPC decoder, word/phoneme banks, a SAM-derived reciter/naive
//! synth path, and a dedicated `NaiveVocoder`/formant filter) -- the other
//! large subsystem left out of this port, alongside
//! [`super::six_op_engine::SixOpEngine`]. `render` outputs silence;
//! `set_prosody_amount`/`set_speed` (called by `Voice` for engine slot 15)
//! are no-ops.
use crate::engine::{Engine, EngineParameters, PostProcessingSettings};

#[derive(Default)]
pub struct SpeechEngine;

impl SpeechEngine {
    #[inline]
    pub fn set_prosody_amount(&mut self, _prosody_amount: f32) {}

    #[inline]
    pub fn set_speed(&mut self, _speed: f32) {}
}

impl Engine for SpeechEngine {
    fn init(&mut self) {}
    fn reset(&mut self) {}
    fn load_user_data(&mut self, _user_data: Option<&'static [u8]>) {}

    fn render(
        &mut self,
        _parameters: &EngineParameters,
        out: &mut [f32],
        aux: &mut [f32],
        already_enveloped: bool,
    ) -> bool {
        out.fill(0.0);
        aux.fill(0.0);
        already_enveloped
    }

    fn post_processing_settings(&self) -> PostProcessingSettings {
        PostProcessingSettings {
            out_gain: -0.7,
            aux_gain: 0.8,
            already_enveloped: false,
        }
    }
}
