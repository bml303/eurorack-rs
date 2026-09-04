//! `plaits/dsp/noise/smooth_random_generator.h` -- smoothstep-interpolated
//! random walk, used for the engines' internal slow modulations.

use stmlib::Random;

#[derive(Debug, Clone, Copy, Default)]
pub struct SmoothRandomGenerator {
    phase: f32,
    from: f32,
    interval: f32,
}

impl SmoothRandomGenerator {
    pub fn init(&mut self) {
        *self = Self::default();
    }

    pub fn render(&mut self, frequency: f32) -> f32 {
        self.phase += frequency;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
            self.from += self.interval;
            self.interval = Random::get_float() * 2.0 - 1.0 - self.from;
        }
        let t = self.phase * self.phase * (3.0 - 2.0 * self.phase);
        self.from + self.interval * t
    }
}
