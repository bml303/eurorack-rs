//! `grids/clock.{cc,h}` -- `Clock`: the global 32-bit phase-increment clock.
//! To implement swing, the value the counter wraps at is `1 << 31` times a
//! swing factor.
//!
//! `Clock::Wrap`/`past_falling_edge` reinterpret `phase_` (a `uint32_t`) as
//! a `LongWord` union to read/write just its top byte (`bytes[3]`, the most
//! significant byte on AVR's little-endian layout). Ported as plain shifts/
//! masks on the `u32` directly (`(phase >> 24) as u8` / clearing bit 31 /
//! zeroing the top byte) -- the identical operation, no union or `unsafe`
//! needed.

use crate::pattern_generator::ClockResolution;
use crate::resources::LUT_RES_TEMPO_PHASE_INCREMENT;

#[derive(Debug, Clone, Copy, Default)]
pub struct Clock {
    locked: bool,
    bpm: u16,
    phase: u32,
    phase_increment: u32,
    falling_edge: u8,
}

impl Clock {
    pub fn new() -> Self {
        let mut c = Self::default();
        c.init();
        c
    }

    pub fn init(&mut self) {
        self.update(120, ClockResolution::Ppqn24);
        self.locked = false;
    }

    pub fn update(&mut self, bpm: u16, resolution: ClockResolution) {
        self.bpm = bpm;
        self.phase_increment = LUT_RES_TEMPO_PHASE_INCREMENT[bpm as usize];
        match resolution {
            ClockResolution::Ppqn4 => self.phase_increment >>= 1,
            ClockResolution::Ppqn24 => self.phase_increment = (self.phase_increment << 1) + self.phase_increment,
            ClockResolution::Ppqn8 => {}
        }
    }

    pub fn reset(&mut self) {
        self.phase = 0;
    }

    pub fn tick(&mut self) {
        self.phase = self.phase.wrapping_add(self.phase_increment);
    }

    pub fn wrap(&mut self, amount: i8) {
        if amount == 0 {
            self.phase &= 0x7fff_ffff; // top byte &= 0x7f
            self.falling_edge = 0x40;
        } else {
            let top_byte = (self.phase >> 24) as u8;
            let threshold = (128i16 + amount as i16) as u8;
            if top_byte >= threshold {
                self.phase &= 0x00ff_ffff; // top byte = 0
            }
            self.falling_edge = ((128i16 + amount as i16) >> 1) as u8;
        }
    }

    pub fn raising_edge(&self) -> bool {
        self.phase < self.phase_increment
    }

    pub fn past_falling_edge(&self) -> bool {
        (self.phase >> 24) as u8 >= self.falling_edge
    }

    pub fn lock(&mut self) {
        self.locked = true;
    }
    pub fn unlock(&mut self) {
        self.locked = false;
    }
    pub fn locked(&self) -> bool {
        self.locked
    }
    pub fn bpm(&self) -> u16 {
        self.bpm
    }
}
