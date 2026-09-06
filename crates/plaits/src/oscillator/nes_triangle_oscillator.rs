//! `plaits/dsp/oscillator/nes_triangle_oscillator.h` -- the NES APU's 16-step
//! (by default; `num_bits` configurable) triangle, cross-fading into a
//! band-limited naive triangle as the frequency rises past what the step
//! resolution can represent without aliasing.
//!
//! `num_bits` is a runtime field here (defaults to 5, i.e. 32 steps) instead
//! of a C++ template parameter.

use stmlib::parameter_interpolator::ParameterInterpolator;
use stmlib::polyblep::{
    next_blep_sample, next_integrated_blep_sample, this_blep_sample, this_integrated_blep_sample,
};

#[derive(Debug, Clone, Copy)]
pub struct NesTriangleOscillator {
    num_bits: u32,
    phase: f32,
    next_sample: f32,
    step: i32,
    ascending: bool,
    frequency: f32,
}

impl Default for NesTriangleOscillator {
    fn default() -> Self {
        Self {
            num_bits: 5,
            phase: 0.0,
            next_sample: 0.0,
            step: 0,
            ascending: true,
            frequency: 0.001,
        }
    }
}

impl NesTriangleOscillator {
    pub fn new(num_bits: u32) -> Self {
        let mut o = Self {
            num_bits,
            ..Default::default()
        };
        o.init();
        o
    }

    pub fn init(&mut self) {
        self.phase = 0.0;
        self.step = 0;
        self.ascending = true;
        self.next_sample = 0.0;
        self.frequency = 0.001;
    }

    pub fn render(&mut self, frequency: f32, out: &mut [f32]) {
        let num_steps = 1i32 << self.num_bits;
        let half = num_steps / 2;
        let top = if num_steps != 2 { num_steps - 1 } else { 2 };
        let num_steps_f = num_steps as f32;
        let scale = if num_steps != 2 {
            4.0 / (top - 1) as f32
        } else {
            2.0
        };

        let frequency = frequency.min(0.25);
        let size = out.len();
        let mut fm = ParameterInterpolator::new(&mut self.frequency, frequency, size);

        let mut next_sample = self.next_sample;
        for o in out.iter_mut() {
            let frequency = fm.next();
            self.phase += frequency;

            // Point at which we cross-fade from the full-resolution NES
            // triangle to a naive band-limited triangle.
            let fade_to_tri = ((frequency - 0.5 / num_steps_f) * 2.0 * num_steps_f).clamp(0.0, 1.0);

            let nes_gain = 1.0 - fade_to_tri;
            let tri_gain = fade_to_tri * 2.0 / scale;

            let mut this_sample = next_sample;
            next_sample = 0.0;

            // Discontinuity at the top of the naive triangle.
            if self.ascending && self.phase >= 0.5 {
                let discontinuity = 4.0 * frequency * tri_gain;
                if discontinuity != 0.0 {
                    let t = (self.phase - 0.5) / frequency;
                    this_sample -= this_integrated_blep_sample(t) * discontinuity;
                    next_sample -= next_integrated_blep_sample(t) * discontinuity;
                }
                self.ascending = false;
            }

            let mut next_step = (self.phase * num_steps_f) as i32;
            if next_step != self.step {
                let mut wrap = false;
                if next_step >= num_steps {
                    self.phase -= 1.0;
                    next_step -= num_steps;
                    wrap = true;
                }

                let mut discontinuity = if next_step < half { 1.0 } else { -1.0 };
                if num_steps == 2 {
                    discontinuity = -discontinuity;
                } else if next_step == 0 || next_step == half {
                    discontinuity = 0.0;
                }

                // Discontinuity at each step of the NES triangle.
                discontinuity *= nes_gain;
                if discontinuity != 0.0 {
                    let frac = self.phase * num_steps_f - next_step as f32;
                    let t = frac / (frequency * num_steps_f);
                    this_sample += this_blep_sample(t) * discontinuity;
                    next_sample += next_blep_sample(t) * discontinuity;
                }

                // Discontinuity at the bottom of the naive triangle.
                if wrap {
                    let discontinuity = 4.0 * frequency * tri_gain;
                    if discontinuity != 0.0 {
                        let t = self.phase / frequency;
                        this_sample += this_integrated_blep_sample(t) * discontinuity;
                        next_sample += next_integrated_blep_sample(t) * discontinuity;
                    }
                    self.ascending = true;
                }
            }
            self.step = next_step;

            // Contribution from the NES triangle.
            next_sample += nes_gain
                * (if self.step < half {
                    self.step
                } else {
                    top - self.step
                }) as f32;

            // Contribution from the naive triangle.
            next_sample += tri_gain
                * (if self.phase < 0.5 {
                    2.0 * self.phase
                } else {
                    2.0 - 2.0 * self.phase
                });

            *o = this_sample * scale - 1.0;
        }
        self.next_sample = next_sample;
    }
}
