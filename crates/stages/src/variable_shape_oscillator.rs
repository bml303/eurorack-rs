//! `stages/variable_shape_oscillator.h` -- continuously variable waveform:
//! triangle > saw > square, both with variable slope / pulse-width. "Taken
//! from Plaits and simplified to remove all unused sync stuff + remove
//! interpolation of PW and shape modulation" (the C's own comment) -- so this
//! is deliberately a different, smaller class from
//! `plaits::VariableShapeOscillator` / `mi-plaits`'s port of it, not a
//! duplicate to be shared.

use stmlib::polyblep::{
    next_blep_sample, next_integrated_blep_sample, this_blep_sample, this_integrated_blep_sample,
};

/// `plaits::kMaxFrequency` (0.25) -- the one constant this file borrows from
/// Plaits' `oscillator.h`.
const K_MAX_FREQUENCY: f32 = 0.25;

#[derive(Debug, Clone, Copy, Default)]
pub struct VariableShapeOscillator {
    phase: f32,
    next_sample: f32,
    high: bool,
}

impl VariableShapeOscillator {
    pub fn init(&mut self) {
        self.phase = 0.0;
        self.next_sample = 0.0;
        self.high = false;
    }

    /// `Render(frequency, macro, out, size)` -- derives `pw`/`waveshape` from
    /// a single macro control.
    pub fn render_macro(&mut self, frequency: f32, macro_: f32, out: &mut [f32]) {
        let shape = (macro_ * 1.5).clamp(0.0, 1.0);
        let pw = (0.5 + (macro_ - 0.66) * 1.46).clamp(0.5, 0.995);
        self.render(frequency, pw, shape, out);
    }

    pub fn render(&mut self, frequency: f32, pw: f32, waveshape: f32, out: &mut [f32]) {
        let frequency = frequency.min(K_MAX_FREQUENCY);

        let pw = if frequency >= 0.25 {
            0.5
        } else {
            pw.clamp(frequency * 2.0, 1.0 - 2.0 * frequency)
        };

        let mut next_sample = self.next_sample;

        let square_amount = (waveshape - 0.5).max(0.0) * 2.0;
        let triangle_amount = (1.0 - waveshape * 2.0).max(0.0);
        let slope_up = 1.0 / pw;
        let slope_down = 1.0 / (1.0 - pw);

        for o in out.iter_mut() {
            let mut this_sample = next_sample;
            next_sample = 0.0;
            self.phase += frequency;
            if !self.high && self.phase >= pw {
                let t = (self.phase - pw) / frequency;
                let mut triangle_step = (slope_up + slope_down) * frequency;
                triangle_step *= triangle_amount;
                this_sample += square_amount * this_blep_sample(t);
                next_sample += square_amount * next_blep_sample(t);
                this_sample -= triangle_step * this_integrated_blep_sample(t);
                next_sample -= triangle_step * next_integrated_blep_sample(t);
                self.high = true;
            } else if self.phase >= 1.0 {
                self.phase -= 1.0;
                let t = self.phase / frequency;
                let mut triangle_step = (slope_up + slope_down) * frequency;
                triangle_step *= triangle_amount;

                this_sample -= (1.0 - triangle_amount) * this_blep_sample(t);
                next_sample -= (1.0 - triangle_amount) * next_blep_sample(t);
                this_sample += triangle_step * this_integrated_blep_sample(t);
                next_sample += triangle_step * next_integrated_blep_sample(t);
                self.high = false;
            }

            next_sample += Self::compute_naive_sample(
                self.phase,
                pw,
                slope_up,
                slope_down,
                triangle_amount,
                square_amount,
            );
            *o = 2.0 * this_sample - 1.0;
        }
        self.next_sample = next_sample;
    }

    #[inline]
    fn compute_naive_sample(
        phase: f32,
        pw: f32,
        slope_up: f32,
        slope_down: f32,
        triangle_amount: f32,
        square_amount: f32,
    ) -> f32 {
        let mut saw = phase;
        let square = if phase < pw { 0.0 } else { 1.0 };
        let triangle = if phase < pw {
            phase * slope_up
        } else {
            1.0 - (phase - pw) * slope_down
        };
        saw += (square - saw) * square_amount;
        saw += (triangle - saw) * triangle_amount;
        saw
    }
}
