//! `plaits/dsp/engine/chord_engine.h` -- an inverted chord, voiced across
//! `NUM_VOICES` divide-down (string-synth) + wavetable oscillators that
//! cross-fade by MORPH, with TIMBRE selecting the inversion.

use stmlib::fdsp::one_pole;

use crate::chords::{ChordBank, NUM_VOICES};
use crate::engine::{note_to_frequency, Engine, EngineParameters, PostProcessingSettings};
use crate::oscillator::{StringSynthOscillator, WavetableOscillator};
use crate::resources::WAV_INTEGRATED_WAVES;

const NUM_HARMONICS: usize = 3;
const REGISTRATION_TABLE_SIZE: usize = 8;

#[rustfmt::skip]
const REGISTRATIONS: [[f32; NUM_HARMONICS * 2]; REGISTRATION_TABLE_SIZE] = [
    [0.0, 1.0, 0.0, 0.0, 0.0, 0.0], // Square
    [1.0, 0.0, 0.0, 0.0, 0.0, 0.0], // Saw
    [0.5, 0.0, 0.5, 0.0, 0.0, 0.0], // Saw + saw
    [0.33, 0.0, 0.33, 0.0, 0.33, 0.0], // Full saw
    [0.33, 0.0, 0.0, 0.33, 0.0, 0.33], // Full saw + square hybrid
    [0.5, 0.0, 0.0, 0.0, 0.0, 0.5], // Saw + high square harmo
    [0.0, 0.5, 0.0, 0.0, 0.0, 0.5], // Square + high square harmo
    [0.0, 0.1, 0.1, 0.0, 0.2, 0.6], // Saw+square + high harmo
];

const FADE_POINT: [f32; NUM_VOICES] = [0.55, 0.47, 0.49, 0.51, 0.53];

/// `WAVE(bank, row, column)`: index of a wave in `wav_integrated_waves`
/// (each wave is 132 = 128 + 4 samples).
const fn wave_index(bank: usize, row: usize, column: usize) -> usize {
    bank * 64 + row * 8 + column
}

#[rustfmt::skip]
const WAVETABLE_INDICES: [usize; 15] = [
    wave_index(2, 6, 1), wave_index(2, 6, 6), wave_index(2, 6, 4),
    wave_index(0, 6, 0), wave_index(0, 6, 1), wave_index(0, 6, 2), wave_index(0, 6, 7),
    wave_index(2, 4, 7), wave_index(2, 4, 6), wave_index(2, 4, 5), wave_index(2, 4, 4),
    wave_index(2, 4, 3), wave_index(2, 4, 2), wave_index(2, 4, 1), wave_index(2, 4, 0),
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

#[derive(Debug)]
pub struct ChordEngine {
    divide_down_voice: [StringSynthOscillator; NUM_VOICES],
    wavetable_voice: [WavetableOscillator; NUM_VOICES],
    chords: ChordBank,
    morph_lp: f32,
    timbre_lp: f32,
}

impl Default for ChordEngine {
    fn default() -> Self {
        Self {
            divide_down_voice: [StringSynthOscillator::default(); NUM_VOICES],
            wavetable_voice: [WavetableOscillator::default(); NUM_VOICES],
            chords: ChordBank::default(),
            morph_lp: 0.0,
            timbre_lp: 0.0,
        }
    }
}

impl Engine for ChordEngine {
    fn init(&mut self) {
        for i in 0..NUM_VOICES {
            self.divide_down_voice[i].init();
            self.wavetable_voice[i].init();
        }
        self.chords.init();
        self.morph_lp = 0.0;
        self.timbre_lp = 0.0;
    }

    fn reset(&mut self) {
        self.chords.reset();
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
        one_pole(&mut self.morph_lp, parameters.morph, 0.1);
        one_pole(&mut self.timbre_lp, parameters.timbre, 0.1);

        self.chords.set_chord(parameters.harmonics);

        let mut harmonics = [0.0f32; NUM_HARMONICS * 2 + 1];
        let mut note_amplitudes = [0.0f32; NUM_VOICES];
        let registration = (1.0 - self.morph_lp * 2.15).max(0.0);

        let mut harm6 = [0.0f32; NUM_HARMONICS * 2];
        compute_registration(registration, &mut harm6);
        harmonics[..NUM_HARMONICS * 2].copy_from_slice(&harm6);
        harmonics[NUM_HARMONICS * 2] = 0.0;
        let harmonics7: [f32; 7] = harmonics[..7].try_into().unwrap();

        let mut ratios = [0.0f32; NUM_VOICES];
        let aux_note_mask =
            self.chords
                .compute_chord_inversion(self.timbre_lp, &mut ratios, &mut note_amplitudes);

        out.fill(0.0);
        aux.fill(0.0);

        let f0 = note_to_frequency(parameters.note) * 0.998;
        let waveform = ((self.morph_lp - 0.535) * 2.15).max(0.0);

        let mut wavetable: [&[i16]; 15] = [&WAV_INTEGRATED_WAVES[..132]; 15];
        for (slot, &idx) in wavetable.iter_mut().zip(WAVETABLE_INDICES.iter()) {
            *slot = &WAV_INTEGRATED_WAVES[idx * 132..idx * 132 + 132];
        }

        for note in 0..NUM_VOICES {
            let wavetable_amount = (50.0 * (self.morph_lp - FADE_POINT[note])).clamp(0.0, 1.0);
            let mut divide_down_amount = 1.0 - wavetable_amount;
            let use_aux = (1usize << note) & aux_note_mask as usize != 0;

            let note_f0 = f0 * ratios[note];
            let divide_down_gain = (4.0 - note_f0 * 32.0).clamp(0.0, 1.0);
            divide_down_amount *= divide_down_gain;

            if wavetable_amount != 0.0 {
                let dest: &mut [f32] = if use_aux { aux } else { out };
                self.wavetable_voice[note].render(
                    128,
                    15,
                    true,
                    true,
                    note_f0 * 1.004,
                    note_amplitudes[note] * wavetable_amount,
                    waveform,
                    &wavetable,
                    dest,
                );
            }

            if divide_down_amount != 0.0 {
                let dest: &mut [f32] = if use_aux { aux } else { out };
                self.divide_down_voice[note].render(
                    note_f0,
                    &harmonics7,
                    note_amplitudes[note] * divide_down_amount,
                    dest,
                );
            }
        }

        for i in 0..size {
            out[i] += aux[i];
            aux[i] *= 3.0;
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
