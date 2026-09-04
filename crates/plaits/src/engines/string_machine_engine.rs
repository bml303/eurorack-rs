//! `plaits/dsp/engine2/string_machine_engine.h` -- an electro-mechanical/
//! organ "string machine" built from `NUM_NOTES` divide-down oscillators
//! (one per chord note), a mixdown VCF pair, and an ensemble (chorus) FX.

use stmlib::filter::{FilterMode, FrequencyApproximation, NaiveSvf};
use stmlib::fdsp::one_pole;
use stmlib::units::semitones_to_ratio;

use crate::chords::{ChordBank, NUM_NOTES};
use crate::engine::{note_to_frequency, Engine, EngineParameters, PostProcessingSettings};
use crate::fx::Ensemble;
use crate::oscillator::StringSynthOscillator;

const NUM_HARMONICS: usize = 3;
const REGISTRATION_TABLE_SIZE: usize = 11;

#[rustfmt::skip]
const REGISTRATIONS: [[f32; NUM_HARMONICS * 2]; REGISTRATION_TABLE_SIZE] = [
    [1.0, 0.0, 0.0, 0.0, 0.0, 0.0], // Saw
    [0.5, 0.0, 0.5, 0.0, 0.0, 0.0], // Saw + saw
    [0.4, 0.0, 0.2, 0.0, 0.4, 0.0], // Full saw
    [0.3, 0.0, 0.0, 0.3, 0.0, 0.4], // Full saw + square hybrid
    [0.3, 0.0, 0.0, 0.0, 0.0, 0.7], // Saw + high square harmo
    [0.2, 0.0, 0.0, 0.2, 0.0, 0.6], // Weird hybrid
    [0.0, 0.2, 0.1, 0.0, 0.2, 0.5], // Sawsquare high harmo
    [0.0, 0.3, 0.0, 0.3, 0.0, 0.4], // Square high armo
    [0.0, 0.4, 0.0, 0.3, 0.0, 0.3], // Full square
    [0.0, 0.5, 0.0, 0.5, 0.0, 0.0], // Square + Square
    [0.0, 1.0, 0.0, 0.0, 0.0, 0.0], // Square
];

fn compute_registration(registration: f32, amplitudes: &mut [f32; NUM_HARMONICS * 2]) {
    let registration = registration * (REGISTRATION_TABLE_SIZE as f32 - 1.001);
    let registration_integral = registration as usize;
    let registration_fractional = registration - registration_integral as f32;

    for i in 0..NUM_HARMONICS * 2 {
        let a = REGISTRATIONS[registration_integral][i];
        let b = REGISTRATIONS[registration_integral + 1][i];
        amplitudes[i] = a + (b - a) * registration_fractional;
    }
}

pub struct StringMachineEngine {
    chords: ChordBank,
    ensemble: Ensemble,
    divide_down_voice: [StringSynthOscillator; NUM_NOTES],
    svf: [NaiveSvf; 2],
    morph_lp: f32,
    timbre_lp: f32,
}

impl Default for StringMachineEngine {
    fn default() -> Self {
        Self {
            chords: ChordBank::default(),
            ensemble: Ensemble::default(),
            divide_down_voice: [StringSynthOscillator::default(); NUM_NOTES],
            svf: [NaiveSvf::default(); 2],
            morph_lp: 0.0,
            timbre_lp: 0.0,
        }
    }
}

impl Engine for StringMachineEngine {
    fn init(&mut self) {
        for voice in self.divide_down_voice.iter_mut() {
            voice.init();
        }
        self.chords.init();
        self.morph_lp = 0.0;
        self.timbre_lp = 0.0;
        self.svf[0].init();
        self.svf[1].init();
        self.ensemble.init();
    }

    fn reset(&mut self) {
        self.chords.reset();
        self.ensemble.reset();
    }

    fn load_user_data(&mut self, _user_data: Option<&'static [u8]>) {}

    fn render(
        &mut self,
        parameters: &EngineParameters,
        out: &mut [f32],
        aux: &mut [f32],
        already_enveloped: bool,
    ) -> bool {
        one_pole(&mut self.morph_lp, parameters.morph, 0.1);
        one_pole(&mut self.timbre_lp, parameters.timbre, 0.1);

        self.chords.set_chord(parameters.harmonics);

        let mut harmonics = [0.0f32; NUM_HARMONICS * 2 + 1];
        let registration = self.morph_lp.max(0.0);
        let mut harm6 = [0.0f32; NUM_HARMONICS * 2];
        compute_registration(registration, &mut harm6);
        harmonics[..NUM_HARMONICS * 2].copy_from_slice(&harm6);
        harmonics[NUM_HARMONICS * 2] = 0.0;
        let harmonics7: [f32; 7] = harmonics;

        // Render string/organ sound.
        out.fill(0.0);
        aux.fill(0.0);
        let f0 = note_to_frequency(parameters.note) * 0.998;
        for note in 0..NUM_NOTES {
            let note_f0 = f0 * self.chords.ratio(note);
            let divide_down_gain = (4.0 - note_f0 * 32.0).clamp(0.0, 1.0);
            let dest: &mut [f32] = if note & 1 != 0 { &mut *aux } else { &mut *out };
            self.divide_down_voice[note].render(note_f0, &harmonics7, 0.25 * divide_down_gain, dest);
        }

        // Pass through VCF.
        let cutoff = 2.2 * f0 * semitones_to_ratio(120.0 * parameters.timbre);
        self.svf[0].set_f_q(cutoff, 1.0, FrequencyApproximation::Dirty);
        self.svf[1].set_f_q(cutoff * 1.5, 1.0, FrequencyApproximation::Dirty);

        // Mixdown.
        for i in 0..out.len() {
            let l = self.svf[0].process(FilterMode::LowPass, out[i]);
            let r = self.svf[1].process(FilterMode::LowPass, aux[i]);
            out[i] = 0.66 * l + 0.33 * r;
            aux[i] = 0.66 * r + 0.33 * l;
        }

        // Ensemble FX.
        let amount = (parameters.timbre - 0.5).abs() * 2.0;
        let depth = 0.35 + 0.65 * parameters.timbre;
        self.ensemble.set_amount(amount);
        self.ensemble.set_depth(depth);
        self.ensemble.process(out, aux);

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
