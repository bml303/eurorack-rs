//! `plaits/dsp/engine/string_engine.h` -- 3 [`StringVoice`]s, round-robin
//! triggered, with a short delay line so a newly-struck string picks up the
//! frequency the player was dialing in ~14 samples ago (compensates trigger
//! jitter from external sequencers).

extern crate alloc;

use alloc::boxed::Box;
use alloc::vec;

use stmlib::DelayLine;

use crate::engine::{
    note_to_frequency, trigger_state, Engine, EngineParameters, PostProcessingSettings,
};
use crate::physical_modelling::StringVoice;

const NUM_STRINGS: usize = 3;

#[derive(Default, Debug)]
pub struct StringEngine {
    voice: [StringVoice; NUM_STRINGS],
    f0: [f32; NUM_STRINGS],
    f0_delay: DelayLine<16>,
    active_string: usize,
    temp_buffer: Box<[f32]>,
}

impl StringEngine {
    pub fn new(block_size: usize) -> Self {
        Self {
            voice: Default::default(),
            f0: [0.01; NUM_STRINGS],
            f0_delay: DelayLine::default(),
            active_string: NUM_STRINGS - 1,
            temp_buffer: vec![0.0; block_size].into_boxed_slice(),
        }
    }
}

impl Engine for StringEngine {
    fn init(&mut self) {
        for v in self.voice.iter_mut() {
            v.init();
        }
        self.f0 = [0.01; NUM_STRINGS];
        self.active_string = NUM_STRINGS - 1;
        self.f0_delay.init();
    }

    fn reset(&mut self) {
        self.f0_delay.reset();
        for v in self.voice.iter_mut() {
            v.reset();
        }
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
        if parameters.trigger & trigger_state::RISING_EDGE != 0 {
            // 8 in the original firmware version; 14 fixes a MicroBrute
            // interoperability problem (per the C's comment history).
            self.f0[self.active_string] = self.f0_delay.read_at(14);
            self.active_string = (self.active_string + 1) % NUM_STRINGS;
        }

        let f0 = note_to_frequency(parameters.note);
        self.f0[self.active_string] = f0;
        self.f0_delay.write(f0);

        out.fill(0.0);
        aux.fill(0.0);

        for (i, v) in self.voice.iter_mut().enumerate() {
            v.render(
                parameters.trigger & trigger_state::UNPATCHED != 0 && i == self.active_string,
                parameters.trigger & trigger_state::RISING_EDGE != 0 && i == self.active_string,
                parameters.accent,
                self.f0[i],
                parameters.harmonics,
                parameters.timbre * parameters.timbre,
                parameters.morph,
                &mut self.temp_buffer[..size],
                out,
                aux,
            );
        }
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
