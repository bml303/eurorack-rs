//! `streams/envelope.{h,cc}` -- a simple AD/AR/ADSAR/ADAR envelope
//! generator, adapted from Peaks' multistage envelope.
//!
//! **A vestigial dead branch, preserved as-is**: `Process`'s local
//! `release` is declared `false`, and the only statement that could set it
//! (`gate_ = false; release = false;`, on the gate's falling edge) sets it
//! to `false` too -- `release` is provably always `false` at the `else if
//! (release && sustain_point_)` check below it, making that whole branch
//! dead code in the shipped firmware. Kept verbatim (not simplified away)
//! since it costs nothing and removes any risk of this analysis being
//! wrong; see `PORTING.md`.

use crate::consts::{K_ABOVE_UNITY_GAIN, K_SCHMITT_TRIGGER_THRESHOLD};
use crate::meta_parameters::{compute_amount_offset, compute_attack_decay};
use crate::resources::{LUT_ENV_INCREMENTS, LOOKUP_TABLE_TABLE};
use stmlib::fixed::interpolate_824_u16;

pub const K_MAX_NUM_SEGMENTS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u16)]
pub enum EnvelopeShape {
    #[default]
    Linear,
    Exponential,
    Quartic,
}

#[derive(Debug, Clone, Copy)]
pub struct Envelope {
    gate: bool,

    level: [i16; K_MAX_NUM_SEGMENTS],
    time: [u16; K_MAX_NUM_SEGMENTS],
    shape: [EnvelopeShape; K_MAX_NUM_SEGMENTS],

    segment: i16,
    start_value: i16,
    value: i16,

    phase: u32,
    phase_increment: u32,

    num_segments: u16,
    sustain_point: u16,

    target_frequency_amount: i32,
    target_frequency_offset: i32,
    frequency_amount: i32,
    frequency_offset: i32,

    attack: u16,
    decay: u16,

    alternate: bool,
    hard_reset: bool,

    rate_modulation: i32,
    gate_level: i32,
}

impl Default for Envelope {
    fn default() -> Self {
        Self {
            gate: false,
            level: [0; K_MAX_NUM_SEGMENTS],
            time: [0; K_MAX_NUM_SEGMENTS],
            shape: [EnvelopeShape::Linear; K_MAX_NUM_SEGMENTS],
            segment: 0,
            start_value: 0,
            value: 0,
            phase: 0,
            phase_increment: 0,
            num_segments: 0,
            sustain_point: 0,
            target_frequency_amount: 0,
            target_frequency_offset: 0,
            frequency_amount: 0,
            frequency_offset: 0,
            attack: 0,
            decay: 0,
            alternate: false,
            hard_reset: false,
            rate_modulation: 0,
            gate_level: 0,
        }
    }
}

impl Envelope {
    pub fn init(&mut self) {
        self.shape = [EnvelopeShape::Linear; K_MAX_NUM_SEGMENTS];
        self.set_ad(0, 8192);
        self.segment = self.num_segments as i16;
        self.phase = 0;
        self.phase_increment = 0;
        self.start_value = 0;
        self.value = 0;
        self.rate_modulation = 0;
        self.gate_level = 0;
        self.gate = false;
        self.hard_reset = false;
        self.attack = 0;
        self.decay = 0;
    }

    pub fn set_time(&mut self, segment: usize, time: u16) {
        self.time[segment] = time;
    }
    pub fn set_level(&mut self, segment: usize, level: i16) {
        self.level[segment] = level;
    }
    pub fn set_num_segments(&mut self, num_segments: u16) {
        self.num_segments = num_segments;
    }
    pub fn set_sustain_point(&mut self, sustain_point: u16) {
        self.sustain_point = sustain_point;
    }

    /// `num_segments` must not exceed `K_MAX_NUM_SEGMENTS - 1` -- `Process`
    /// reads `level[segment + 1]`/`shape[segment]` with `segment` ranging up
    /// to `num_segments` itself (the "done" state), same constraint the C++
    /// array capacity already implies. The firmware only ever configures up
    /// to 4 segments (`set_adsar`/`set_adar`), well inside that limit.
    pub fn set_ad(&mut self, attack: u16, decay: u16) {
        self.num_segments = 2;
        self.sustain_point = 0;

        self.level[0] = 0;
        self.level[1] = 32767;
        self.level[2] = 0;

        self.time[0] = attack;
        self.time[1] = decay;

        self.shape[0] = EnvelopeShape::Linear;
        self.shape[1] = EnvelopeShape::Exponential;
    }

    pub fn set_adr(&mut self, attack: u16, decay: u16, sustain: u16, release: u16) {
        self.num_segments = 3;
        self.sustain_point = 0;

        self.level[0] = 0;
        self.level[1] = 32767;
        self.level[2] = sustain as i16;
        self.level[3] = 0;

        self.time[0] = attack;
        self.time[1] = decay;
        self.time[2] = release;

        self.shape[0] = EnvelopeShape::Linear;
        self.shape[1] = EnvelopeShape::Linear;
        self.shape[2] = EnvelopeShape::Linear;
    }

    pub fn set_ar(&mut self, attack: u16, decay: u16) {
        self.num_segments = 2;
        self.sustain_point = 1;

        self.level[0] = 0;
        self.level[1] = 32767;
        self.level[2] = 0;

        self.time[0] = attack;
        self.time[1] = decay;

        self.shape[0] = EnvelopeShape::Linear;
        self.shape[1] = EnvelopeShape::Linear;
    }

