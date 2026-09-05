//! `plaits/dsp/engine2/phase_distortion_engine.h` -- Casio CZ-style phase
//! distortion: a `VariableShapeOscillator` in phase-output mode drives the
//! phase of a sine, once hard-synced to the fundamental (`out`) and once
//! free-running (`aux`).

extern crate alloc;

use alloc::boxed::Box;
use alloc::vec;

use crate::engine::{note_to_frequency, Engine, EngineParameters, PostProcessingSettings};
use crate::oscillator::{sine, VariableShapeOscillator};
use crate::resources::LUT_FM_FREQUENCY_QUANTIZER;
use stmlib::fdsp::interpolate;
use stmlib::units::semitones_to_ratio;

#[derive(Default, Debug)]
pub struct PhaseDistortionEngine {
    shaper: VariableShapeOscillator,
    modulator: VariableShapeOscillator,
    temp_buffer_1: Box<[f32]>,
    temp_buffer_2: Box<[f32]>,
}

impl PhaseDistortionEngine {
    pub fn new(block_size: usize) -> Self {
        Self {
            shaper: VariableShapeOscillator::default(),
            modulator: VariableShapeOscillator::default(),
            temp_buffer_1: vec![0.0; block_size * 2].into_boxed_slice(),
            temp_buffer_2: vec![0.0; block_size * 2].into_boxed_slice(),
        }
    }
}

impl Engine for PhaseDistortionEngine {
    fn init(&mut self) {
        self.modulator.init();
        self.shaper.init();
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
        let f0 = 0.5 * note_to_frequency(parameters.note);
        let modulator_f = 0.25f32.min(
            f0 * semitones_to_ratio(interpolate(
                &LUT_FM_FREQUENCY_QUANTIZER,
                parameters.harmonics,
                128.0,
            )),
        );
        let pw = 0.5 + parameters.morph * 0.49;
        let amount = 8.0 * parameters.timbre * parameters.timbre * (1.0 - modulator_f * 3.8);

        // Upsample by 2x
        let synced = &mut self.temp_buffer_1[..2 * size];
        let free_running = &mut self.temp_buffer_2[..2 * size];

        self.shaper
            .render_full(true, true, f0, modulator_f, pw, 0.0, amount, synced);
        self.modulator
            .render_full(false, true, f0, modulator_f, pw, 0.0, amount, free_running);

        for i in 0..size {
            out[i] = 0.5 * sine(synced[2 * i] + 0.25);
            out[i] += 0.5 * sine(synced[2 * i + 1] + 0.25);

            aux[i] = 0.5 * sine(free_running[2 * i] + 0.25);
            aux[i] += 0.5 * sine(free_running[2 * i + 1] + 0.25);
        }

        already_enveloped
    }

    fn post_processing_settings(&self) -> PostProcessingSettings {
        PostProcessingSettings {
            out_gain: 0.7,
            aux_gain: 0.7,
            already_enveloped: false,
        }
    }
}
