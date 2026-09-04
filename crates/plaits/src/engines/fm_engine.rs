//! `plaits/dsp/engine/fm_engine.h` -- classic 2-operator FM (as in Braids,
//! Rings, Elements), 4x oversampled with a small FIR downsampler to keep the
//! feedback path alias-free.

use stmlib::fdsp::{interpolate, one_pole};

use crate::dsp::A0;
use crate::downsampler::{Downsampler, OVERSAMPLING};
use crate::engine::{note_to_frequency, Engine, EngineParameters, PostProcessingSettings};
use crate::oscillator::sine_pm;
use crate::resources::LUT_FM_FREQUENCY_QUANTIZER;
use stmlib::parameter_interpolator::ParameterInterpolator;

#[derive(Default)]
pub struct FmEngine {
    carrier_phase: u32,
    modulator_phase: u32,
    sub_phase: u32,
    previous_carrier_frequency: f32,
    previous_modulator_frequency: f32,
    previous_amount: f32,
    previous_feedback: f32,
    previous_sample: f32,
    sub_fir: f32,
    carrier_fir: f32,
}

impl Engine for FmEngine {
    fn init(&mut self) {
        self.carrier_phase = 0;
        self.modulator_phase = 0;
        self.sub_phase = 0;
        self.previous_carrier_frequency = A0;
        self.previous_modulator_frequency = A0;
        self.previous_amount = 0.0;
        self.previous_feedback = 0.0;
        self.previous_sample = 0.0;
        self.sub_fir = 0.0;
        self.carrier_fir = 0.0;
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
        let note = parameters.note - 24.0;

        let ratio = interpolate(&LUT_FM_FREQUENCY_QUANTIZER, parameters.harmonics, 128.0);

        let modulator_note = note + ratio;
        let target_modulator_frequency = note_to_frequency(modulator_note).clamp(0.0, 0.5);

        // Reduce the maximum FM index for high-pitched notes, to prevent aliasing.
        let mut hf_taming = (1.0 - (modulator_note - 72.0) * 0.025).clamp(0.0, 1.0);
        hf_taming *= hf_taming;

        let mut carrier_frequency =
            ParameterInterpolator::new(&mut self.previous_carrier_frequency, note_to_frequency(note), size);
        let mut modulator_frequency = ParameterInterpolator::new(
            &mut self.previous_modulator_frequency,
            target_modulator_frequency,
            size,
        );
        let mut amount_modulation = ParameterInterpolator::new(
            &mut self.previous_amount,
            2.0 * parameters.timbre * parameters.timbre * hf_taming,
            size,
        );
        let mut feedback_modulation =
            ParameterInterpolator::new(&mut self.previous_feedback, 2.0 * parameters.morph - 1.0, size);

        let mut carrier_downsampler = Downsampler::new(&mut self.carrier_fir);
        let mut sub_downsampler = Downsampler::new(&mut self.sub_fir);

        const MAX_UINT32: f32 = 4_294_967_296.0;

        for i in 0..size {
            let amount = amount_modulation.next();
            let feedback = feedback_modulation.next();
            let phase_feedback = if feedback < 0.0 { 0.5 * feedback * feedback } else { 0.0 };
            let carrier_increment = (MAX_UINT32 * carrier_frequency.next()) as u32;
            let modulator_frequency_now = modulator_frequency.next();

            for j in 0..OVERSAMPLING {
                self.modulator_phase = self.modulator_phase.wrapping_add(
                    (MAX_UINT32 * modulator_frequency_now * (1.0 + self.previous_sample * phase_feedback))
                        as u32,
                );
                self.carrier_phase = self.carrier_phase.wrapping_add(carrier_increment);
                self.sub_phase = self.sub_phase.wrapping_add(carrier_increment >> 1);
                let modulator_fb = if feedback > 0.0 { 0.25 * feedback * feedback } else { 0.0 };
                let modulator = sine_pm(self.modulator_phase, modulator_fb * self.previous_sample);
                let carrier = sine_pm(self.carrier_phase, amount * modulator);
                let sub = sine_pm(self.sub_phase, amount * carrier * 0.25);
                one_pole(&mut self.previous_sample, carrier, 0.05);
                carrier_downsampler.accumulate(j, carrier);
                sub_downsampler.accumulate(j, sub);
            }

            out[i] = carrier_downsampler.read();
            aux[i] = sub_downsampler.read();
        }
        already_enveloped
    }

    fn post_processing_settings(&self) -> PostProcessingSettings {
        PostProcessingSettings {
            out_gain: 0.6,
            aux_gain: 0.6,
            already_enveloped: false,
        }
    }
}