    pub fn set_adsar(&mut self, attack: u16, decay: u16, sustain: u16, release: u16) {
        self.num_segments = 4;
        self.sustain_point = 2;

        self.level[0] = 0;
        self.level[1] = 32767;
        self.level[2] = sustain as i16;
        self.level[3] = 32767;
        self.level[4] = 0;

        self.time[0] = attack;
        self.time[1] = decay;
        self.time[2] = attack;
        self.time[3] = release;

        self.shape[0] = EnvelopeShape::Linear;
        self.shape[1] = EnvelopeShape::Linear;
        self.shape[2] = EnvelopeShape::Linear;
        self.shape[3] = EnvelopeShape::Linear;
    }

    pub fn set_adar(&mut self, attack: u16, decay: u16, sustain: u16, release: u16) {
        self.num_segments = 4;
        self.sustain_point = 0;

        self.level[0] = 0;
        self.level[1] = 32767;
        self.level[2] = sustain as i16;
        self.level[3] = 32767;
        self.level[4] = 0;

        self.time[0] = attack;
        self.time[1] = decay;
        self.time[2] = attack;
        self.time[3] = release;

        self.shape[0] = EnvelopeShape::Linear;
        self.shape[1] = EnvelopeShape::Linear;
        self.shape[2] = EnvelopeShape::Linear;
        self.shape[3] = EnvelopeShape::Linear;
    }

    pub fn set_hard_reset(&mut self, hard_reset: bool) {
        self.hard_reset = hard_reset;
    }

    pub fn configure(&mut self, alternate: bool, parameters: &[i32; 2], globals: Option<&[i32; 4]>) {
        let (a, d) = if let Some(globals) = globals {
            let a = globals[0] as u16;
            let d = globals[2] as u16;
            (a, d)
        } else {
            compute_attack_decay(parameters[0])
        };
        let (target_frequency_amount, target_frequency_offset) = compute_amount_offset(parameters[1]);
        self.target_frequency_amount = target_frequency_amount;
        self.target_frequency_offset = target_frequency_offset;

        if a != self.attack || d != self.decay || alternate != self.alternate {
            self.attack = a;
            self.decay = d;
            self.alternate = alternate;
            if self.alternate {
                self.set_ar(a, d);
            } else {
                self.set_ad(a, d);
            }
            self.set_hard_reset(true);
        }
    }

    pub fn process(&mut self, _audio: i16, excite: i16, gain: &mut u16, frequency: &mut u16) {
        self.frequency_amount = self.frequency_amount.wrapping_add((self.target_frequency_amount - self.frequency_amount) >> 8);
        self.frequency_offset = self.frequency_offset.wrapping_add((self.target_frequency_offset - self.frequency_offset) >> 8);

        let mut trigger = false;
        let mut release = false;
        if !self.gate {
            if excite as i32 > K_SCHMITT_TRIGGER_THRESHOLD {
                trigger = true;
                self.gate = true;
                self.set_hard_reset(false);
            }
        } else if (excite as i32) < (K_SCHMITT_TRIGGER_THRESHOLD >> 1) {
            self.gate = false;
            release = false;
        } else {
            self.gate_level = self.gate_level.wrapping_add((excite as i32 - self.gate_level) >> 8);
        }

        if trigger {
            self.start_value = if self.segment == self.num_segments as i16 || self.hard_reset { self.level[0] } else { self.value };
            self.segment = 0;
            self.phase = 0;
        } else if release && self.sustain_point != 0 {
            self.start_value = self.value;
            self.segment = self.sustain_point as i16;
            self.phase = 0;
        } else if self.phase < self.phase_increment {
            self.start_value = self.level[self.segment as usize + 1];
            self.segment += 1;
            self.phase = 0;
        }

        let done = self.segment == self.num_segments as i16;
        let sustained = self.sustain_point != 0 && self.segment == self.sustain_point as i16 && self.gate;
        let mut increment: u32 = if sustained || done { 0 } else { LUT_ENV_INCREMENTS[(self.time[self.segment as usize] >> 8) as usize] };

        // Modulates the envelope rate by the actual excitation pulse.
        let excite_gated = if excite as i32 > K_SCHMITT_TRIGGER_THRESHOLD { excite as i32 } else { 0 };
        self.rate_modulation = self.rate_modulation.wrapping_add(excite_gated.wrapping_sub(self.rate_modulation) >> 12);
        let delta = ((increment >> 7) as i32).wrapping_mul(self.rate_modulation >> 7);
        increment = increment.wrapping_add(delta as u32);

        self.phase_increment = increment;

        let a = self.start_value as i32;
        let b = self.level[self.segment as usize + 1] as i32;
        let t = interpolate_824_u16(LOOKUP_TABLE_TABLE[self.shape[self.segment as usize] as usize], self.phase);
        self.value = a.wrapping_add((b.wrapping_sub(a)).wrapping_mul((t >> 1) as i32) >> 15) as i16;
        self.phase = self.phase.wrapping_add(self.phase_increment);

        // Applies a variable amount of distortion, depending on the level.
        let value = self.value as i32;
        let mut compressed = 32767 - ((32767 - value).wrapping_mul(32767 - value) >> 15);
        compressed = 32767 - ((32767 - compressed).wrapping_mul(32767 - compressed) >> 15);
        let mut scaled = value.wrapping_add((compressed.wrapping_sub(value)).wrapping_mul(self.gate_level) >> 15);
        scaled = scaled.wrapping_mul(28672 + (self.gate_level >> 3)) >> 15;
        *gain = (scaled.wrapping_mul(K_ABOVE_UNITY_GAIN) >> 15) as u16;
        *frequency = self.frequency_offset.wrapping_add(scaled.wrapping_mul(self.frequency_amount) >> 15) as u16;
    }
}
