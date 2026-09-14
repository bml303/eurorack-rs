//! `peaks/drums/svf.h` -- the state-variable filter used by the drum
//! synthesis engines, with an added "punch" parameter (a resonance/cutoff
//! kick proportional to the current low-pass level, for a percussive
//! transient) on top of `mi-streams`' otherwise-identical `Svf`.

use stmlib::clip16_sym;
use stmlib::fixed::interpolate_824_u16;

use crate::resources::{LUT_SVF_CUTOFF, LUT_SVF_DAMP};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SvfMode {
    Lp,
    Bp,
    Hp,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Svf {
    dirty: bool,

    frequency: i16,
    resonance: i16,

    punch: i32,
    f: i32,
    damp: i32,

    lp: i32,
    bp: i32,
}

impl Svf {
    pub fn init(&mut self) {
        self.lp = 0;
        self.bp = 0;
        self.frequency = 33 << 7;
        self.resonance = 16384;
        self.dirty = true;
        self.punch = 0;
    }

    pub fn set_frequency(&mut self, frequency: i16) {
        self.dirty = self.dirty || (self.frequency != frequency);
        self.frequency = frequency;
    }

    pub fn set_resonance(&mut self, resonance: i16) {
        self.resonance = resonance;
        self.dirty = true;
    }

    pub fn set_punch(&mut self, punch: u16) {
        self.punch = ((punch as u32).wrapping_mul(punch as u32) >> 24) as i32;
    }

    pub fn process(&mut self, mode: SvfMode, input: i32) -> i32 {
        if self.dirty {
            self.f = interpolate_824_u16(&LUT_SVF_CUTOFF, ((self.frequency as i32) << 17) as u32) as i32;
            self.damp = interpolate_824_u16(&LUT_SVF_DAMP, ((self.resonance as i32) << 17) as u32) as i32;
            self.dirty = false;
        }
        let mut f = self.f;
        let mut damp = self.damp;
        if self.punch != 0 {
            let punch_signal = if self.lp > 4096 { self.lp } else { 2048 };
            f = f.wrapping_add(((punch_signal >> 4).wrapping_mul(self.punch)) >> 9);
            damp = damp.wrapping_add((punch_signal - 2048) >> 3);
        }
        let notch = input.wrapping_sub(self.bp.wrapping_mul(damp) >> 15);
        self.lp = clip16_sym(self.lp.wrapping_add(f.wrapping_mul(self.bp) >> 15));
        let hp = notch.wrapping_sub(self.lp);
        self.bp = clip16_sym(self.bp.wrapping_add(f.wrapping_mul(hp) >> 15));

        match mode {
            SvfMode::Bp => self.bp,
            SvfMode::Hp => hp,
            SvfMode::Lp => self.lp,
        }
    }
}
