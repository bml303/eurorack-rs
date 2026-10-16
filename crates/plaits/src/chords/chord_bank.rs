//! `plaits/dsp/chords/chord_bank.h` -- the 11-chord table shared by
//! `ChordEngine`, `StringMachineEngine` and `ChiptuneEngine`, plus inversion
//! voicing.
//!
//! # Deviations from the C
//!
//! - A host can replace the 11 chords with a table of its own (up to
//!   [`MAX_CHORDS`], [`ChordBank::set_user_chords`]), as Plaits' community
//!   firmwares do. Without one the bank is exactly the C's.

use stmlib::units::semitones_to_ratio;
use stmlib::HysteresisQuantizer2;

pub const NUM_NOTES: usize = 4;
pub const NUM_VOICES: usize = NUM_NOTES + 1;
pub const NUM_CHORDS: usize = 11;
/// The most chords a user table holds
pub const MAX_CHORDS: usize = 16;

/// A chord: its notes in semitones above the root
pub type Chord = [f32; NUM_NOTES];

#[rustfmt::skip]
const CHORDS: [[f32; NUM_NOTES]; NUM_CHORDS] = [
    [0.00, 0.01, 11.99, 12.00], // OCT
    [0.00, 7.00,  7.01, 12.00], // 5
    [0.00, 5.00,  7.00, 12.00], // sus4
    [0.00, 3.00,  7.00, 12.00], // m
    [0.00, 3.00,  7.00, 10.00], // m7
    [0.00, 3.00, 10.00, 14.00], // m9
    [0.00, 3.00, 10.00, 17.00], // m11
    [0.00, 2.00,  9.00, 16.00], // 69
    [0.00, 4.00, 11.00, 14.00], // M9
    [0.00, 4.00,  7.00, 11.00], // M7
    [0.00, 4.00,  7.00, 12.00], // M
];

/// How many different pitch classes `chord` has: notes less than a tenth of
/// a semitone apart, octaves folded, count once
pub fn pitch_classes(chord: &Chord) -> i32 {
    let mut count = 0;
    for (i, &note) in chord.iter().enumerate() {
        let repeated = chord[..i].iter().any(|&earlier| {
            let d = (note - earlier) % 12.0;
            let d = if d < 0.0 { d + 12.0 } else { d };
            d < 0.1 || d > 11.9
        });
        if !repeated {
            count += 1;
        }
    }
    count
}

#[derive(Debug)]
pub struct ChordBank {
    chord_index_quantizer: HysteresisQuantizer2,
    ratios: [[f32; NUM_NOTES]; MAX_CHORDS],
    sorted_ratios: [f32; NUM_NOTES],
    note_count: [i32; MAX_CHORDS],
    user_chords: Option<&'static [Chord]>,
}

impl Default for ChordBank {
    fn default() -> Self {
        Self {
            chord_index_quantizer: HysteresisQuantizer2::default(),
            ratios: [[0.0; NUM_NOTES]; MAX_CHORDS],
            sorted_ratios: [0.0; NUM_NOTES],
            note_count: [0; MAX_CHORDS],
            user_chords: None,
        }
    }
}

impl ChordBank {
    pub fn init(&mut self) {
        self.chord_index_quantizer
            .init(NUM_CHORDS as i32, 0.075, false);
    }

    /// Plays `chords` (1..=[`MAX_CHORDS`]; more are ignored, none is the
    /// same as `None`) instead of the 11 bundled chords, or the bundled
    /// ones again with `None`. Takes effect at the next [`ChordBank::reset`].
    pub fn set_user_chords(&mut self, chords: Option<&'static [Chord]>) {
        self.user_chords = chords.filter(|c| !c.is_empty()).map(|c| &c[..c.len().min(MAX_CHORDS)]);
    }

    /// How many chords HARMONICS steps through
    pub fn num_chords(&self) -> usize {
        self.user_chords.map_or(NUM_CHORDS, |c| c.len())
    }

    pub fn reset(&mut self) {
        if let Some(chords) = self.user_chords {
            self.reset_user(chords);
            return;
        }
        if self.chord_index_quantizer.num_steps() != NUM_CHORDS as i32 {
            // Back from a user table
            self.init();
        }
        for i in 0..NUM_CHORDS {
            let mut count = 0;
            for j in 0..NUM_NOTES {
                self.ratios[i][j] = semitones_to_ratio(CHORDS[i][j]);
                if CHORDS[i][j] != 0.01
                    && CHORDS[i][j] != 7.01
                    && CHORDS[i][j] != 11.99
                    && CHORDS[i][j] != 12.00
                {
                    count += 1;
                }
            }
            self.note_count[i] = count;
        }
        self.sort();
    }

    /// `reset` with a user table. The C counts a chord's notes by leaving
    /// out its marker values (0.01, 7.01, 11.99, 12.00, which double a note
    /// or its octave); a user chord counts each pitch class once, which
    /// gives the same counts for the bundled chords.
    fn reset_user(&mut self, chords: &[Chord]) {
        if self.chord_index_quantizer.num_steps() != chords.len() as i32 {
            self.chord_index_quantizer.init(chords.len() as i32, 0.075, false);
        }
        for (i, chord) in chords.iter().enumerate() {
            for (ratio, &note) in self.ratios[i].iter_mut().zip(chord) {
                *ratio = semitones_to_ratio(note);
            }
            self.note_count[i] = pitch_classes(chord);
        }
        self.sort();
    }

