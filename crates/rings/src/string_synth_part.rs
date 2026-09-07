//! `rings/dsp/string_synth_part.{h,cc}` -- the "Disastrous Peace" easter egg: a
//! polyphonic PolyBLEP string-ensemble / organ. Each strum assigns a note to a
//! voice group; the group plays a chord of stacked-harmonic voices, then a
//! selectable effect (formant filter, chorus, ensemble or reverb) and a
//! limiter.

use stmlib::filter::{FilterMode, FrequencyApproximation, Svf};
use stmlib::units::semitones_to_ratio;

use crate::dsp::{A3, MAX_BLOCK_SIZE, SAMPLE_RATE};
use crate::fx::{Chorus, Ensemble, Reverb};
use crate::limiter::Limiter;
use crate::note_filter::NoteFilter;
use crate::string_synth_envelope::{
    FLAG_FALLING_EDGE, FLAG_GATE, FLAG_RISING_EDGE, StringSynthEnvelope,
};
use crate::string_synth_voice::StringSynthVoice;

const MAX_POLYPHONY: usize = 4;
const NUM_VOICES: usize = 12;
const MAX_CHORD_SIZE: usize = 8;
const NUM_HARMONICS: usize = 3;
const NUM_FORMANTS: usize = 3;
const NUM_CHORDS: usize = 11;

/// `rings::FxType` -- `fx % 3` selects the family (formant / modulation / reverb).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FxType {
    Formant = 0,
    Chorus = 1,
    Reverb = 2,
    Formant2 = 3,
    Ensemble = 4,
    Reverb2 = 5,
}

