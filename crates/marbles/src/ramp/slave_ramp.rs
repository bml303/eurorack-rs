//! `marbles/ramp/slave_ramp.h` -- a ramp that follows a master ramp through
//! division/multiplication, or free-runs as a Bernoulli-gate pulse.

use super::{MAX_RAMP_VALUE, Ratio};

#[derive(Debug, Clone, Default)]
pub struct SlaveRamp {
    phase: f32,
    max_phase: f32,
    ratio: f32,
    pulse_width: f32,
    target: f32,
    pulse_length: i32,

    bernoulli: bool,
    must_complete: bool,
}

impl SlaveRamp {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn init(&mut self) {
        self.phase = 0.0;
        self.max_phase = MAX_RAMP_VALUE;
        self.ratio = 1.0;
        self.pulse_width = 0.0;
        self.target = 1.0;
        self.pulse_length = 0;
        self.bernoulli = false;
        self.must_complete = false;
    }

    pub fn reset(&mut self) {
        self.init();
        self.phase = 1.0;
    }

    /// Initialize with a multiplied/divided rate compared to the master.
    pub fn init_divided(&mut self, pattern_length: i32, ratio: Ratio, pulse_width: f32) {
        self.bernoulli = false;

        self.phase = 0.0;
        self.max_phase = pattern_length as f32 * MAX_RAMP_VALUE;
        self.ratio = ratio.to_float();
        self.pulse_width = pulse_width;
        self.target = 1.0;
        self.pulse_length = 0;
    }

    /// Initialize with an adaptive slope: divide the frequency by 2 every
    /// time we know we won't have to reach 1.0 at the next tick.
    pub fn init_bernoulli(&mut self, must_complete: bool, pulse_width: f32, expected_value: f32) {
        self.bernoulli = true;

        if self.must_complete {
            self.phase = 0.0;
            self.pulse_width = pulse_width;
            self.ratio = 1.0;
            self.pulse_length = 0;
        }

        if !must_complete {
            self.ratio = (1.0 - self.phase) * expected_value;
        } else {
            self.ratio = 1.0 - self.phase;
        }
        self.must_complete = must_complete;
    }

    pub fn process(&mut self, frequency: f32) -> (f32, bool) {
        let output_phase = if self.bernoulli {
            self.phase += frequency * self.ratio;
            if self.phase >= 1.0 { 1.0 } else { self.phase }
        } else {
            self.phase += frequency;
            if self.phase >= self.max_phase {
                self.phase = self.max_phase;
            }
            let mut phase = self.phase * self.ratio;
            if phase > self.target {
                self.pulse_length = 0;
                self.target += 1.0;
            }
            phase -= phase as i32 as f32;
            phase
        };
        let gate = if self.pulse_width == 0.0 {
            self.pulse_length < 32 && output_phase <= 0.5
        } else {
            output_phase < self.pulse_width
        };
        self.pulse_length += 1;
        (output_phase, gate)
    }
}
