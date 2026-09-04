//! `plaits/dsp/engine/grain_engine.h` -- windowed sine "grainlet" segments,
//! plus a Z-oscillator formant on `aux`. The C keeps a commented-out
//! `VOSIMOscillator` alternate; not ported (dead code in C too).

use stmlib::filter::{FilterMode, FrequencyApproximation, OnePole};
use stmlib::units::semitones_to_ratio;

use crate::engine::{note_to_frequency, Engine, EngineParameters, PostProcessingSettings};
use crate::oscillator::{GrainletOscillator, ZOscillator};

#[derive(Default)]
pub struct GrainEngine {
    grainlet: [GrainletOscillator; 2],
    z_oscillator: ZOscillator,
    dc_blocker: [OnePole; 2],
}

impl Engine for GrainEngine {
    fn init(&mut self) {
        self.grainlet[0].init();
        self.grainlet[1].init();
        self.z_oscillator.init();
        self.dc_blocker[0].init();
        self.dc_blocker[1].init();
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
        let root = parameters.note;
        let f0 = note_to_frequency(root);

        let f1 = note_to_frequency(24.0 + 84.0 * parameters.timbre);
        let ratio = semitones_to_ratio(-24.0 + 48.0 * parameters.harmonics);
        let carrier_bleed = if parameters.harmonics < 0.5 {
            1.0 - 2.0 * parameters.harmonics
        } else {
            0.0
        };
        let carrier_bleed_fixed = carrier_bleed * (2.0 - carrier_bleed);
        let carrier_shape =
            0.33 + (parameters.morph - 0.33) * (1.0 - f0 * 24.0).max(0.0);

        self.grainlet[0].render(f0, f1, carrier_shape, carrier_bleed_fixed, out);
        self.grainlet[1].render(f0, f1 * ratio, carrier_shape, carrier_bleed_fixed, aux);
        self.dc_blocker[0].set_f(0.3 * f0, FrequencyApproximation::Dirty);
        for i in 0..size {
            out[i] = self.dc_blocker[0].process(FilterMode::HighPass, out[i] + aux[i]);
        }

        let cutoff = note_to_frequency(root + 96.0 * parameters.timbre);
        self.z_oscillator
            .render(f0, cutoff, parameters.morph, parameters.harmonics, aux);

        self.dc_blocker[1].set_f(0.3 * f0, FrequencyApproximation::Dirty);
        self.dc_blocker[1].process_in_place(FilterMode::HighPass, aux);
        already_enveloped
    }

    fn post_processing_settings(&self) -> PostProcessingSettings {
        PostProcessingSettings {
            out_gain: 0.7,
            aux_gain: 0.6,
            already_enveloped: false,
        }
    }
}
