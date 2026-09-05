//! `plaits/dsp/engine2/chiptune_engine.h` -- NES/Game Boy-style square/
//! triangle waveforms, either arpeggiated over the current chord when
//! clocked (a trigger patched) or rendered as a plain chord otherwise.

use stmlib::fdsp::one_pole;
use stmlib::units::semitones_to_ratio;
use stmlib::HysteresisQuantizer2;

use super::arpeggiator::{Arpeggiator, ArpeggiatorMode};
use crate::chords::{ChordBank, NUM_NOTES, NUM_VOICES};
use crate::dsp::SAMPLE_RATE;
use crate::engine::{
    note_to_frequency, trigger_state, Engine, EngineParameters, PostProcessingSettings,
};
use crate::oscillator::{NesTriangleOscillator, SuperSquareOscillator};

/// Sentinel `envelope_shape` value meaning "no envelope" (`NO_ENVELOPE` in
/// the C, `enum { NO_ENVELOPE = 2 }`).
pub const NO_ENVELOPE: f32 = 2.0;

#[derive(Debug)]
pub struct ChiptuneEngine {
    voice: [SuperSquareOscillator; NUM_VOICES],
    bass: NesTriangleOscillator,

    chords: ChordBank,
    arpeggiator: Arpeggiator,
    arpeggiator_pattern_selector: HysteresisQuantizer2,

    envelope_shape: f32,
    envelope_state: f32,
    aux_envelope_amount: f32,
}

impl Default for ChiptuneEngine {
    fn default() -> Self {
        Self {
            voice: [SuperSquareOscillator::default(); NUM_VOICES],
            bass: NesTriangleOscillator::default(),
            chords: ChordBank::default(),
            arpeggiator: Arpeggiator::default(),
            arpeggiator_pattern_selector: HysteresisQuantizer2::default(),
            envelope_shape: NO_ENVELOPE,
            envelope_state: 0.0,
            aux_envelope_amount: 0.0,
        }
    }
}

impl ChiptuneEngine {
    #[inline]
    pub fn set_envelope_shape(&mut self, envelope_shape: f32) {
        self.envelope_shape = envelope_shape;
    }
}

impl Engine for ChiptuneEngine {
    fn init(&mut self) {
        self.bass.init();
        // Note: the C only inits `kChordNumNotes` (4) of the 5
        // `voice_` elements too -- `voice_[4]` keeps its
        // zero-initialized state until first used (harmless: `slave_frequency`
        // differs from `init()`'s 0.01 only for one interpolation ramp).
        for voice in self.voice.iter_mut().take(NUM_NOTES) {
            voice.init();
        }
        self.chords.init();
        self.arpeggiator.init();
        self.arpeggiator_pattern_selector.init(12, 0.075, false);
        self.envelope_shape = NO_ENVELOPE;
        self.envelope_state = 0.0;
        self.aux_envelope_amount = 0.0;
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
        _already_enveloped: bool,
    ) -> bool {
        let size = out.len();
        let f0 = note_to_frequency(parameters.note);
        let shape = parameters.morph * 0.995;
        let clocked = parameters.trigger & trigger_state::UNPATCHED == 0;
        let mut root_transposition = 1.0f32;

        let already_enveloped = clocked;

        if clocked {
            if parameters.trigger & trigger_state::RISING_EDGE != 0 {
                self.chords.set_chord(parameters.harmonics);
                self.chords.sort();

                let pattern = self.arpeggiator_pattern_selector.process(parameters.timbre);
                self.arpeggiator
                    .set_mode(ArpeggiatorMode::from_index(pattern / 3));
                self.arpeggiator.set_range(1 << (pattern % 3));
                self.arpeggiator.clock(self.chords.num_notes());
                self.envelope_state = 1.0;
            }
            let octave = (1i32 << self.arpeggiator.octave()) as f32;
            let note_f0 = f0 * self.chords.sorted_ratio(self.arpeggiator.note() as usize) * octave;
            root_transposition = octave;
            self.voice[0].render(note_f0, shape, out);
        } else {
            let mut ratios = [0.0f32; NUM_VOICES];
            let mut amplitudes = [0.0f32; NUM_VOICES];

            self.chords.set_chord(parameters.harmonics);
            self.chords
                .compute_chord_inversion(parameters.timbre, &mut ratios, &mut amplitudes);
            for j in (1..NUM_VOICES).step_by(2) {
                amplitudes[j] = -amplitudes[j];
            }

            out.fill(0.0);
            for voice in 0..NUM_VOICES {
                let voice_f0 = f0 * ratios[voice];
                self.voice[voice].render(voice_f0, shape, aux);
                for j in 0..size {
                    out[j] += aux[j] * amplitudes[voice];
                }
            }
        }

        // Render bass note.
        self.bass.render(f0 * 0.5 * root_transposition, aux);

        // Apply envelope if necessary.
        if self.envelope_shape != NO_ENVELOPE {
            let shape = self.envelope_shape.abs();
            let decay = 1.0 - 2.0 / SAMPLE_RATE * semitones_to_ratio(60.0 * shape) * shape;
            let aux_envelope_amount = (self.envelope_shape * 20.0).clamp(0.0, 1.0);

            for i in 0..size {
                one_pole(&mut self.aux_envelope_amount, aux_envelope_amount, 0.01);
                self.envelope_state *= decay;
                out[i] *= self.envelope_state;
                aux[i] *= 1.0 + self.aux_envelope_amount * (self.envelope_state - 1.0);
            }
        }

        already_enveloped
    }

    fn post_processing_settings(&self) -> PostProcessingSettings {
        PostProcessingSettings {
            out_gain: 0.5,
            aux_gain: 0.5,
            already_enveloped: false,
        }
    }
}