#[rustfmt::skip]
const REGISTRATIONS: [[f32; NUM_HARMONICS * 2]; 11] = [
    [1.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    [1.0, 1.0, 0.0, 0.0, 0.0, 0.0],
    [1.0, 0.0, 1.0, 0.0, 0.0, 0.0],
    [1.0, 0.1, 0.0, 0.0, 1.0, 0.0],
    [1.0, 0.5, 1.0, 0.0, 1.0, 0.0],
    [1.0, 1.0, 1.0, 1.0, 1.0, 1.0],
    [0.0, 1.0, 1.0, 1.0, 1.0, 0.0],
    [0.0, 0.5, 1.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 1.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 0.0, 0.0, 1.0],
];

const FORMANTS: [[f32; NUM_FORMANTS]; 5] = [
    [700.0, 1100.0, 2400.0],
    [500.0, 1300.0, 1700.0],
    [400.0, 2000.0, 2500.0],
    [600.0, 800.0, 2400.0],
    [300.0, 900.0, 2200.0],
];

// Chord table by Bryan Noll for the string synth. `[polyphony - 1][chord][note]`,
// trailing zeros where a row uses fewer than 8 notes.
#[rustfmt::skip]
const CHORDS: [[[f32; MAX_CHORD_SIZE]; NUM_CHORDS]; MAX_POLYPHONY] = [
    [
        [-12.0, -0.01,  0.0,  0.01,  0.02, 11.99, 12.0, 24.0],
        [-12.0, -5.01, -5.0,  0.0,   7.0,  12.0,  19.0, 24.0],
        [-12.0, -5.0,   0.0,  5.0,   7.0,  12.0,  17.0, 24.0],
        [-12.0, -5.0,   0.0,  0.01,  3.0,  12.0,  19.0, 24.0],
        [-12.0, -5.01, -5.0,  0.0,   3.0,  10.0,  19.0, 24.0],
        [-12.0, -5.0,   0.0,  3.0,  10.0,  14.0,  19.0, 24.0],
        [-12.0, -5.01, -5.0,  0.0,   3.0,  10.0,  17.0, 24.0],
        [-12.0, -5.0,   0.0,  2.0,   9.0,  16.0,  19.0, 24.0],
        [-12.0, -5.0,   0.0,  4.0,  11.0,  14.0,  19.0, 24.0],
        [-12.0, -5.0,   0.0,  4.0,   7.0,  11.0,  19.0, 24.0],
        [-12.0, -5.0,   0.0,  4.0,   7.0,  12.0,  19.0, 24.0],
    ],
    [
        [-12.0, -0.01,  0.0,  0.01, 12.0,  12.01, 0.0, 0.0],
        [-12.0, -5.01, -5.0,  0.0,   7.0,  12.0,  0.0, 0.0],
        [-12.0, -5.0,   0.0,  5.0,   7.0,  12.0,  0.0, 0.0],
        [-12.0, -5.0,   0.0,  0.01,  3.0,  12.0,  0.0, 0.0],
        [-12.0, -5.01, -5.0,  0.0,   3.0,  10.0,  0.0, 0.0],
        [-12.0, -5.0,   0.0,  3.0,  10.0,  14.0,  0.0, 0.0],
        [-12.0, -5.0,   0.0,  3.0,  10.0,  17.0,  0.0, 0.0],
        [-12.0, -5.0,   0.0,  2.0,   9.0,  16.0,  0.0, 0.0],
        [-12.0, -5.0,   0.0,  4.0,  11.0,  14.0,  0.0, 0.0],
        [-12.0, -5.0,   0.0,  4.0,   7.0,  11.0,  0.0, 0.0],
        [-12.0, -5.0,   0.0,  4.0,   7.0,  12.0,  0.0, 0.0],
    ],
    [
        [-12.0, 0.0,  0.01, 12.0, 0.0, 0.0, 0.0, 0.0],
        [-12.0, 6.99, 7.0,  12.0, 0.0, 0.0, 0.0, 0.0],
        [-12.0, 5.0,  7.0,  12.0, 0.0, 0.0, 0.0, 0.0],
        [-12.0, 3.0,  11.99, 12.0, 0.0, 0.0, 0.0, 0.0],
        [-12.0, 3.0,  9.99, 10.0, 0.0, 0.0, 0.0, 0.0],
        [-12.0, 3.0,  10.0, 14.0, 0.0, 0.0, 0.0, 0.0],
        [-12.0, 3.0,  10.0, 17.0, 0.0, 0.0, 0.0, 0.0],
        [-12.0, 2.0,  9.0,  16.0, 0.0, 0.0, 0.0, 0.0],
        [-12.0, 4.0,  11.0, 14.0, 0.0, 0.0, 0.0, 0.0],
        [-12.0, 4.0,  7.0,  11.0, 0.0, 0.0, 0.0, 0.0],
        [-12.0, 4.0,  7.0,  12.0, 0.0, 0.0, 0.0, 0.0],
    ],
    [
        [0.0, 0.01, 12.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        [0.0, 7.0,  12.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        [5.0, 7.0,  12.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        [0.0, 3.0,  12.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        [0.0, 3.0,  10.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        [3.0, 10.0, 14.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        [3.0, 10.0, 17.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        [2.0, 9.0,  16.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        [4.0, 11.0, 14.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        [4.0, 7.0,  11.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        [4.0, 7.0,  12.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    ],
];

#[derive(Debug, Clone)]
struct VoiceGroup {
    tonic: f32,
    envelope: StringSynthEnvelope,
    chord: i32,
}

/// `rings::StringSynthPart`.
pub struct StringSynthPart {
    voice: [StringSynthVoice<NUM_HARMONICS>; NUM_VOICES],
    group: [VoiceGroup; MAX_POLYPHONY],

    formant_filter: [Svf; NUM_FORMANTS],
    ensemble: Ensemble,
    reverb: Reverb,
    chorus: Chorus,
    limiter: Limiter,

    active_group: usize,
    polyphony: usize,
    acquisition_delay: i32,

    fx_type: FxType,
    note_filter: NoteFilter,

    filter_in_buffer: [f32; MAX_BLOCK_SIZE],
    filter_out_buffer: [f32; MAX_BLOCK_SIZE],

    clear_fx: bool,
}

impl Default for StringSynthPart {
    fn default() -> Self {
        Self::new()
    }
}

impl StringSynthPart {
    /// `StringSynthPart()` + `Init`.
    pub fn new() -> Self {
        let mut p = Self {
            voice: core::array::from_fn(|_| StringSynthVoice::new()),
            group: core::array::from_fn(|_| VoiceGroup {
                tonic: 0.0,
                envelope: StringSynthEnvelope::new(),
                chord: 0,
            }),
            formant_filter: [Svf::default(); NUM_FORMANTS],
            ensemble: Ensemble::new(),
            reverb: Reverb::new(),
            chorus: Chorus::new(),
            limiter: Limiter::new(),
            active_group: 0,
            polyphony: 1,
            acquisition_delay: 0,
            fx_type: FxType::Ensemble,
            note_filter: NoteFilter::new(),
            filter_in_buffer: [0.0; MAX_BLOCK_SIZE],
            filter_out_buffer: [0.0; MAX_BLOCK_SIZE],
            clear_fx: false,
        };
        p.init();
        p
    }

    /// `Init`.
    pub fn init(&mut self) {
        self.active_group = 0;
        self.acquisition_delay = 0;
        self.polyphony = 1;
        self.fx_type = FxType::Ensemble;

        for v in self.voice.iter_mut() {
            v.init();
        }
        for g in self.group.iter_mut() {
            g.tonic = 0.0;
            g.envelope.init();
        }
        for f in self.formant_filter.iter_mut() {
            f.init();
        }
        self.limiter.init();
        self.reverb.init();
        self.chorus.init();
        self.ensemble.init();

        self.note_filter.init(
            SAMPLE_RATE / MAX_BLOCK_SIZE as f32,
            0.001,
            0.005,
            0.050,
            0.004,
        );
    }

    /// `set_polyphony`.
    pub fn set_polyphony(&mut self, polyphony: i32) {
        let old = self.polyphony;
        self.polyphony = (polyphony.max(1) as usize).min(MAX_POLYPHONY);
        for i in old..self.polyphony {
            self.group[i].tonic = self.group[0].tonic + i as f32 * 0.01;
        }
        if self.active_group >= self.polyphony {
            self.active_group = 0;
        }
    }

    /// `set_fx`.
    pub fn set_fx(&mut self, fx_type: FxType) {
        if (fx_type as u32 % 3) != (self.fx_type as u32 % 3) {
            self.clear_fx = true;
        }
        self.fx_type = fx_type;
    }

    fn process_envelopes(
        &mut self,
        shape: f32,
        flags: &[u8; MAX_POLYPHONY],
        values: &mut [f32; MAX_POLYPHONY],
    ) {
        let decay = shape;
        let attack = if shape < 0.5 {
            0.0
        } else {
            (shape - 0.5) * 2.0
        };

        let period = SAMPLE_RATE / MAX_BLOCK_SIZE as f32;
        let attack_time = semitones_to_ratio(attack * 96.0) * 0.005 * period;
        let decay_time = semitones_to_ratio(decay * 84.0) * 0.180 * period;
        let attack_rate = 1.0 / attack_time;
        let decay_rate = 1.0 / decay_time;

        for i in 0..self.polyphony {
            let mut drone = if shape < 0.98 {
                0.0
            } else {
                (shape - 0.98) * 55.0
            };
            if drone >= 1.0 {
                drone = 1.0;
            }
            self.group[i].envelope.set_ad(attack_rate, decay_rate);
            let value = self.group[i].envelope.process(flags[i]);
            values[i] = value + (1.0 - value) * drone;
        }
    }

    fn compute_registration(
        gain: f32,
        mut registration: f32,
        amplitudes: &mut [f32; NUM_HARMONICS * 2],
    ) {
        registration *= (11 - 1) as f32 - 0.001;
        let registration_integral = registration as usize;
        let registration_fractional = registration - registration_integral as f32;
        let mut total = 0.0f32;
        for i in 0..NUM_HARMONICS * 2 {
            let a = REGISTRATIONS[registration_integral][i];
            let b = REGISTRATIONS[registration_integral + 1][i];
            amplitudes[i] = a + (b - a) * registration_fractional;
            total += amplitudes[i];
        }
        for a in amplitudes.iter_mut() {
            *a = gain * *a / total;
        }
    }

    fn process_formant_filter(
        &mut self,
        mut vowel: f32,
        shift: f32,
        resonance: f32,
        out: &mut [f32],
        aux: &mut [f32],
        size: usize,
    ) {
        for i in 0..size {
            self.filter_in_buffer[i] = out[i] + aux[i];
        }
        out[..size].fill(0.0);
        aux[..size].fill(0.0);

        vowel *= (5 - 1) as f32 - 0.001;
        let vowel_integral = vowel as usize;
        let vowel_fractional = vowel - vowel_integral as f32;

        for i in 0..NUM_FORMANTS {
            let a = FORMANTS[vowel_integral][i];
            let b = FORMANTS[vowel_integral + 1][i];
            let f = (a + (b - a) * vowel_fractional) * shift;
            self.formant_filter[i].set_f_q(
                f / SAMPLE_RATE,
                resonance,
                FrequencyApproximation::Dirty,
            );
            self.formant_filter[i].process_block(
                FilterMode::BandPass,
                &self.filter_in_buffer[..size],
                &mut self.filter_out_buffer[..size],
            );
            let pan = i as f32 * 0.3 + 0.2;
            for j in 0..size {
                out[j] += self.filter_out_buffer[j] * pan * 0.5;
                aux[j] += self.filter_out_buffer[j] * (1.0 - pan) * 0.5;
            }
        }
    }

    /// `Process`.
    pub fn process(
        &mut self,
        performance_state: &crate::part::PerformanceState,
        patch: &crate::part::Patch,
        input: &[f32],
        out: &mut [f32],
        aux: &mut [f32],
        size: usize,
    ) {
        let mut envelope_flags = [0u8; MAX_POLYPHONY];

        self.note_filter
            .process(performance_state.note, performance_state.strum);
        if performance_state.strum {
            self.group[self.active_group].tonic = self.note_filter.stable_note();
            envelope_flags[self.active_group] = FLAG_FALLING_EDGE;
            self.active_group = (self.active_group + 1) % self.polyphony;
            envelope_flags[self.active_group] = FLAG_RISING_EDGE;
            self.acquisition_delay = 3;
        }
        if self.acquisition_delay != 0 {
            self.acquisition_delay -= 1;
        } else {
            self.group[self.active_group].tonic = self.note_filter.note();
            self.group[self.active_group].chord = performance_state.chord;
            envelope_flags[self.active_group] |= FLAG_GATE;
        }

        let mut envelope_values = [0.0f32; MAX_POLYPHONY];
        self.process_envelopes(patch.damping, &envelope_flags, &mut envelope_values);

        aux[..size].copy_from_slice(&input[..size]);
        out[..size].copy_from_slice(&input[..size]);
        let chord_size = (NUM_VOICES / self.polyphony).min(MAX_CHORD_SIZE);

        for group in 0..self.polyphony {
            let mut harmonics = [0.0f32; NUM_HARMONICS * 2];
            Self::compute_registration(
                envelope_values[group] * 0.25,
                patch.brightness,
                &mut harmonics,
            );

            let mut note_values = [0.0f32; MAX_CHORD_SIZE];
            let mut note_amplitudes = [0.0f32; MAX_CHORD_SIZE];
            let chord = (self.group[group].chord.max(0) as usize).min(NUM_CHORDS - 1);
            for i in 0..chord_size {
                let n = CHORDS[self.polyphony - 1][chord][i];
                note_values[i] = n;
                note_amplitudes[i] = if (0.0..=17.0).contains(&n) { 1.0 } else { 0.7 };
            }

            for chord_note in 0..chord_size {
                let note = self.group[group].tonic
                    + performance_state.tonic
                    + performance_state.fm
                    + note_values[chord_note];

                let mut amplitudes = [0.0f32; NUM_HARMONICS * 2];
                for i in 0..NUM_HARMONICS * 2 {
                    amplitudes[i] = note_amplitudes[chord_note] * harmonics[i];
                }

                let num_harmonics = if self.polyphony >= 2 && chord_note < 2 {
                    NUM_HARMONICS - 1
                } else {
                    NUM_HARMONICS
                };
                for i in num_harmonics..NUM_HARMONICS {
                    amplitudes[2 * (num_harmonics - 1)] += amplitudes[2 * i];
                    amplitudes[2 * (num_harmonics - 1) + 1] += amplitudes[2 * i + 1];
                }

                let frequency = semitones_to_ratio(note - 69.0) * A3;
                let dest: &mut [f32] = if (group + chord_note) & 1 == 1 {
                    out
                } else {
                    aux
                };
                self.voice[group * chord_size + chord_note].render(
                    frequency,
                    &amplitudes,
                    num_harmonics,
                    dest,
                    size,
                );
            }
        }

        if self.clear_fx {
            self.reverb.clear();
            self.clear_fx = false;
        }

        let position = patch.position;
        match self.fx_type {
            FxType::Formant | FxType::Formant2 => {
                let (shift, resonance) = if self.fx_type == FxType::Formant {
                    (1.0, 25.0)
                } else {
                    (1.1, 10.0)
                };
                self.process_formant_filter(position, shift, resonance, out, aux, size);
            }
            FxType::Chorus => {
                self.chorus.set_amount(position);
                self.chorus.set_depth(0.15 + 0.5 * position);
                self.chorus
                    .process(&mut out[..size], &mut aux[..size], size);
            }
            FxType::Ensemble => {
                self.ensemble.set_amount(position * (2.0 - position));
                self.ensemble.set_depth(0.2 + 0.8 * position * position);
                self.ensemble
                    .process(&mut out[..size], &mut aux[..size], size);
            }
            FxType::Reverb | FxType::Reverb2 => {
                self.reverb.set_amount(position * 0.5);
                self.reverb.set_diffusion(0.625);
                self.reverb.set_time(if self.fx_type == FxType::Reverb {
                    0.5 + 0.49 * position
                } else {
                    0.3 + 0.6 * position
                });
                self.reverb.set_input_gain(0.2);
                self.reverb.set_lp(if self.fx_type == FxType::Reverb {
                    0.3
                } else {
                    0.6
                });
                self.reverb
                    .process(&mut out[..size], &mut aux[..size], size);
            }
        }

        for a in aux[..size].iter_mut() {
            *a = -*a;
        }
        self.limiter
            .process(&mut out[..size], &mut aux[..size], size, 1.0);
    }
}