    pub fn sort(&mut self) {
        for i in 0..NUM_NOTES {
            let mut r = self.ratio(i);
            while r > 2.0 {
                r *= 0.5;
            }
            self.sorted_ratios[i] = r;
        }
        self.sorted_ratios
            .sort_unstable_by(|a, b| a.partial_cmp(b).unwrap());
    }

    #[inline]
    pub fn set_chord(&mut self, parameter: f32) {
        self.chord_index_quantizer.process(parameter * 1.02);
    }

    #[inline]
    pub fn chord_index(&self) -> usize {
        self.chord_index_quantizer.quantized_value() as usize
    }

    #[inline]
    pub fn ratios(&self) -> &[f32; NUM_NOTES] {
        &self.ratios[self.chord_index()]
    }

    #[inline]
    pub fn ratio(&self, note: usize) -> f32 {
        self.ratios[self.chord_index()][note]
    }

    #[inline]
    pub fn sorted_ratio(&self, note: usize) -> f32 {
        self.sorted_ratios[note]
    }

    #[inline]
    pub fn num_notes(&self) -> i32 {
        self.note_count[self.chord_index()]
    }

    /// `ComputeChordInversion(inversion, ratios, amplitudes)` -- spreads the
    /// current chord's notes across `NUM_VOICES` voices for the given
    /// inversion amount, returns a bitmask of which voices are the two
    /// currently cross-fading (root-adjacent) ones.
    pub fn compute_chord_inversion(
        &self,
        inversion: f32,
        ratios: &mut [f32; NUM_VOICES],
        amplitudes: &mut [f32; NUM_VOICES],
    ) -> u32 {
        let base_ratio = self.ratios();
        let inversion = inversion * (NUM_NOTES * NUM_VOICES) as f32;

        let inversion_integral = inversion as i32;
        let inversion_fractional = inversion - inversion_integral as f32;

        let num_rotations = inversion_integral / NUM_NOTES as i32;
        let rotated_note = inversion_integral % NUM_NOTES as i32;

        const BASE_GAIN: f32 = 0.25;
        let mut mask = 0u32;

        for i in 0..NUM_NOTES as i32 {
            let transposition = 0.25
                * (1i32
                    << (((NUM_NOTES as i32 - 1 + inversion_integral - i) / NUM_NOTES as i32)
                        as u32)) as f32;
            let target_voice = (i - num_rotations).rem_euclid(NUM_VOICES as i32) as usize;
            let previous_voice = (target_voice as i32 - 1).rem_euclid(NUM_VOICES as i32) as usize;

            if i == rotated_note {
                ratios[target_voice] = base_ratio[i as usize] * transposition;
                ratios[previous_voice] = ratios[target_voice] * 2.0;
                amplitudes[previous_voice] = BASE_GAIN * inversion_fractional;
                amplitudes[target_voice] = BASE_GAIN * (1.0 - inversion_fractional);
            } else if i < rotated_note {
                ratios[previous_voice] = base_ratio[i as usize] * transposition;
                amplitudes[previous_voice] = BASE_GAIN;
            } else {
                ratios[target_voice] = base_ratio[i as usize] * transposition;
                amplitudes[target_voice] = BASE_GAIN;
            }

            if i == 0 {
                if i >= rotated_note {
                    mask |= 1 << target_voice;
                }
                if i <= rotated_note {
                    mask |= 1 << previous_voice;
                }
            }
        }
        mask
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pitch_classes_count_the_bundled_chords_as_the_c_does() {
        let mut bank = ChordBank::default();
        bank.init();
        bank.reset();
        for (i, chord) in CHORDS.iter().enumerate() {
            assert_eq!(pitch_classes(chord), bank.note_count[i], "chord {i}");
        }
    }

    #[test]
    fn harmonics_reaches_every_user_chord() {
        static SIXTEEN: [Chord; MAX_CHORDS] = {
            let mut t = [[0.0; NUM_NOTES]; MAX_CHORDS];
            let mut i = 0;
            while i < MAX_CHORDS {
                t[i] = [0.0, i as f32, 7.0, 12.0];
                i += 1;
            }
            t
        };
        let mut bank = ChordBank::default();
        bank.init();
        bank.set_user_chords(Some(&SIXTEEN));
        bank.reset();
        assert_eq!(bank.num_chords(), MAX_CHORDS);
        for h in [0.0, 1.0, 0.0] {
            for _ in 0..4 {
                bank.set_chord(h);
            }
            assert_eq!(bank.chord_index(), if h > 0.5 { MAX_CHORDS - 1 } else { 0 });
        }
        // Back to the bundled chords: 11 again
        bank.set_user_chords(None);
        bank.reset();
        bank.set_chord(1.0);
        assert_eq!((bank.num_chords(), bank.chord_index()), (NUM_CHORDS, NUM_CHORDS - 1));
        // None and an empty table are the same
        bank.set_user_chords(Some(&[]));
        assert_eq!(bank.num_chords(), NUM_CHORDS);
    }
}
