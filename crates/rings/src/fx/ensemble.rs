//! `rings/dsp/fx/ensemble.h` -- a stereo 3-voice ensemble (two delay lines,
//! six phase-shifted LFO taps mixing a slow and a fast modulator).

use super::fx_engine::{Format16, FxEngine, bases};
use crate::resources::LUT_SINE;

const LENGTHS: [usize; 2] = [2047, 2047];
const BASES: [usize; 2] = bases(LENGTHS);
const LINE_L: usize = 0;
const LINE_R: usize = 1;

#[inline]
fn sine(phi: i32) -> f32 {
    LUT_SINE[(phi & 4095) as usize]
}

/// `rings::Ensemble`.
pub struct Ensemble {
    engine: FxEngine<Format16, 4096>,
    amount: f32,
    depth: f32,
    phase_1: f32,
    phase_2: f32,
}

impl Default for Ensemble {
    fn default() -> Self {
        Self::new()
    }
}

impl Ensemble {
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
        self.depth = depth * 128.0;
    }

    /// `Process(left, right, size)`.
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32], size: usize) {
        for i in 0..size {
            let mut c = self.engine.start();
            let dry_amount = 1.0 - self.amount * 0.5;

            self.phase_1 += 1.57e-05;
            if self.phase_1 >= 1.0 {
                self.phase_1 -= 1.0;
            }
            self.phase_2 += 1.37e-04;
            if self.phase_2 >= 1.0 {
                self.phase_2 -= 1.0;
            }
            let phi_1 = (self.phase_1 * 4096.0) as i32;
            let slow_0 = sine(phi_1);
            let slow_120 = sine(phi_1 + 1365);
            let slow_240 = sine(phi_1 + 2730);
            let phi_2 = (self.phase_2 * 4096.0) as i32;
            let fast_0 = sine(phi_2);
            let fast_120 = sine(phi_2 + 1365);
            let fast_240 = sine(phi_2 + 2730);

            let a = self.depth;
            let b = self.depth * 0.1;

            let mod_1 = slow_0 * a + fast_0 * b;
            let mod_2 = slow_120 * a + fast_120 * b;
            let mod_3 = slow_240 * a + fast_240 * b;

            let mut wet = 0.0f32;

            c.read_scaled(left[i], 1.0);
            c.write_line(BASES[LINE_L], LENGTHS[LINE_L], 0, 0.0);
            c.read_scaled(right[i], 1.0);
            c.write_line(BASES[LINE_R], LENGTHS[LINE_R], 0, 0.0);

            c.interpolate(BASES[LINE_L], mod_1 + 1024.0, 0.33);
            c.interpolate(BASES[LINE_L], mod_2 + 1024.0, 0.33);
            c.interpolate(BASES[LINE_R], mod_3 + 1024.0, 0.33);
            c.write_out_scaled(&mut wet, 0.0);
            left[i] = wet * self.amount + left[i] * dry_amount;

            c.interpolate(BASES[LINE_R], mod_1 + 1024.0, 0.33);
            c.interpolate(BASES[LINE_R], mod_2 + 1024.0, 0.33);
            c.interpolate(BASES[LINE_L], mod_3 + 1024.0, 0.33);
            c.write_out_scaled(&mut wet, 0.0);
            right[i] = wet * self.amount + right[i] * dry_amount;
        }
    }
}
