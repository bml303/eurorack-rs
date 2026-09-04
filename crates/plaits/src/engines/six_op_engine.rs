//! Stub for `plaits/dsp/engine2/six_op_engine.h` -- the 6-operator DX7-style
//! FM engine (registered 3 times in the real firmware, at engine slots 2-4,
//! differentiated only by which FM patch bank is loaded via
//! [`Engine::load_user_data`]).
//!
//! **Not ported.** The real engine pulls in `plaits/dsp/fm/*` (~2000 lines:
//! an FM algorithm graph, operator/envelope/LFO/pitch-envelope modules, a
//! dedicated resonant filter, and SysEx patch parsing) plus
//! `six_op_engine.cc` (~180 lines) -- by far the largest and most
//! self-contained subsystem left out of this port, alongside
//! [`super::speech_engine::SpeechEngine`]. `render` outputs silence.
use crate::engine::{Engine, EngineParameters, PostProcessingSettings};

#[derive(Default)]
pub struct SixOpEngine;

impl Engine for SixOpEngine {
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
            out_gain: 1.0,
            aux_gain: 1.0,
            already_enveloped: true,
        }
    }
}
