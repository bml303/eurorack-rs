//! `rings/dsp/part.{h,cc}` + `patch.h` + `performance_state.h` -- the resonator
//! voice group: six models, 1-4 voice polyphony, internal exciter and the
//! odd/even (or per-voice) output routing, then the string+reverb blend and the
//! output limiter.

use stmlib::fdsp::sqrt;
use stmlib::filter::{DcBlocker, FilterMode, FrequencyApproximation, Svf};
use stmlib::units::semitones_to_ratio;
use stmlib::{CosineOscillator, CosineOscillatorMode};

use crate::dsp::{A3, MAX_BLOCK_SIZE, SAMPLE_RATE};
use crate::fm_voice::FmVoice;
use crate::fx::Reverb;
use crate::limiter::Limiter;
use crate::note_filter::NoteFilter;
use crate::plucker::Plucker;
use crate::resonator::Resonator;
use crate::string::String;

/// `kNumChords`.
pub const NUM_CHORDS: i32 = 11;

const MAX_POLYPHONY: usize = 4;
const NUM_STRINGS: usize = MAX_POLYPHONY * 2;

/// `rings::PerformanceState` -- per-block note-triggering state.
#[derive(Debug, Clone, Copy, Default)]
pub struct PerformanceState {
    pub strum: bool,
    pub internal_exciter: bool,
    pub internal_strum: bool,
    pub internal_note: bool,
    pub tonic: f32,
    pub note: f32,
    pub fm: f32,
    pub chord: i32,
}

/// `rings::Patch` -- the four synthesis parameters, all `[0, 1]`.
#[derive(Debug, Clone, Copy, Default)]
pub struct Patch {
    pub structure: f32,
    pub brightness: f32,
    pub damping: f32,
    pub position: f32,
}

/// `rings::ResonatorModel`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResonatorModel {
    Modal = 0,
    SympatheticString = 1,
    String = 2,
    FmVoice = 3,
    SympatheticStringQuantized = 4,
    StringAndReverb = 5,
}

const MODEL_GAINS: [f32; 6] = [1.4, 1.0, 1.4, 0.7, 1.0, 1.4];

const PING_PATTERN: [usize; 8] = [1, 0, 2, 1, 0, 2, 1, 0];

