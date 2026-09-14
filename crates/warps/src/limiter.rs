//! `warps/dsp/limiter.h`.

use stmlib::fdsp::{slope, soft_limit};

#[derive(Debug, Clone, Copy)]
pub struct Limiter {
    peak: f32,
}

impl Default for Limiter {
    fn default() -> Self {
        Self { peak: 0.5 }
    }
}

impl Limiter {
    pub fn init(&mut self) {
        self.peak = 0.5;
    }

    pub fn process(&mut self, in_out: &mut [f32], pre_gain: f32) {
        for s in in_out.iter_mut() {
            let x = *s * pre_gain;
            slope(&mut self.peak, x.abs(), 0.05, 0.00002);
            let gain = if self.peak <= 1.0 { 1.0 } else { 1.0 / self.peak };
            *s = soft_limit(x * gain * 0.8);
        }
    }
}
