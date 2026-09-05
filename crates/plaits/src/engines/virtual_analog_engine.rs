//! `plaits/dsp/engine/virtual_analog_engine.h` -- 2 variable-shape oscillators
//! with sync and crossfading.
//!
//! The C keeps three alternate `VA_VARIANT` implementations behind an `#if`,
//! selected at compile time; the shipped firmware builds `VA_VARIANT == 2`
//! (the other two are dead code even in C), so that's the only one ported:
//! a self-synced variable square (TIMBRE) mixed with a variable saw (MORPH)
//! on `out`, and a hard-synced dual variable-waveshape pair (detuned by
//! HARMONICS, sync'ed by TIMBRE) on `aux`.

extern crate alloc;

use alloc::boxed::Box;
use alloc::vec;

use stmlib::parameter_interpolator::ParameterInterpolator;

use crate::engine::{note_to_frequency, Engine, EngineParameters, PostProcessingSettings};
use crate::oscillator::{VariableSawOscillator, VariableShapeOscillator};

#[rustfmt::skip]
const INTERVALS: [f32; 5] = [0.0, 7.01, 12.01, 19.01, 24.01];

#[inline]
fn squash(x: f32) -> f32 {
    x * x * (3.0 - 2.0 * x)
}

fn compute_detuning(detune: f32) -> f32 {
    let detune = (2.05 * detune - 1.025).clamp(-1.0, 1.0);
    let sign = if detune < 0.0 { -1.0 } else { 1.0 };
    let detune = detune * sign * 3.9999;
    let detune_integral = detune as usize;
    let detune_fractional = detune - detune_integral as f32;

    let a = INTERVALS[detune_integral];
    let b = INTERVALS[detune_integral + 1];
    (a + (b - a) * squash(squash(detune_fractional))) * sign
}

#[derive(Default, Debug)]
pub struct VirtualAnalogEngine {
    primary: VariableShapeOscillator,
    auxiliary: VariableShapeOscillator,
    sync: VariableShapeOscillator,
    variable_saw: VariableSawOscillator,
    auxiliary_amount: f32,
    xmod_amount: f32,
    temp_buffer: Box<[f32]>,
}

impl VirtualAnalogEngine {
    pub fn new(block_size: usize) -> Self {
        Self {
            primary: VariableShapeOscillator::default(),
            auxiliary: VariableShapeOscillator::default(),
            sync: VariableShapeOscillator::default(),
            variable_saw: VariableSawOscillator::default(),
            auxiliary_amount: 0.0,
            xmod_amount: 0.0,
            temp_buffer: vec![0.0; block_size].into_boxed_slice(),
        }
    }
}

impl Engine for VirtualAnalogEngine {
    fn init(&mut self) {
        self.primary.init();
        self.auxiliary.init();
        self.auxiliary.set_master_phase(0.25);
        self.sync.init();
        self.variable_saw.init();
        self.auxiliary_amount = 0.0;
        self.xmod_amount = 0.0;
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
        let sync_amount = parameters.timbre * parameters.timbre;
        let auxiliary_detune = compute_detuning(parameters.harmonics);
        let primary_f = note_to_frequency(parameters.note);
        let auxiliary_f = note_to_frequency(parameters.note + auxiliary_detune);
        let primary_sync_f = note_to_frequency(parameters.note + sync_amount * 48.0);
        let auxiliary_sync_f =
            note_to_frequency(parameters.note + auxiliary_detune + sync_amount * 48.0);

        let shape = (parameters.morph * 1.5).clamp(0.0, 1.0);
        let pw = (0.5 + (parameters.morph - 0.66) * 1.46).clamp(0.5, 0.995);

        // Monster sync into `aux`.
        self.primary
            .render_synced(primary_f, primary_sync_f, pw, shape, out);
        self.auxiliary
            .render_synced(auxiliary_f, auxiliary_sync_f, pw, shape, aux);
        for i in 0..size {
            aux[i] = (aux[i] - out[i]) * 0.5;
        }

        // Double varishape into `out`.
        let square_pw = (1.3 * parameters.timbre - 0.15).clamp(0.005, 0.5);
        let square_sync_ratio = if parameters.timbre < 0.5 {
            0.0
        } else {
            (parameters.timbre - 0.5) * (parameters.timbre - 0.5) * 4.0 * 48.0
        };
        let square_gain = (parameters.timbre * 8.0).min(1.0);

        let mut saw_pw = if parameters.morph < 0.5 {
            parameters.morph + 0.5
        } else {
            1.0 - (parameters.morph - 0.5) * 2.0
        };
        saw_pw *= 1.1;
        saw_pw = saw_pw.clamp(0.005, 1.0);

        let saw_shape = (10.0 - 21.0 * parameters.morph).clamp(0.0, 1.0);
        let saw_gain = (8.0 * (1.0 - parameters.morph)).clamp(0.02, 1.0);

        let square_sync_f = note_to_frequency(parameters.note + square_sync_ratio);

        self.sync.render_synced(
            primary_f,
            square_sync_f,
            square_pw,
            1.0,
            &mut self.temp_buffer[..size],
        );
        self.variable_saw
            .render(auxiliary_f, saw_pw, saw_shape, out);

        let norm = 1.0 / square_gain.max(saw_gain);

        let mut square_gain_modulation =
            ParameterInterpolator::new(&mut self.auxiliary_amount, square_gain * 0.3 * norm, size);
        let mut saw_gain_modulation =
            ParameterInterpolator::new(&mut self.xmod_amount, saw_gain * 0.5 * norm, size);

        for i in 0..size {
            out[i] = out[i] * saw_gain_modulation.next()
                + square_gain_modulation.next() * self.temp_buffer[i];
        }

        already_enveloped
    }

    fn post_processing_settings(&self) -> PostProcessingSettings {
        PostProcessingSettings {
            out_gain: 0.8,
            aux_gain: 0.8,
            already_enveloped: false,
        }
    }
}
