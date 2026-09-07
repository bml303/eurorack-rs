//! `elements/dsp/multistage_envelope.{h,cc}` -- a float re-implementation of
//! Peaks' segment envelope, used here only in its ADSR configuration.
//!
//! The C exposes a dozen `set_ad*` presets; Elements' voices only ever call
//! [`MultistageEnvelope::set_adsr`], so that is all this port keeps.

use crate::resources::{LOOKUP_TABLE_TABLE, LUT_ENV_INCREMENTS};

/// Segment interpolation shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvelopeShape {
    Linear,
    Exponential,
    Quartic,
}

/// `LUT_ENV_LINEAR` -- index of the first of the three segment-shape curves in
/// `LOOKUP_TABLE_TABLE` (`_LINEAR`, `_EXPO`, `_QUARTIC` are consecutive).
const LUT_ENV_LINEAR: usize = 10;

/// Gate transition flags, matching `EnvelopeFlags` in the C.
pub const FLAG_RISING_EDGE: u8 = 1;
pub const FLAG_FALLING_EDGE: u8 = 2;
pub const FLAG_GATE: u8 = 4;

const MAX_NUM_SEGMENTS: usize = 6;

#[inline]
fn interpolate8(table: &[f32], index: f32) -> f32 {
    let index = index * 256.0;
    let integral = index as usize;
    let fractional = index - integral as f32;
    let a = table[integral];
    let b = table[integral + 1];
    a + (b - a) * fractional
}

/// `elements::MultistageEnvelope`.
#[derive(Debug, Clone)]
pub struct MultistageEnvelope {
    level: [f32; MAX_NUM_SEGMENTS],
    time: [f32; MAX_NUM_SEGMENTS],
    shape: [EnvelopeShape; MAX_NUM_SEGMENTS],

    segment: i32,
    start_value: f32,
    value: f32,
    phase: f32,

    num_segments: usize,
    sustain_point: usize,
    loop_start: usize,
    loop_end: usize,
    hard_reset: bool,
}

impl Default for MultistageEnvelope {
    fn default() -> Self {
        Self::new()
    }
}

impl MultistageEnvelope {
    pub fn new() -> Self {
        let mut e = Self {
            level: [0.0; MAX_NUM_SEGMENTS],
            time: [0.0; MAX_NUM_SEGMENTS],
            shape: [EnvelopeShape::Linear; MAX_NUM_SEGMENTS],
            segment: 0,
            start_value: 0.0,
            value: 0.0,
            phase: 0.0,
            num_segments: 0,
            sustain_point: 0,
            loop_start: 0,
            loop_end: 0,
            hard_reset: false,
        };
        e.init();
        e
    }

    /// `Init`.
    pub fn init(&mut self) {
        self.set_adsr(0.0, 0.25, 0.25, 0.5);
        self.segment = self.num_segments as i32;
        self.phase = 0.0;
        self.start_value = 0.0;
        self.value = 0.0;
        self.hard_reset = false;
    }

    /// `set_adsr`.
    pub fn set_adsr(&mut self, attack: f32, decay: f32, sustain: f32, release: f32) {
        self.num_segments = 3;
        self.sustain_point = 2;

        self.level[0] = 0.0;
        self.level[1] = 1.0;
        self.level[2] = sustain;
        self.level[3] = 0.0;

        self.time[0] = attack;
        self.time[1] = decay;
        self.time[2] = release;

        self.shape[0] = EnvelopeShape::Quartic;
        self.shape[1] = EnvelopeShape::Exponential;
        self.shape[2] = EnvelopeShape::Exponential;

        self.loop_start = 0;
        self.loop_end = 0;
    }

    #[inline]
    pub fn set_hard_reset(&mut self, hard_reset: bool) {
        self.hard_reset = hard_reset;
    }

    /// `Process(flags)` -- advance one control sample and return the level.
    pub fn process(&mut self, flags: u8) -> f32 {
        if flags & FLAG_RISING_EDGE != 0 {
            self.start_value = if self.segment == self.num_segments as i32 || self.hard_reset {
                self.level[0]
            } else {
                self.value
            };
            self.segment = 0;
            self.phase = 0.0;
        } else if flags & FLAG_FALLING_EDGE != 0 && self.sustain_point != 0 {
            self.start_value = self.value;
            self.segment = self.sustain_point as i32;
            self.phase = 0.0;
        } else if self.phase >= 1.0 {
            self.start_value = self.level[self.segment as usize + 1];
            self.segment += 1;
            self.phase = 0.0;
            if self.segment == self.loop_end as i32 {
                self.segment = self.loop_start as i32;
            }
        }

        let done = self.segment == self.num_segments as i32;
        let sustained = self.sustain_point != 0
            && self.segment == self.sustain_point as i32
            && flags & FLAG_GATE != 0;

        let seg = self.segment as usize;
        let mut phase_increment = 0.0;
        if !sustained && !done {
            phase_increment = interpolate8(&LUT_ENV_INCREMENTS, self.time[seg]);
        }
        let t = interpolate8(
            LOOKUP_TABLE_TABLE[LUT_ENV_LINEAR + self.shape[seg] as usize],
            self.phase,
        );
        self.phase += phase_increment;
        self.value = self.start_value + (self.level[seg + 1] - self.start_value) * t;
        self.value
    }
}
