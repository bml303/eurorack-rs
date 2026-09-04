//! `plaits/dsp/engine/hi_hat_engine.h` -- two 808-style hi-hats: a faithful
//! one (`out`) and a more metallic ring-modulated one (`aux`).

use crate::dsp::MAX_BLOCK_SIZE;
use crate::drums::{HiHat, MetallicNoise, Vca};
use crate::engine::{note_to_frequency, trigger_state, Engine, EngineParameters, PostProcessingSettings};

pub struct HiHatEngine {
    hi_hat_1: HiHat,
    hi_hat_2: HiHat,
    temp_buffer: [f32; MAX_BLOCK_SIZE * 2],
}

impl Default for HiHatEngine {
    fn default() -> Self {
        Self {
            hi_hat_1: HiHat::new(MetallicNoise::Square, Vca::Swing, true, false),
            hi_hat_2: HiHat::new(MetallicNoise::RingMod, Vca::Linear, false, true),
            temp_buffer: [0.0; MAX_BLOCK_SIZE * 2],
        }
    }
}

impl Engine for HiHatEngine {
    fn init(&mut self) {
        self.hi_hat_1.init();
        self.hi_hat_2.init();
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

        let (temp_1, temp_2) = self.temp_buffer.split_at_mut(size);
        let temp_1 = &mut temp_1[..size];
        let temp_2 = &mut temp_2[..size];

        self.hi_hat_1.render(
            parameters.trigger & trigger_state::UNPATCHED != 0,
            parameters.trigger & trigger_state::RISING_EDGE != 0,
            parameters.accent,
            f0,
            parameters.timbre,
            parameters.morph,
            parameters.harmonics,
            temp_1,
            temp_2,
            out,
        );

        self.hi_hat_2.render(
            parameters.trigger & trigger_state::UNPATCHED != 0,
            parameters.trigger & trigger_state::RISING_EDGE != 0,
            parameters.accent,
            f0,
            parameters.timbre,
            parameters.morph,
            parameters.harmonics,
            temp_1,
            temp_2,
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
