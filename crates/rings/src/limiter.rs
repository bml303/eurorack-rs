//! `rings/dsp/limiter.h` -- a stereo peak limiter clamping to ~8 Vpp and
//! soft-clipping towards 10 Vpp.

use stmlib::fdsp::{slope, soft_limit};

/// `rings::Limiter`.
#[derive(Debug, Clone, Copy)]
pub struct Limiter {
    peak: f32,
}

impl Default for Limiter {
    fn default() -> Self {
        Self::new()
    }
}

impl Limiter {
    pub fn new() -> Self {
        Self { peak: 0.5 }
    }

    /// `Init`.
    pub fn init(&mut self) {
        self.peak = 0.5;
    }

    /// `Process(l, r, size, pre_gain)`.
    pub fn process(&mut self, l: &mut [f32], r: &mut [f32], size: usize, pre_gain: f32) {
        for i in 0..size {
            let l_pre = l[i] * pre_gain;
            let r_pre = r[i] * pre_gain;

            let l_peak = l_pre.abs();
            let r_peak = r_pre.abs();
            let s_peak = (r_pre - l_pre).abs();

            let peak = l_peak.max(r_peak).max(s_peak);
            slope(&mut self.peak, peak, 0.05, 0.00002);

            let gain = if self.peak <= 1.0 {
                1.0
            } else {
                1.0 / self.peak
            };
            l[i] = soft_limit(l_pre * gain * 0.8);
            r[i] = soft_limit(r_pre * gain * 0.8);
        }
    }
}
