//! `streams/svf.{h,cc}` -- a state-variable filter used as the analysis
//! filter bank in [`crate::follower::Follower`].

use stmlib::clip16_sym;
use stmlib::fixed::interpolate_824_u16;

use crate::resources::{LUT_SVF_CUTOFF, LUT_SVF_DAMP};

#[derive(Debug, Clone, Copy, Default)]
pub struct Svf {
    dirty: bool,

    frequency: i16,
    resonance: i16,

    f: i32,
    damp: i32,

    lp: i32,
    bp: i32,
    hp: i32,
}

impl Svf {
    pub fn init(&mut self) {
        self.lp = 0;
        self.bp = 0;
        self.frequency = 33 << 7;
        self.resonance = 16384;
        self.dirty = true;
    }

    pub fn set_frequency(&mut self, frequency: i16) {
        self.dirty = self.dirty || (self.frequency != frequency);
        self.frequency = frequency;
    }

    pub fn set_resonance(&mut self, resonance: i16) {
        self.resonance = resonance;
        self.dirty = true;
    }

    pub fn process(&mut self, sample: i32) {
        if self.dirty {
            // Both shifts genuinely overflow i32 for realistic frequency/
            // resonance values (e.g. the `init` default `resonance_ = 16384`
            // shifted left 17 is exactly 2^31) -- the C relies on this
            // wrapping into a large uint32_t phase when passed to
            // `Interpolate824`, so this port does too.
            self.f = interpolate_824_u16(&LUT_SVF_CUTOFF, ((self.frequency as i32) << 17) as u32) as i32;
            self.damp = interpolate_824_u16(&LUT_SVF_DAMP, ((self.resonance as i32) << 17) as u32) as i32;
            self.dirty = false;
        }
        let f = self.f;
        let damp = self.damp;
        let notch = sample.wrapping_sub(self.bp.wrapping_mul(damp) >> 15);
        self.lp = clip16_sym(self.lp.wrapping_add(f.wrapping_mul(self.bp) >> 15));
        self.hp = notch.wrapping_sub(self.lp);
        self.bp = clip16_sym(self.bp.wrapping_add(f.wrapping_mul(self.hp) >> 15));
        self.hp = clip16_sym(self.hp);
    }

    pub fn lp(&self) -> i32 {
        self.lp
    }
    pub fn bp(&self) -> i32 {
        self.bp
    }
    pub fn hp(&self) -> i32 {
        self.hp
    }
}
