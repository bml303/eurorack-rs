//! `plaits/dsp/chords/chord_bank.h` -- the 11-chord table shared by
//! `ChordEngine` and `StringMachineEngine`, plus inversion voicing.

use stmlib::units::semitones_to_ratio;
use stmlib::HysteresisQuantizer2;

pub const NUM_NOTES: usize = 4;
pub const NUM_VOICES: usize = NUM_NOTES + 1;
pub const NUM_CHORDS: usize = 11;

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

#[derive(Debug)]
pub struct ChordBank {
    chord_index_quantizer: HysteresisQuantizer2,
    ratios: [[f32; NUM_NOTES]; NUM_CHORDS],
    sorted_ratios: [f32; NUM_NOTES],
    note_count: [i32; NUM_CHORDS],
}

impl Default for ChordBank {
    fn default() -> Self {
        Self {
            chord_index_quantizer: HysteresisQuantizer2::default(),
            ratios: [[0.0; NUM_NOTES]; NUM_CHORDS],
            sorted_ratios: [0.0; NUM_NOTES],
            note_count: [0; NUM_CHORDS],
        }
    }
}

impl ChordBank {
    pub fn init(&mut self) {
        self.chord_index_quantizer
            .init(NUM_CHORDS as i32, 0.075, false);
    }

    pub fn reset(&mut self) {
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
