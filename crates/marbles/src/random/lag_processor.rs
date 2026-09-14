//! `marbles/random/lag_processor.h` -- lag processor for the STEPS control.

use stmlib::constrain;
use stmlib::fdsp::{crossfade, one_pole};
use stmlib::units::semitones_to_ratio;

use crate::resources::LUT_RAISED_COSINE;

/// `stmlib::Interpolate` with the C's "read one past a `size + 1` table,
/// multiplied by a zero fraction" clamped in bounds (as in `mi-rings` /
/// `mi-elements`). `phase` can legitimately reach exactly `1.0` here (a
/// `SlaveRamp` in Bernoulli mode clamps its output phase to `1.0` while
/// holding a gate), which would otherwise index one past the end of
/// `LUT_RAISED_COSINE`.
#[inline]
fn interpolate(table: &[f32], index: f32, size: f32) -> f32 {
    let scaled = index.clamp(0.0, 1.0) * size;
    let last = table.len() - 1;
    let integral = (scaled as usize).min(last);
    let fractional = scaled - integral as f32;
    let a = table[integral];
    let b = table[(integral + 1).min(last)];
    a + (b - a) * fractional
}

#[derive(Debug, Clone, Copy, Default)]
pub struct LagProcessor {
    ramp_start: f32,
    ramp_value: f32,
    lp_state: f32,
    previous_phase: f32,
}

impl LagProcessor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn init(&mut self) {
        self.ramp_start = 0.0;
        self.ramp_value = 0.0;
        self.lp_state = 0.0;
        self.previous_phase = 0.0;
    }

    pub fn reset_ramp(&mut self) {
        self.ramp_start = self.ramp_value;
    }

    pub fn process(&mut self, value: f32, smoothness: f32, phase: f32) -> f32 {
        let mut frequency = phase - self.previous_phase;
        if frequency < 0.0 {
            frequency += 1.0;
        }
        self.previous_phase = phase;

        // The frequency of the portamento/glide LP filter follows an
        // exponential scale, with a minimum frequency corresponding to half
        // the clock pulse frequency (giving a roughly linear glide), and a
        // maximum value 7 octaves above.
        //
        // When smoothness approaches 0, the response curve is tweaked to
        // give immediate voltage changes, without any lag.
        frequency *= 0.25;
        frequency *= semitones_to_ratio(84.0 * (1.0 - smoothness));
        if frequency >= 1.0 {
            frequency = 1.0;
        }
        if smoothness <= 0.05 {
            frequency += 20.0 * (0.05 - smoothness) * (1.0 - frequency);
        }

        one_pole(&mut self.lp_state, value, frequency);

        // The final output is a crossfade between a variable shape
        // interpolation and the low-pass glide/lag.
        let interp_amount = constrain((smoothness - 0.6) * 5.0, 0.0, 1.0);

        let interp_linearity = constrain((1.0 - smoothness) * 5.0, 0.0, 1.0);
        let warped_phase = interpolate(&LUT_RAISED_COSINE, phase, 256.0);

        let interp_phase = crossfade(warped_phase, phase, interp_linearity);
        let interp = crossfade(self.ramp_start, value, interp_phase);
        self.ramp_value = interp;

        crossfade(self.lp_state, interp, interp_amount)
    }
}
