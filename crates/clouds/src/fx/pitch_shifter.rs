//! `clouds/dsp/fx/pitch_shifter.h` -- a crossfading two-tap delay pitch
//! shifter (used only by the looping-delay mode).

use stmlib::fdsp::one_pole;

use crate::frame::FloatFrame;

use super::fx_engine::{bases, Format16, FxEngine};

const LENGTHS: [usize; 2] = [2047, 2047];
const BASES: [usize; 2] = bases(LENGTHS);
const LEFT: usize = 0;
const RIGHT: usize = 1;

/// `PitchShifter`.
pub struct PitchShifter {
    engine: FxEngine<Format16, 4096>,
    phase: f32,
    ratio: f32,
    size: f32,
}

impl PitchShifter {
    pub fn new() -> Self {
        Self {
            engine: FxEngine::new(),
            phase: 0.0,
            ratio: 0.0,
            size: 2047.0,
        }
    }

    /// `Init`.
    pub fn init(&mut self) {
        self.engine.clear();
        self.phase = 0.0;
        self.size = 2047.0;
    }

    /// `Clear`.
    pub fn clear(&mut self) {
        self.engine.clear();
    }

    #[inline]
    pub fn set_ratio(&mut self, ratio: f32) {
        self.ratio = ratio;
    }

    /// `set_size` -- one-pole smoothed toward `128 + (2047 - 128) * size^3`.
    #[inline]
    pub fn set_size(&mut self, size: f32) {
        let target_size = 128.0 + (2047.0 - 128.0) * size * size * size;
        one_pole(&mut self.size, target_size, 0.05);
    }

    /// `Process(FloatFrame*)` -- one frame.
    pub fn process_frame(&mut self, frame: &mut FloatFrame) {
        let mut c = self.engine.start();

        self.phase += (1.0 - self.ratio) / self.size;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        if self.phase <= 0.0 {
            self.phase += 1.0;
        }
        let tri = 2.0 * if self.phase >= 0.5 { 1.0 - self.phase } else { self.phase };
        let phase = self.phase * self.size;
        let mut half = phase + self.size * 0.5;
        if half >= self.size {
            half -= self.size;
        }

        c.read_scaled(frame.l, 1.0);
        c.write_line(BASES[LEFT], LENGTHS[LEFT], 0, 0.0);
        c.interpolate(BASES[LEFT], phase, tri);
        c.interpolate(BASES[LEFT], half, 1.0 - tri);
        c.write_out_scaled(&mut frame.l, 0.0);

        c.read_scaled(frame.r, 1.0);
        c.write_line(BASES[RIGHT], LENGTHS[RIGHT], 0, 0.0);
        c.interpolate(BASES[RIGHT], phase, tri);
        c.interpolate(BASES[RIGHT], half, 1.0 - tri);
        c.write_out_scaled(&mut frame.r, 0.0);
    }

    /// `Process(FloatFrame*, size)`.
    pub fn process(&mut self, in_out: &mut [FloatFrame]) {
        for frame in in_out.iter_mut() {
            self.process_frame(frame);
        }
    }
}

impl Default for PitchShifter {
    fn default() -> Self {
        Self::new()
    }
}
