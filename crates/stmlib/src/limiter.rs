//! `stmlib/dsp/limiter.h` -- a soft peak limiter.

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

    pub fn process(&mut self, pre_gain: f32, in_out: &mut [f32]) {
        for s in in_out.iter_mut() {
            let x = *s * pre_gain;
            crate::fdsp::slope(&mut self.peak, x.abs(), 0.05, 0.00002);
            let gain = if self.peak <= 1.0 { 1.0 } else { 1.0 / self.peak };
            *s = x * gain * 0.8;
        }
    }
}
