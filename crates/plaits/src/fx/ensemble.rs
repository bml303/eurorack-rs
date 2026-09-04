//! `plaits/dsp/fx/ensemble.h` -- a 3-tap chorus/ensemble effect (2 slow + fast
//! sine LFOs at 3 phases feeding fractional delay taps that cross-feed L/R).

use super::fx_engine::{FxBuffer, Tap};
use crate::oscillator::sine_raw;

const SIZE: usize = 1024;

const LINE_L: Tap = Tap { base: 0, length: 511 };
const LINE_R: Tap = Tap { base: 512, length: 511 };

const ONE_THIRD: u32 = 1_417_339_207;
const TWO_THIRD: u32 = 2_834_678_415;

pub struct Ensemble {
    engine: FxBuffer<SIZE>,
    amount: f32,
    depth: f32,
    phase_1: u32,
    phase_2: u32,
}

impl Default for Ensemble {
    fn default() -> Self {
        Self {
            engine: FxBuffer::default(),
            amount: 0.0,
            depth: 0.0,
            phase_1: 0,
            phase_2: 0,
        }
    }
}

impl Ensemble {
    pub fn init(&mut self) {
        self.phase_1 = 0;
        self.phase_2 = 0;
    }

    pub fn reset(&mut self) {
        self.engine.clear();
    }

    pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        for i in 0..left.len() {
            let mut c = self.engine.start();
            let dry_amount = 1.0 - self.amount * 0.5;

            self.phase_1 = self.phase_1.wrapping_add(67_289); // 0.75 Hz
            self.phase_2 = self.phase_2.wrapping_add(589_980); // 6.57 Hz
            let slow_0 = sine_raw(self.phase_1);
            let slow_120 = sine_raw(self.phase_1.wrapping_add(ONE_THIRD));
            let slow_240 = sine_raw(self.phase_1.wrapping_add(TWO_THIRD));
            let fast_0 = sine_raw(self.phase_2);
            let fast_120 = sine_raw(self.phase_2.wrapping_add(ONE_THIRD));
            let fast_240 = sine_raw(self.phase_2.wrapping_add(TWO_THIRD));

            // Max deviation: 176.
            let a = self.depth * 160.0;
            let b = self.depth * 16.0;

            let mod_1 = slow_0 * a + fast_0 * b;
            let mod_2 = slow_120 * a + fast_120 * b;
            let mod_3 = slow_240 * a + fast_240 * b;

            let mut wet = 0.0f32;

            // Sum L & R into the chorus lines.
            c.read_value(left[i], 1.0);
            c.write(LINE_L, 0, 0.0);
            c.read_value(right[i], 1.0);
            c.write(LINE_R, 0, 0.0);

            c.interpolate(LINE_L, mod_1 + 192.0, 0.33);
            c.interpolate(LINE_L, mod_2 + 192.0, 0.33);
            c.interpolate(LINE_R, mod_3 + 192.0, 0.33);
            c.write_out(&mut wet, 0.0);
            left[i] = wet * self.amount + left[i] * dry_amount;

            c.interpolate(LINE_R, mod_1 + 192.0, 0.33);
            c.interpolate(LINE_R, mod_2 + 192.0, 0.33);
            c.interpolate(LINE_L, mod_3 + 192.0, 0.33);
            c.write_out(&mut wet, 0.0);
            right[i] = wet * self.amount + right[i] * dry_amount;
        }
    }

    #[inline]
    pub fn set_amount(&mut self, amount: f32) {
        self.amount = amount;
    }

    #[inline]
    pub fn set_depth(&mut self, depth: f32) {
        self.depth = depth;
    }
}
