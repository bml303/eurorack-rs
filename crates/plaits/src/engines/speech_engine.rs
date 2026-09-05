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

extern crate alloc;

use alloc::boxed::Box;
use alloc::vec;

use crate::dsp::SAMPLE_RATE;
use crate::engine::{
    Engine, EngineParameters, PostProcessingSettings, note_to_frequency, trigger_state,
};
use crate::speech::lpc_speech_synth_controller::LpcSpeechSynthController;
use crate::speech::lpc_speech_synth_words::NUM_WORD_BANKS;
use crate::speech::naive_speech_synth::NaiveSpeechSynth;
use crate::speech::sam_speech_synth::SamSpeechSynth;
use crate::utils::hysteresis_quantizer::HysteresisQuantizer2;

#[derive(Default, Debug, Clone)]
pub struct SpeechEngine<'a> {
    word_bank_quantizer: HysteresisQuantizer2,

    naive_speech_synth: NaiveSpeechSynth,
    sam_speech_synth: SamSpeechSynth,

    lpc_speech_synth_controller: LpcSpeechSynthController<'a>,
    temp_buffer_1: Box<[f32]>,
    temp_buffer_2: Box<[f32]>,
    prosody_amount: f32,
    speed: f32,
}

impl SpeechEngine<'_> {
    pub fn new(block_size: usize) -> Self {
        Self {
            word_bank_quantizer: HysteresisQuantizer2::new(),
            naive_speech_synth: NaiveSpeechSynth::new(),
            sam_speech_synth: SamSpeechSynth::new(),
            lpc_speech_synth_controller: LpcSpeechSynthController::new(),
            temp_buffer_1: vec![0.0; block_size].into_boxed_slice(),
            temp_buffer_2: vec![0.0; block_size].into_boxed_slice(),
            prosody_amount: 0.0,
            speed: 1.0,
        }
    }
    pub fn set_prosody_amount(&mut self, prosody_amount: f32) {
        self.prosody_amount = prosody_amount;
    }

    pub fn set_speed(&mut self, speed: f32) {
        self.speed = speed;
    }
}

impl Engine for SpeechEngine<'_> {
    fn init(&mut self) {
        self.sam_speech_synth.init(SAMPLE_RATE);
        self.naive_speech_synth.init();
        self.lpc_speech_synth_controller.init(SAMPLE_RATE);
        self.word_bank_quantizer
            .init(NUM_WORD_BANKS as i32 + 1, 0.1, false);
        self.prosody_amount = 0.0;
        self.speed = 0.0;
        self.reset();
    }
    fn reset(&mut self) {
        self.lpc_speech_synth_controller.reset();
    }
    fn load_user_data(&mut self, _user_data: Option<&'static [u8]>) {}

    fn render(
        &mut self,
        parameters: &EngineParameters,
        out: &mut [f32],
        aux: &mut [f32],
        #[allow(unused_assignments)] mut already_enveloped: bool,
    ) -> bool {
        let f0 = note_to_frequency(parameters.note);

        let group = parameters.harmonics * 6.0;

        let sustain = matches!(parameters.trigger, trigger_state::UNPATCHED);
        let trigger = matches!(parameters.trigger, trigger_state::RISING_EDGE);

        // Interpolates between the 3 models: naive, SAM, LPC.
        if group <= 2.0 {
            already_enveloped = false;

            let mut blend = group;

            if group <= 1.0 {
                self.naive_speech_synth.render(
                    trigger,
                    f0,
                    parameters.morph,
                    parameters.timbre,
                    &mut self.temp_buffer_1,
                    aux,
                    out,
                );
            } else {
                self.lpc_speech_synth_controller.render(
                    sustain,
                    trigger,
                    -1,
                    f0,
                    0.0,
                    0.0,
                    parameters.morph,
                    parameters.timbre,
                    1.0,
                    aux,
                    out,
                );
                blend = 2.0 - blend;
            }

            self.sam_speech_synth.render(
                sustain,
                f0,
                parameters.morph,
                parameters.timbre,
                &mut self.temp_buffer_1,
                &mut self.temp_buffer_2,
            );

            blend = blend * blend * (3.0 - 2.0 * blend);
            blend = blend * blend * (3.0 - 2.0 * blend);

            for (i, (out_sample, aux_sample)) in out.iter_mut().zip(aux.iter_mut()).enumerate() {
                *aux_sample += (self.temp_buffer_1[i] - *aux_sample) * blend;
                *out_sample += (self.temp_buffer_2[i] - *out_sample) * blend;
            }
        } else {
            // Change phonemes/words for LPC.
            let word_bank = self.word_bank_quantizer.process((group - 2.0) * 0.275) - 1;

            let replay_prosody = word_bank >= 0 && !sustain;

            already_enveloped = replay_prosody;

            self.lpc_speech_synth_controller.render(
                sustain,
                trigger,
                word_bank,
                f0,
                self.prosody_amount,
                self.speed,
                parameters.morph,
                parameters.timbre,
                if replay_prosody {
                    parameters.accent
                } else {
                    1.0
                },
                aux,
                out,
            );
        }
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
