//! `braids/svf.h` -- Chamberlin state-variable filter used to model the bridged
//! T-networks in the drum models. Fixed-point, bit-faithful to the C.

use stmlib::clip16_sym;
use stmlib::fixed::interpolate_824_u16;

use crate::resources::{LUT_SVF_CUTOFF, LUT_SVF_DAMP};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SvfMode {
    Lp,
    #[default]
    Bp,
    Hp,
}

#[derive(Debug, Clone)]
pub struct Svf {
    dirty: bool,
    frequency: i16,
    resonance: i16,
    punch: i32,
    f: i32,
    damp: i32,
    lp: i32,
    bp: i32,
    mode: SvfMode,
}

impl Default for Svf {
    fn default() -> Self {
        Self::new()
    }
}

impl Svf {
    pub fn new() -> Self {
        let mut s = Self {
            dirty: true,
            frequency: 33 << 7,
            resonance: 16384,
            punch: 0,
            f: 0,
            damp: 0,
            lp: 0,
            bp: 0,
            mode: SvfMode::Bp,
        };
        s.init();
        s
    }

    pub fn init(&mut self) {
        self.lp = 0;
        self.bp = 0;
        self.frequency = 33 << 7;
        self.resonance = 16384;
        self.dirty = true;
        self.punch = 0;
        self.mode = SvfMode::Bp;
    }

    #[inline]
    pub fn set_frequency(&mut self, frequency: i16) {
        self.dirty = self.dirty || (self.frequency != frequency);
        self.frequency = frequency;
    }

    #[inline]
    pub fn set_resonance(&mut self, resonance: i16) {
        self.resonance = resonance;
        self.dirty = true;
    }

    #[inline]
    pub fn set_punch(&mut self, punch: u16) {
        self.punch = ((punch as u32 * punch as u32) >> 24) as i32;
    }

    #[inline]
    pub fn set_mode(&mut self, mode: SvfMode) {
        self.mode = mode;
    }

    #[inline]
    pub fn process(&mut self, input: i32) -> i32 {
        if self.dirty {
            self.f =
                interpolate_824_u16(&LUT_SVF_CUTOFF, ((self.frequency as i32) << 17) as u32) as i32;
            self.damp =
                interpolate_824_u16(&LUT_SVF_DAMP, ((self.resonance as i32) << 17) as u32) as i32;
            self.dirty = false;
        }
        let mut f = self.f;
        let mut damp = self.damp;
        if self.punch != 0 {
            let punch_signal = if self.lp > 4096 { self.lp } else { 2048 };
            f += ((punch_signal >> 4) * self.punch) >> 9;
            damp += (punch_signal - 2048) >> 3;
        }
        let notch = input - (self.bp * damp >> 15);
        self.lp += f * self.bp >> 15;
        self.lp = clip16_sym(self.lp);
        let hp = notch - self.lp;
        self.bp += f * hp >> 15;
        self.bp = clip16_sym(self.bp);
        match self.mode {
            SvfMode::Bp => self.bp,
            SvfMode::Hp => hp,
            SvfMode::Lp => self.lp,
        }
    }
}
