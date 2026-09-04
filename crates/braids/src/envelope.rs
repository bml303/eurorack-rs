//! `braids/envelope.h` -- a two-segment (AD) exponential envelope.

use stmlib::fixed::{interpolate_824_u16, mix_u16};

use crate::resources::{LUT_ENV_EXPO, LUT_ENV_PORTAMENTO_INCREMENTS};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvelopeSegment {
    Attack = 0,
    Decay = 1,
    Dead = 2,
}

const NUM_SEGMENTS: usize = 3;

#[derive(Debug, Clone)]
pub struct Envelope {
    increment: [u32; NUM_SEGMENTS],
    target: [u16; NUM_SEGMENTS],
    segment: usize,
    a: u16,
    b: u16,
    value: u16,
    phase: u32,
}

impl Default for Envelope {
    fn default() -> Self {
        Self::new()
    }
}

impl Envelope {
    pub fn new() -> Self {
        let mut e = Self {
            increment: [0; NUM_SEGMENTS],
            target: [0; NUM_SEGMENTS],
            segment: EnvelopeSegment::Dead as usize,
            a: 0,
            b: 0,
            value: 0,
            phase: 0,
        };
        e.init();
        e
    }

    pub fn init(&mut self) {
        self.target[EnvelopeSegment::Attack as usize] = 65535;
        self.target[EnvelopeSegment::Decay as usize] = 0;
        self.target[EnvelopeSegment::Dead as usize] = 0;
        self.increment[EnvelopeSegment::Dead as usize] = 0;
    }

    #[inline]
    pub fn segment(&self) -> EnvelopeSegment {
        match self.segment {
            0 => EnvelopeSegment::Attack,
            1 => EnvelopeSegment::Decay,
            _ => EnvelopeSegment::Dead,
        }
    }

    #[inline]
    pub fn update(&mut self, a: usize, d: usize) {
        self.increment[EnvelopeSegment::Attack as usize] = LUT_ENV_PORTAMENTO_INCREMENTS[a];
        self.increment[EnvelopeSegment::Decay as usize] = LUT_ENV_PORTAMENTO_INCREMENTS[d];
    }

    #[inline]
    pub fn trigger(&mut self, segment: EnvelopeSegment) {
        if segment == EnvelopeSegment::Dead {
            self.value = 0;
        }
        self.a = self.value;
        self.b = self.target[segment as usize];
        self.segment = segment as usize;
        self.phase = 0;
    }

    #[inline]
    pub fn render(&mut self) -> u16 {
        let increment = self.increment[self.segment];
        self.phase = self.phase.wrapping_add(increment);
        if self.phase < increment {
            self.value = mix_u16(self.a, self.b, 65535);
            let next = (self.segment + 1).min(EnvelopeSegment::Dead as usize);
            let next_seg = match next {
                0 => EnvelopeSegment::Attack,
                1 => EnvelopeSegment::Decay,
                _ => EnvelopeSegment::Dead,
            };
            self.trigger(next_seg);
        }
        if self.increment[self.segment] != 0 {
            self.value = mix_u16(
                self.a,
                self.b,
                interpolate_824_u16(&LUT_ENV_EXPO, self.phase),
            );
        }
        self.value
    }

    #[inline]
    pub fn value(&self) -> u16 {
        self.value
    }
}