// Chord table by Bryan Noll (`BRYAN_CHORDS`). `[polyphony - 1][chord][note]`,
// trailing zeros where a row uses fewer than 8 notes.
#[rustfmt::skip]
const CHORDS: [[[f32; 8]; 11]; 4] = [
    [
        [-12.0, -0.01, 0.0, 0.01, 0.02, 11.98, 11.99, 12.0],
        [-12.0, -5.0,  0.0, 6.99, 7.0,  11.99, 12.0,  19.0],
        [-12.0, -5.0,  0.0, 5.0,  7.0,  11.99, 12.0,  17.0],
        [-12.0, -5.0,  0.0, 3.0,  7.0,  3.01,  12.0,  19.0],
        [-12.0, -5.0,  0.0, 3.0,  7.0,  3.01,  10.0,  19.0],
        [-12.0, -5.0,  0.0, 3.0,  14.0, 3.01,  10.0,  19.0],
        [-12.0, -5.0,  0.0, 3.0,  7.0,  3.01,  10.0,  17.0],
        [-12.0, -5.0,  0.0, 2.0,  7.0,  9.0,   16.0,  19.0],
        [-12.0, -5.0,  0.0, 4.0,  7.0,  11.0,  14.0,  19.0],
        [-12.0, -5.0,  0.0, 4.0,  7.0,  11.0,  10.99, 19.0],
        [-12.0, -5.0,  0.0, 4.0,  7.0,  11.99, 12.0,  19.0],
    ],
    [
        [-12.0, 0.0,  0.01, 12.0,  0.0, 0.0, 0.0, 0.0],
        [-12.0, 6.99, 7.0,  12.0,  0.0, 0.0, 0.0, 0.0],
        [-12.0, 5.0,  7.0,  12.0,  0.0, 0.0, 0.0, 0.0],
        [-12.0, 3.0,  11.99, 12.0, 0.0, 0.0, 0.0, 0.0],
        [-12.0, 3.0,  10.0, 12.0,  0.0, 0.0, 0.0, 0.0],
        [-12.0, 3.0,  10.0, 14.0,  0.0, 0.0, 0.0, 0.0],
        [-12.0, 3.0,  10.0, 17.0,  0.0, 0.0, 0.0, 0.0],
        [-12.0, 2.0,  9.0,  16.0,  0.0, 0.0, 0.0, 0.0],
        [-12.0, 4.0,  11.0, 14.0,  0.0, 0.0, 0.0, 0.0],
        [-12.0, 4.0,  7.0,  11.0,  0.0, 0.0, 0.0, 0.0],
        [-12.0, 4.0,  7.0,  12.0,  0.0, 0.0, 0.0, 0.0],
    ],
    [
        [0.0, -12.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        [0.0, 2.0,   0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        [0.0, 3.0,   0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        [0.0, 4.0,   0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        [0.0, 5.0,   0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        [0.0, 7.0,   0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        [0.0, 9.0,   0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        [0.0, 10.0,  0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        [0.0, 11.0,  0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        [0.0, 12.0,  0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        [-12.0, 12.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    ],
    [
        [0.0, -12.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        [0.0, 2.0,   0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        [0.0, 3.0,   0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        [0.0, 4.0,   0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        [0.0, 5.0,   0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        [0.0, 7.0,   0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        [0.0, 9.0,   0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        [0.0, 10.0,  0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        [0.0, 11.0,  0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        [0.0, 12.0,  0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        [-12.0, 12.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    ],
];

const LFO_FREQUENCIES: [f32; NUM_STRINGS] = [0.5, 0.4, 0.35, 0.23, 0.211, 0.2, 0.171, 0.0];

/// `Squash(x)` -- a soft gate curve.
#[inline]
fn squash(mut x: f32) -> f32 {
    if x < 0.5 {
        x *= 2.0;
        x *= x;
        x *= x;
        x *= x;
        x *= x;
        x *= 0.5;
    } else {
        x = 2.0 - 2.0 * x;
        x *= x;
        x *= x;
        x *= x;
        x *= x;
        x = 1.0 - 0.5 * x;
    }
    x
}

/// `rings::Part`.
pub struct Part {
    bypass: bool,
    dirty: bool,
    model: ResonatorModel,

    active_voice: usize,
    step_counter: u32,
    polyphony: usize,

    resonator: [Resonator; MAX_POLYPHONY],
    string: [String; NUM_STRINGS],
    lfo: [CosineOscillator; NUM_STRINGS],
    fm_voice: [FmVoice; MAX_POLYPHONY],

    excitation_filter: [Svf; MAX_POLYPHONY],
    dc_blocker: [DcBlocker; MAX_POLYPHONY],
    plucker: [Plucker; MAX_POLYPHONY],

    note: [f32; MAX_POLYPHONY],
    note_filter: NoteFilter,

    resonator_input: [f32; MAX_BLOCK_SIZE],
    sympathetic_resonator_input: [f32; MAX_BLOCK_SIZE],
    noise_burst_buffer: [f32; MAX_BLOCK_SIZE],
    out_buffer: [f32; MAX_BLOCK_SIZE],
    aux_buffer: [f32; MAX_BLOCK_SIZE],

    reverb: Reverb,
    limiter: Limiter,
}

impl Default for Part {
    fn default() -> Self {
        Self::new()
    }
}

impl Part {
    /// `Part()` + `Init`.
    pub fn new() -> Self {
        let mut p = Self {
            bypass: false,
            dirty: true,
            model: ResonatorModel::Modal,
            active_voice: 0,
            step_counter: 0,
            polyphony: 1,
            resonator: core::array::from_fn(|_| Resonator::new()),
            string: core::array::from_fn(|_| String::new()),
            lfo: [CosineOscillator::new(CosineOscillatorMode::Approximate, 0.0); NUM_STRINGS],
            fm_voice: core::array::from_fn(|_| FmVoice::new()),
            excitation_filter: [Svf::default(); MAX_POLYPHONY],
            dc_blocker: [DcBlocker::default(); MAX_POLYPHONY],
            plucker: core::array::from_fn(|_| Plucker::new()),
            note: [0.0; MAX_POLYPHONY],
            note_filter: NoteFilter::new(),
            resonator_input: [0.0; MAX_BLOCK_SIZE],
            sympathetic_resonator_input: [0.0; MAX_BLOCK_SIZE],
            noise_burst_buffer: [0.0; MAX_BLOCK_SIZE],
            out_buffer: [0.0; MAX_BLOCK_SIZE],
            aux_buffer: [0.0; MAX_BLOCK_SIZE],
            reverb: Reverb::new(),
            limiter: Limiter::new(),
        };
        p.init();
        p
    }

    /// `Init`.
    pub fn init(&mut self) {
        self.active_voice = 0;
        self.note = [0.0; MAX_POLYPHONY];
        self.bypass = false;
        self.polyphony = 1;
        self.model = ResonatorModel::Modal;
        self.dirty = true;
        self.step_counter = 0;

        for i in 0..MAX_POLYPHONY {
            self.excitation_filter[i].init();
            self.plucker[i].init();
            self.dc_blocker[i].init(1.0 - 10.0 / SAMPLE_RATE);
        }

        self.reverb.init();
        self.limiter.init();

        self.note_filter.init(
            SAMPLE_RATE / MAX_BLOCK_SIZE as f32,
            0.001,
            0.010,
            0.050,
            0.004,
        );
    }

    #[inline]
    pub fn bypass(&self) -> bool {
        self.bypass
    }
    #[inline]
    pub fn set_bypass(&mut self, bypass: bool) {
        self.bypass = bypass;
    }
    #[inline]
    pub fn polyphony(&self) -> i32 {
        self.polyphony as i32
    }

    /// `set_polyphony`.
    pub fn set_polyphony(&mut self, polyphony: i32) {
        let old = self.polyphony;
        self.polyphony = (polyphony.max(1) as usize).min(MAX_POLYPHONY);
        for i in old..self.polyphony {
            self.note[i] = self.note[0] + i as f32 * 0.05;
        }
        self.dirty = true;
    }

    #[inline]
    pub fn model(&self) -> ResonatorModel {
        self.model
    }

    /// `set_model`.
    pub fn set_model(&mut self, model: ResonatorModel) {
        if model != self.model {
            self.model = model;
            self.dirty = true;
        }
    }

    fn configure_resonators(&mut self) {
        if !self.dirty {
            return;
        }
        match self.model {
            ResonatorModel::Modal => {
                let resolution = 64 / self.polyphony as i32 - 4;
                for i in 0..self.polyphony {
                    self.resonator[i].init();
                    self.resonator[i].set_resolution(resolution);
                }
            }
            ResonatorModel::SympatheticString
            | ResonatorModel::String
            | ResonatorModel::SympatheticStringQuantized
            | ResonatorModel::StringAndReverb => {
                let has_dispersion = self.model == ResonatorModel::String
                    || self.model == ResonatorModel::StringAndReverb;
                for i in 0..NUM_STRINGS {
                    self.string[i].init(has_dispersion);
                    let f_lfo = MAX_BLOCK_SIZE as f32 / SAMPLE_RATE * LFO_FREQUENCIES[i];
                    self.lfo[i].init(CosineOscillatorMode::Approximate, f_lfo);
                }
                for i in 0..self.polyphony {
                    self.plucker[i].init();
                }
            }
            ResonatorModel::FmVoice => {
                for i in 0..self.polyphony {
                    self.fm_voice[i].init();
                }
            }
        }
        if self.active_voice >= self.polyphony {
            self.active_voice = 0;
        }
        self.dirty = false;
    }

    fn compute_sympathetic_strings_notes(
        &self,
        tonic: f32,
        note: f32,
        mut parameter: f32,
        destination: &mut [f32],
        num_strings: usize,
    ) {
        let notes = [
            tonic,
            note - 12.0,
            note - 7.01955,
            note,
            note + 7.01955,
            note + 12.0,
            note + 19.01955,
            note + 24.0,
            note + 24.0,
        ];
        let detunings = [0.013f32, 0.011, 0.007, 0.017];

        if parameter >= 2.0 {
            let chord_index = (parameter - 2.0) as usize;
            let chord = &CHORDS[self.polyphony - 1][chord_index];
            for i in 0..num_strings {
                destination[i] = chord[i] + note;
            }
            return;
        }

        let num_detuned_strings = (num_strings - 1) >> 1;
        let first_detuned_string = num_strings - num_detuned_strings;

        for i in 0..first_detuned_string {
            let mut n = 3.0f32;
            if i != 0 {
                n = parameter * 7.0;
                parameter += (1.0 - parameter) * 0.2;
            }
            let n_integral = n as usize;
            let n_fractional = squash(n - n_integral as f32);

            let a = notes[n_integral];
            let b = notes[n_integral + 1];
            let value = a + (b - a) * n_fractional;
            destination[i] = value;
            if i + first_detuned_string < num_strings {
                destination[i + first_detuned_string] = value + detunings[i & 3];
            }
        }
    }

    fn render_modal_voice(
        &mut self,
        voice: usize,
        strum: bool,
        patch: &Patch,
        frequency: f32,
        filter_cutoff: f32,
        size: usize,
    ) {
        if voice == self.active_voice && strum {
            // Internal exciter is a pre-filter pulse. (The caller only reaches
            // here for `internal_exciter`.)
            self.resonator_input[0] +=
                0.25 * semitones_to_ratio(filter_cutoff * filter_cutoff * 24.0) / filter_cutoff;
        }

        self.excitation_filter[voice]
            .process_in_place(FilterMode::LowPass, &mut self.resonator_input[..size]);

        let r = &mut self.resonator[voice];
        r.set_frequency(frequency);
        r.set_structure(patch.structure);
        r.set_brightness(patch.brightness * patch.brightness);
        r.set_position(patch.position);
        r.set_damping(patch.damping);
        r.process(
            &self.resonator_input[..size],
            &mut self.out_buffer[..size],
            &mut self.aux_buffer[..size],
            size,
        );
    }

    fn render_fm_voice(
        &mut self,
        voice: usize,
        strum: bool,
        patch: &Patch,
        frequency: f32,
        size: usize,
    ) {
        if voice == self.active_voice && strum {
            self.fm_voice[voice].trigger_internal_envelope();
        }
        let v = &mut self.fm_voice[voice];
        v.set_frequency(frequency);
        v.set_ratio(patch.structure);
        v.set_brightness(patch.brightness);
        v.set_feedback_amount(patch.position);
        v.set_position(0.0);
        v.set_damping(patch.damping);
        v.process(
            &self.resonator_input[..size],
            &mut self.out_buffer[..size],
            &mut self.aux_buffer[..size],
            size,
        );
    }

    fn render_string_voice(
        &mut self,
        voice: usize,
        performance_state: &PerformanceState,
        patch: &Patch,
        frequency: f32,
        filter_cutoff: f32,
        size: usize,
    ) {
        let mut num_strings = 1usize;
        let mut frequencies = [0.0f32; NUM_STRINGS];

        let sympathetic = self.model == ResonatorModel::SympatheticString
            || self.model == ResonatorModel::SympatheticStringQuantized;
        if sympathetic {
            num_strings = 2 * MAX_POLYPHONY / self.polyphony;
            let parameter = if self.model == ResonatorModel::SympatheticString {
                patch.structure
            } else {
                2.0 + performance_state.chord as f32
            };
            self.compute_sympathetic_strings_notes(
                performance_state.tonic + performance_state.fm,
                performance_state.tonic + self.note[voice] + performance_state.fm,
                parameter,
                &mut frequencies,
                num_strings,
            );
            for f in frequencies.iter_mut().take(num_strings) {
                *f = semitones_to_ratio(*f - 69.0) * A3;
            }
        } else {
            frequencies[0] = frequency;
        }

        if voice == self.active_voice {
            let gain = 1.0 / sqrt(num_strings as f32 * 2.0);
            for s in self.resonator_input[..size].iter_mut() {
                *s *= gain;
            }
        }

        self.excitation_filter[voice]
            .process_in_place(FilterMode::LowPass, &mut self.resonator_input[..size]);

        if performance_state.internal_exciter {
            if voice == self.active_voice && performance_state.strum {
                self.plucker[voice].trigger(frequency, filter_cutoff * 8.0, patch.position);
            }
            self.plucker[voice].process(&mut self.noise_burst_buffer[..size], size);
            for i in 0..size {
                self.resonator_input[i] += self.noise_burst_buffer[i];
            }
        }
        self.dc_blocker[voice].process(&mut self.resonator_input[..size]);

        self.out_buffer[..size].fill(0.0);
        self.aux_buffer[..size].fill(0.0);

        let structure = patch.structure;
        let dispersion = if structure < 0.24 {
            (structure - 0.24) * 4.166
        } else if structure > 0.26 {
            (structure - 0.26) * 1.35135
        } else {
            0.0
        };

        for string in 0..num_strings {
            let i = voice + string * self.polyphony;
            let lfo_value = self.lfo[i].next();

            let mut brightness = patch.brightness;
            let mut damping = patch.damping;
            let mut position = patch.position;
            let mut glide = 1.0f32;
            let string_index = string as f32 / num_strings as f32;
            let mut use_sympathetic_input = false;

            if self.model == ResonatorModel::StringAndReverb {
                damping *= 2.0 - damping;
            }

            if string > 0 && performance_state.internal_exciter {
                brightness *= 2.0 - brightness;
                brightness *= 2.0 - brightness;
                damping = 0.7 + patch.damping * 0.27;
                let amount = (0.5 - (0.5 - patch.position).abs()) * 0.9;
                position = patch.position + lfo_value * amount;
                glide = semitones_to_ratio((brightness - 1.0) * 36.0);
                use_sympathetic_input = true;
            }

            {
                let s = &mut self.string[i];
                s.set_dispersion(dispersion);
                s.set_frequency_glide(frequencies[string], glide);
                s.set_brightness(brightness);
                s.set_position(position);
                s.set_damping(damping + string_index * (0.95 - damping));
            }

            if use_sympathetic_input {
                let input = self.sympathetic_resonator_input;
                self.string[i].process(
                    &input[..size],
                    &mut self.out_buffer[..size],
                    &mut self.aux_buffer[..size],
                    size,
                );
            } else {
                let input = self.resonator_input;
                self.string[i].process(
                    &input[..size],
                    &mut self.out_buffer[..size],
                    &mut self.aux_buffer[..size],
                    size,
                );
            }

            if string == 0 {
                let gain = 0.2 / num_strings as f32;
                for i in 0..size {
                    let sum = self.out_buffer[i] - self.aux_buffer[i];
                    self.sympathetic_resonator_input[i] = gain * sum;
                }
            }
        }
    }

    /// `Process`.
    pub fn process(
        &mut self,
        performance_state: &PerformanceState,
        patch: &Patch,
        input: &[f32],
        out: &mut [f32],
        aux: &mut [f32],
        size: usize,
    ) {
        if self.bypass {
            out[..size].copy_from_slice(&input[..size]);
            aux[..size].copy_from_slice(&input[..size]);
            return;
        }

        self.configure_resonators();

        self.note_filter
            .process(performance_state.note, performance_state.strum);

        if performance_state.strum {
            self.note[self.active_voice] = self.note_filter.stable_note();
            if self.polyphony > 1 && self.polyphony & 1 == 1 {
                self.active_voice = PING_PATTERN[(self.step_counter % 8) as usize];
                self.step_counter = (self.step_counter + 1) % 8;
            } else {
                self.active_voice = (self.active_voice + 1) % self.polyphony;
            }
        }

        self.note[self.active_voice] = self.note_filter.note();

        out[..size].fill(0.0);
        aux[..size].fill(0.0);

        for voice in 0..self.polyphony {
            let cutoff = patch.brightness * (2.0 - patch.brightness);
            let note = self.note[voice] + performance_state.tonic + performance_state.fm;
            let frequency = semitones_to_ratio(note - 69.0) * A3;
            let filter_cutoff_range = if performance_state.internal_exciter {
                frequency * semitones_to_ratio((cutoff - 0.5) * 96.0)
            } else {
                0.4 * semitones_to_ratio((cutoff - 1.0) * 108.0)
            };
            let filter_cutoff = if voice == self.active_voice {
                filter_cutoff_range
            } else {
                10.0 / SAMPLE_RATE
            }
            .min(0.499);
            let filter_q = if performance_state.internal_exciter {
                1.5
            } else {
                0.8
            };

            self.excitation_filter[voice].set_f_q(
                filter_cutoff,
                filter_q,
                FrequencyApproximation::Dirty,
            );
            if voice == self.active_voice {
                self.resonator_input[..size].copy_from_slice(&input[..size]);
            } else {
                self.resonator_input[..size].fill(0.0);
            }

            match self.model {
                ResonatorModel::Modal => self.render_modal_voice(
                    voice,
                    performance_state.strum && performance_state.internal_exciter,
                    patch,
                    frequency,
                    filter_cutoff,
                    size,
                ),
                ResonatorModel::FmVoice => self.render_fm_voice(
                    voice,
                    performance_state.strum && performance_state.internal_exciter,
                    patch,
                    frequency,
                    size,
                ),
                _ => self.render_string_voice(
                    voice,
                    performance_state,
                    patch,
                    frequency,
                    filter_cutoff,
                    size,
                ),
            }

            if self.polyphony == 1 {
                for i in 0..size {
                    out[i] += self.out_buffer[i];
                    aux[i] += self.aux_buffer[i];
                }
            } else {
                let destination: &mut [f32] = if voice & 1 == 1 { aux } else { out };
                for i in 0..size {
                    destination[i] += self.out_buffer[i] - self.aux_buffer[i];
                }
            }
        }

        if self.model == ResonatorModel::StringAndReverb {
            for i in 0..size {
                let l = out[i];
                let r = aux[i];
                out[i] = l * patch.position + (1.0 - patch.position) * r;
                aux[i] = r * patch.position + (1.0 - patch.position) * l;
            }
            self.reverb.set_amount(0.1 + patch.damping * 0.5);
            self.reverb.set_diffusion(0.625);
            self.reverb.set_time(0.35 + 0.63 * patch.damping);
            self.reverb.set_input_gain(0.2);
            self.reverb.set_lp(0.3 + patch.brightness * 0.6);
            self.reverb
                .process(&mut out[..size], &mut aux[..size], size);
            for a in aux[..size].iter_mut() {
                *a = -*a;
            }
        }

        self.limiter.process(
            &mut out[..size],
            &mut aux[..size],
            size,
            MODEL_GAINS[self.model as usize],
        );
    }
}
