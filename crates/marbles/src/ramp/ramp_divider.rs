//! `marbles/ramp/ramp_divider.h` -- generates a ramp whose frequency is p/q
//! times the frequency of the input. Phase is synchronized.

use super::MAX_RAMP_VALUE;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Ratio {
    pub p: i32,
    pub q: i32,
}

impl Ratio {
    pub const fn new(p: i32, q: i32) -> Self {
        Self { p, q }
    }

    pub fn to_float(self) -> f32 {
        self.p as f32 / self.q as f32
    }

    /// `Ratio::Simplify<n>()` -- divide both terms by `n` while both remain
    /// exactly divisible by it.
    pub fn simplify(&mut self, n: i32) {
        while self.p % n == 0 && self.q % n == 0 {
            self.p /= n;
            self.q /= n;
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct RampDivider {
    phase: f32,
    train_phase: f32,
    max_train_phase: f32,
    f_ratio: f32,
    reset_counter: i32,
    reset_at_next_pulse: bool,
}

impl RampDivider {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn init(&mut self) {
        self.phase = 0.0;
        self.train_phase = 0.0;
        self.max_train_phase = 1.0;
        self.f_ratio = 0.99999;
        self.reset_counter = 1;
        self.reset_at_next_pulse = false;
    }

    pub fn reset(&mut self) {
        self.reset_at_next_pulse = true;
    }

    pub fn process(&mut self, ratio: Ratio, input: &[f32], out: &mut [f32]) {
        for (in_sample, out_sample) in input.iter().zip(out.iter_mut()) {
            let new_phase = *in_sample;
            let mut frequency = new_phase - self.phase;
            if frequency < 0.0 {
                if self.reset_at_next_pulse {
                    self.reset_at_next_pulse = false;
                    self.reset_counter = 1;
                }
                frequency += 1.0;
                self.reset_counter -= 1;
                if self.reset_counter == 0 {
                    self.train_phase = new_phase;
                    self.reset_counter = ratio.q;
                    self.f_ratio = ratio.to_float() * MAX_RAMP_VALUE;
                    frequency = 0.0;
                    self.max_train_phase = ratio.q as f32;
                }
            }

            self.train_phase += frequency;
            if self.train_phase >= self.max_train_phase {
                self.train_phase = self.max_train_phase;
            }

            let mut output_phase = self.train_phase * self.f_ratio;
            output_phase -= output_phase as i32 as f32;
            *out_sample = output_phase;
            self.phase = new_phase;
        }
    }
}
