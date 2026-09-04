//! `plaits/dsp/engine/additive_engine.h` -- additive synthesis with 24
//! (integer-ratio) + 8 (organ, odd + a few even harmonics) partials, rendered
//! in batches of 12 by [`HarmonicOscillator`].

use stmlib::fdsp::one_pole;

use crate::engine::{note_to_frequency, Engine, EngineParameters, PostProcessingSettings};
use crate::oscillator::{sine, HarmonicOscillator};

const HARMONIC_BATCH_SIZE: usize = 12;
const NUM_HARMONICS: usize = 36;
const NUM_HARMONIC_OSCILLATORS: usize = NUM_HARMONICS / HARMONIC_BATCH_SIZE;

#[rustfmt::skip]
const INTEGER_HARMONICS: [usize; 24] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15,
    16, 17, 18, 19, 20, 21, 22, 23,
];

const ORGAN_HARMONICS: [usize; 8] = [0, 1, 2, 3, 5, 7, 9, 11];

fn update_amplitudes(
    centroid: f32,
    slope: f32,
    bumps: f32,
    amplitudes: &mut [f32],
    harmonic_indices: &[usize],
) {
    let num_harmonics = harmonic_indices.len();
    let n = num_harmonics as f32 - 1.0;
    let margin = (1.0 / slope - 1.0) / (1.0 + bumps);
    let center = centroid * (n + margin) - 0.5 * margin;

    let mut sum = 0.001f32;

    for (i, &j) in harmonic_indices.iter().enumerate() {
        let order = (i as f32 - center).abs() * slope;
        let mut gain = 1.0 - order;
        gain += gain.abs();
        gain *= gain;

        let b = 0.25 + order * bumps;
        let bump_factor = 1.0 + sine(b);

        gain *= bump_factor;
        gain *= gain;
        gain *= gain;

        // Not a proper LP filter, because of the normalization below -- but
        // (per the original author) this "incorrect" version sounds better
        // than either LP-ing the normalized spectrum or normalizing the
        // LP-ed one.
        one_pole(&mut amplitudes[j], gain, 0.001);
        sum += amplitudes[j];
    }

    let sum = 1.0 / sum;
    for &j in harmonic_indices {
        amplitudes[j] *= sum;
    }
}

pub struct AdditiveEngine {
    harmonic_oscillator: [HarmonicOscillator<HARMONIC_BATCH_SIZE>; NUM_HARMONIC_OSCILLATORS],
    amplitudes: [f32; NUM_HARMONICS],
}

impl Default for AdditiveEngine {
    fn default() -> Self {
        Self {
            harmonic_oscillator: [HarmonicOscillator::default(); NUM_HARMONIC_OSCILLATORS],
            amplitudes: [0.0; NUM_HARMONICS],
        }
    }
}

impl Engine for AdditiveEngine {
    fn init(&mut self) {
        for h in self.harmonic_oscillator.iter_mut() {
            h.init();
        }
    }

    fn reset(&mut self) {
        self.amplitudes = [0.0; NUM_HARMONICS];
    }

    fn load_user_data(&mut self, _user_data: Option<&'static [u8]>) {}

    fn render(
        &mut self,
        parameters: &EngineParameters,
        out: &mut [f32],
        aux: &mut [f32],
        already_enveloped: bool,
    ) -> bool {
        let f0 = note_to_frequency(parameters.note);

        let centroid = parameters.timbre;
        let raw_bumps = parameters.harmonics;
        let raw_slope = (1.0 - 0.6 * raw_bumps) * parameters.morph;
        let slope = 0.01 + 1.99 * raw_slope * raw_slope * raw_slope;
        let bumps = 16.0 * raw_bumps * raw_bumps;

        update_amplitudes(centroid, slope, bumps, &mut self.amplitudes[0..24], &INTEGER_HARMONICS);

        let batch_0: [f32; HARMONIC_BATCH_SIZE] = self.amplitudes[0..12].try_into().unwrap();
        self.harmonic_oscillator[0].render(1, f0, &batch_0, out);
        let batch_1: [f32; HARMONIC_BATCH_SIZE] = self.amplitudes[12..24].try_into().unwrap();
        self.harmonic_oscillator[1].render(13, f0, &batch_1, out);

        update_amplitudes(centroid, slope, bumps, &mut self.amplitudes[24..36], &ORGAN_HARMONICS);
        let batch_2: [f32; HARMONIC_BATCH_SIZE] = self.amplitudes[24..36].try_into().unwrap();
        self.harmonic_oscillator[2].render(1, f0, &batch_2, aux);

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
