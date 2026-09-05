//! `plaits/dsp/engine2/arpeggiator.h` -- a simple up/down/up-down/random
//! arpeggiator over a chord's notes and a range of octaves, used by
//! [`super::chiptune_engine::ChiptuneEngine`] in its clocked mode.

use stmlib::Random;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ArpeggiatorMode {
    #[default]
    Up,
    Down,
    UpDown,
    Random,
}

#[derive(Default, Debug)]
pub struct Arpeggiator {
    mode: ArpeggiatorMode,
    range: i32,
    note: i32,
    octave: i32,
    direction: i32,
}

impl ArpeggiatorMode {
    /// `ArpeggiatorMode(pattern / 3)` in the C -- casting a small integer
    /// (0..=3, from `pattern` in 0..12) to the enum.
    pub fn from_index(index: i32) -> Self {
        match index {
            0 => ArpeggiatorMode::Up,
            1 => ArpeggiatorMode::Down,
            2 => ArpeggiatorMode::UpDown,
            _ => ArpeggiatorMode::Random,
        }
    }
}

impl Arpeggiator {
    pub fn init(&mut self) {
        self.mode = ArpeggiatorMode::Up;
        self.reset();
    }

    pub fn reset(&mut self) {
        self.note = 0;
        self.octave = 0;
        self.direction = 1;
    }

    #[inline]
    pub fn set_mode(&mut self, mode: ArpeggiatorMode) {
        self.mode = mode;
    }

    #[inline]
    pub fn set_range(&mut self, range: i32) {
        self.range = range;
    }

    #[inline]
    pub fn note(&self) -> i32 {
        self.note
    }
    #[inline]
    pub fn octave(&self) -> i32 {
        self.octave
    }

    pub fn clock(&mut self, num_notes: i32) {
        if num_notes == 1 && self.range == 1 {
            self.note = 0;
            self.octave = 0;
            return;
        }

        if self.mode == ArpeggiatorMode::Random {
            loop {
                let w = Random::get_word();
                let octave = ((w >> 4) % self.range as u32) as i32;
                let note = ((w >> 20) % num_notes as u32) as i32;
                if octave != self.octave || note != self.note {
                    self.octave = octave;
                    self.note = note;
                    break;
                }
            }
            return;
        }

        if self.mode == ArpeggiatorMode::Up {
            self.direction = 1;
        }
        if self.mode == ArpeggiatorMode::Down {
            self.direction = -1;
        }

        self.note += self.direction;

        let mut done = false;
        while !done {
            done = true;
            if self.note >= num_notes || self.note < 0 {
                self.octave += self.direction;
                self.note = if self.direction > 0 { 0 } else { num_notes - 1 };
            }
            if self.octave >= self.range || self.octave < 0 {
                self.octave = if self.direction > 0 {
                    0
                } else {
                    self.range - 1
                };
                if self.mode == ArpeggiatorMode::UpDown {
                    self.direction = -self.direction;
                    self.note = if self.direction > 0 { 1 } else { num_notes - 2 };
                    self.octave = if self.direction > 0 {
                        0
                    } else {
                        self.range - 1
                    };
                    done = false;
                }
            }
        }
    }
}
