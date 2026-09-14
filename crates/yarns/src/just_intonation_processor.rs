//! `yarns/just_intonation_processor.{h,cc}` -- a simple just-intonation
//! tuner. A history of the previous 16 notes is kept in a circular buffer.
//! Each note is given a weight: the weight of a note stays `255` while the
//! note is still heard, but decays exponentially once it's released.
//!
//! To tune a note, each possible tuning in the +/- 1 quartertone range is
//! considered. For each tuning, a "badness" score is computed: the weighted
//! sum of the "dissonance" score of the interval between the candidate
//! tuning and the tuning of each previous note in the history. The
//! dissonance score is read from a lookup table -- low for just intervals
//! (say 4/3), higher for more convoluted ratios (say 32/27), rising with a
//! square law as the interval moves away from the just intervals. The
//! tuning with the least badness score is selected.

use crate::resources::LUT_CONSONANCE;

const HISTORY_SIZE: usize = 16;
const OCTAVE: i32 = 12 << 7;

#[derive(Debug, Clone, Copy, Default)]
struct HistoryEntry {
    note: u8,
    weight: u8,
    pitch: i16,
}

#[derive(Debug, Clone)]
pub struct JustIntonationProcessor {
    write_ptr: usize,
    cached_pitch: i16,
    cached_note: u8,
    history: [HistoryEntry; HISTORY_SIZE],
}

impl Default for JustIntonationProcessor {
    fn default() -> Self {
        Self {
            write_ptr: 0,
            cached_pitch: 0,
            cached_note: 0,
            history: [HistoryEntry::default(); HISTORY_SIZE],
        }
    }
}

impl JustIntonationProcessor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn init(&mut self) {
        self.write_ptr = 0;
        self.cached_note = 0xff;
        self.cached_pitch = 0;
        self.history = [HistoryEntry::default(); HISTORY_SIZE];
    }

    pub fn note_off(&mut self, note: u8) {
        for entry in self.history.iter_mut() {
            if entry.note == note && entry.weight == 255 {
                entry.weight = 192;
            }
        }
    }

    pub fn note_on(&mut self, note: u8) -> i16 {
        if note != self.cached_note {
            // Skip the computationally expensive routine on repeated notes.
            self.cached_note = note;
            self.cached_pitch = self.tune((note as i32) << 7);
        }
        // Decay the weight of the previous notes - except those that are
        // still playing.
        for entry in self.history.iter_mut() {
            if entry.weight != 255 {
                entry.weight = ((entry.weight as u32 * 3) >> 2) as u8;
            }
        }
        self.history[self.write_ptr] = HistoryEntry {
            note,
            weight: 255,
            pitch: self.cached_pitch,
        };
        self.write_ptr += 1;
        if self.write_ptr >= HISTORY_SIZE {
            self.write_ptr = 0;
        }
        self.cached_pitch
    }

    fn tune_range(&self, note: i32, min: i32, max: i32, step: i32) -> i32 {
        let mut best_score = 0x7fff_ffffi32;
        let mut best_correction = 0;
        let mut correction = min;
        while correction <= max {
            let mut score = LUT_CONSONANCE
                [if correction >= 0 { correction } else { OCTAVE + correction } as usize]
                as i32;
            let pitch = correction + note;
            for entry in self.history.iter() {
                let interval = (pitch - entry.pitch as i32 + OCTAVE * 12) % OCTAVE;
                score += LUT_CONSONANCE[interval as usize] as i32 * entry.weight as i32;
                if score > best_score {
                    break;
                }
            }
            if score < best_score {
                best_correction = correction;
                best_score = score;
            }
            correction += step;
        }
        best_correction
    }

    fn tune(&self, note: i32) -> i16 {
        let coarse = self.tune_range(note, -32, 32, 4);
        (note + self.tune_range(note, coarse - 6, coarse + 6, 1)) as i16
    }
}
