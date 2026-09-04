//! `plaits/dsp/downsampler/4x_downsampler.h` -- a tiny 4-tap FIR downsampler
//! for 4x-oversampled engines (FM, others).
//!
//! Like [`stmlib::ParameterInterpolator`], the C's `Downsampler` is RAII: its
//! destructor writes the running `head_` back through a borrowed `float*` so
//! the next block picks up where this one left off. Same `Drop`-based port.

use crate::resources::LUT_4X_DOWNSAMPLER_FIR;

pub const OVERSAMPLING: usize = 4;

pub struct Downsampler<'a> {
    head: f32,
    tail: f32,
    state: &'a mut f32,
}

impl<'a> Downsampler<'a> {
    pub fn new(state: &'a mut f32) -> Self {
        let head = *state;
        Self {
            head,
            tail: 0.0,
            state,
        }
    }

    #[inline]
    pub fn accumulate(&mut self, i: usize, sample: f32) {
        self.head += sample * LUT_4X_DOWNSAMPLER_FIR[3 - (i & 3)];
        self.tail += sample * LUT_4X_DOWNSAMPLER_FIR[i & 3];
    }

    #[inline]
    pub fn read(&mut self) -> f32 {
        let value = self.head;
        self.head = self.tail;
        self.tail = 0.0;
        value
    }
}

impl Drop for Downsampler<'_> {
    #[inline]
    fn drop(&mut self) {
        *self.state = self.head;
    }
}
