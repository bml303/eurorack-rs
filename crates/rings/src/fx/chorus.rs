//! `rings/dsp/fx/chorus.h` -- a 2-tap modulated-delay chorus (one delay line,
//! two quadrature LFOs).

use super::fx_engine::{Format16, FxEngine, bases};
use crate::resources::LUT_SINE;

const LENGTHS: [usize; 1] = [2047];
const BASES: [usize; 1] = bases(LENGTHS);

/// `stmlib::Interpolate(lut_sine, x, 4096.0)` -- `lut_sine` is 1.25 periods
/// (5121 entries), so `x` runs to 1.25 here; clamp `i + 1` in bounds to match
/// the C's benign one-past-the-end read.
#[inline]
fn interp_sine(index: f32) -> f32 {
    let scaled = index * 4096.0;
    let i = scaled as usize;
    let f = scaled - i as f32;
    let a = LUT_SINE[i];
    let b = LUT_SINE[(i + 1).min(LUT_SINE.len() - 1)];
    a + (b - a) * f
}

/// `rings::Chorus`.
pub struct Chorus {
    engine: FxEngine<Format16, 2048>,
    amount: f32,
    depth: f32,
    phase_1: f32,
    phase_2: f32,
}

impl Default for Chorus {
    fn default() -> Self {
        Self::new()
    }
}

impl Chorus {
    pub fn new() -> Self {
        Self {
            engine: FxEngine::new(),
            amount: 0.0,
            depth: 0.0,
            phase_1: 0.0,
            phase_2: 0.0,
        }
    }

    /// `Init`.
    pub fn init(&mut self) {
        self.engine.clear();
        self.phase_1 = 0.0;
        self.phase_2 = 0.0;
    }

    #[inline]
    pub fn set_amount(&mut self, amount: f32) {
        self.amount = amount;
    }
    #[inline]
    pub fn set_depth(&mut self, depth: f32) {
        self.depth = depth * 384.0;
    }

    /// `Process(left, right, size)`.
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32], size: usize) {
        for i in 0..size {
            let mut c = self.engine.start();
            let dry_amount = 1.0 - self.amount * 0.5;

            self.phase_1 += 4.17e-06;
            if self.phase_1 >= 1.0 {
                self.phase_1 -= 1.0;
            }
            self.phase_2 += 5.417e-06;
            if self.phase_2 >= 1.0 {
                self.phase_2 -= 1.0;
            }
            let sin_1 = interp_sine(self.phase_1);
            let cos_1 = interp_sine(self.phase_1 + 0.25);
            let sin_2 = interp_sine(self.phase_2);
            let cos_2 = interp_sine(self.phase_2 + 0.25);

            let mut wet = 0.0f32;

            c.read_scaled(left[i], 0.5);
            c.read_scaled(right[i], 0.5);
            c.write_line(BASES[0], LENGTHS[0], 0, 0.0);

            c.interpolate(BASES[0], sin_1 * self.depth + 1200.0, 0.5);
            c.interpolate(BASES[0], sin_2 * self.depth + 800.0, 0.5);
            c.write_out_scaled(&mut wet, 0.0);
            left[i] = wet * self.amount + left[i] * dry_amount;

            c.interpolate(BASES[0], cos_1 * self.depth + 800.0, 0.5);
            c.interpolate(BASES[0], cos_2 * self.depth + 1200.0, 0.5);
            c.write_out_scaled(&mut wet, 0.0);
            right[i] = wet * self.amount + right[i] * dry_amount;
        }
    }
}
