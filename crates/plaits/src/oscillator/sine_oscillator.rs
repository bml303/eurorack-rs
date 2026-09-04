//! `plaits/dsp/oscillator/sine_oscillator.h` -- wavetable sine (+ a "magic
//! circle" recurrence fast sine that avoids the table lookup).

use stmlib::fdsp::{interpolate, interpolate_wrap};
use stmlib::parameter_interpolator::ParameterInterpolator;
use stmlib::rsqrt::fast_rsqrt_carmack;

use crate::resources::LUT_SINE;

pub const SINE_LUT_SIZE: f32 = 512.0;
pub const SINE_LUT_BITS: u32 = 9;

/// `Sine(phase)` -- safe for `phase >= 0.0`, wraps.
#[inline]
pub fn sine(phase: f32) -> f32 {
    interpolate_wrap(&LUT_SINE, phase, SINE_LUT_SIZE)
}

/// `SineNoWrap(phase)` -- unsafe if `phase >= 1.25`.
#[inline]
pub fn sine_no_wrap(phase: f32) -> f32 {
    interpolate(&LUT_SINE, phase, SINE_LUT_SIZE)
}

/// `SinePM(phase, pm)` -- phase (`u32` turn, full-scale = one cycle) with
/// positive-or-negative phase modulation up to an index of 32.
#[inline]
pub fn sine_pm(phase: u32, pm: f32) -> f32 {
    const MAX_UINT32: f32 = 4_294_967_296.0;
    const MAX_INDEX: i32 = 32;
    let offset = MAX_INDEX as f32;
    let scale = MAX_UINT32 / (MAX_INDEX as f32 * 2.0);

    let phase = phase.wrapping_add(
        ((pm + offset) * scale) as u32 * (MAX_INDEX as u32 * 2),
    );

    let integral = (phase >> (32 - SINE_LUT_BITS)) as usize;
    let fractional = (phase << SINE_LUT_BITS) as f32 / MAX_UINT32;
    let a = LUT_SINE[integral];
    let b = LUT_SINE[integral + 1];
    a + (b - a) * fractional
}

/// `SineRaw(phase)` -- direct lookup, no interpolation.
#[inline]
pub fn sine_raw(phase: u32) -> f32 {
    LUT_SINE[(phase >> (32 - SINE_LUT_BITS)) as usize]
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SineOscillator {
    phase: f32,
    frequency: f32,
    amplitude: f32,
}

impl SineOscillator {
    pub fn init(&mut self) {
        self.phase = 0.0;
        self.frequency = 0.0;
        self.amplitude = 0.0;
    }

    #[inline]
    pub fn next(&mut self, frequency: f32) -> f32 {
        let frequency = if frequency >= 0.5 { 0.5 } else { frequency };
        self.phase += frequency;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        sine_no_wrap(self.phase)
    }

    #[inline]
    pub fn next_quadrature(&mut self, frequency: f32, amplitude: f32) -> (f32, f32) {
        let frequency = if frequency >= 0.5 { 0.5 } else { frequency };
        self.phase += frequency;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        (
            amplitude * sine_no_wrap(self.phase),
            amplitude * sine_no_wrap(self.phase + 0.25),
        )
    }

    pub fn render_additive(&mut self, frequency: f32, amplitude: f32, out: &mut [f32]) {
        self.render_internal(frequency, amplitude, out, true);
    }

    pub fn render(&mut self, frequency: f32, out: &mut [f32]) {
        self.render_internal(frequency, 1.0, out, false);
    }

    fn render_internal(&mut self, frequency: f32, amplitude: f32, out: &mut [f32], additive: bool) {
        let frequency = if frequency >= 0.5 { 0.5 } else { frequency };
        let mut fm = ParameterInterpolator::new(&mut self.frequency, frequency, out.len());
        let mut am = ParameterInterpolator::new(&mut self.amplitude, amplitude, out.len());

        for o in out.iter_mut() {
            self.phase += fm.next();
            if self.phase >= 1.0 {
                self.phase -= 1.0;
            }
            let s = sine_no_wrap(self.phase);
            if additive {
                *o += am.next() * s;
            } else {
                *o = s;
            }
        }
    }
}

/// `FastSineOscillator` -- the "magic circle" recurrence: rotate a unit vector
/// `(x, y)` by a small angle each sample instead of indexing a table.
#[derive(Debug, Clone, Copy, Default)]
pub struct FastSineOscillator {
    x: f32,
    y: f32,
    epsilon: f32,
    amplitude: f32,
}

impl FastSineOscillator {
    pub fn init(&mut self) {
        self.x = 1.0;
        self.y = 0.0;
        self.epsilon = 0.0;
        self.amplitude = 0.0;
    }

    /// In theory `epsilon = 2 sin(pi f)`; approximated by a 3rd-order
    /// polynomial (avg error 1.13 cents, max 7.33 cents over 16 Hz-16 kHz @ 48 kHz).
    #[inline]
    pub fn fast_2_sin(f: f32) -> f32 {
        let f_pi = f * core::f32::consts::PI;
        f_pi * (2.0 - (2.0 * 0.96 / 6.0) * f_pi * f_pi)
    }

    pub fn render(&mut self, frequency: f32, out: &mut [f32]) {
        self.render_internal(frequency, 1.0, out, None, Mode::Normal);
    }

    pub fn render_additive(&mut self, frequency: f32, amplitude: f32, out: &mut [f32]) {
        self.render_internal(frequency, amplitude, out, None, Mode::Additive);
    }

    pub fn render_quadrature(&mut self, frequency: f32, amplitude: f32, x: &mut [f32], y: &mut [f32]) {
        self.render_internal(frequency, amplitude, x, Some(y), Mode::Quadrature);
    }

    fn render_internal(
        &mut self,
        frequency: f32,
        mut amplitude: f32,
        out: &mut [f32],
        mut out_2: Option<&mut [f32]>,
        mode: Mode,
    ) {
        let frequency = if frequency >= 0.25 {
            amplitude = 0.0;
            0.25
        } else {
            amplitude *= 1.0 - frequency * 4.0;
            frequency
        };

        let size = out.len();
        let mut epsilon = ParameterInterpolator::new(&mut self.epsilon, Self::fast_2_sin(frequency), size);
        let mut am = ParameterInterpolator::new(&mut self.amplitude, amplitude, size);
        let mut x = self.x;
        let mut y = self.y;

        let norm = x * x + y * y;
        if norm <= 0.5 || norm >= 2.0 {
            let scale = fast_rsqrt_carmack(norm);
            x *= scale;
            y *= scale;
        }

        for i in 0..size {
            let e = epsilon.next();
            x += e * y;
            y -= e * x;
            match mode {
                Mode::Additive => out[i] += am.next() * x,
                Mode::Normal => out[i] = x,
                Mode::Quadrature => {
                    let amplitude = am.next();
                    out[i] = x * amplitude;
                    if let Some(o2) = out_2.as_deref_mut() {
                        o2[i] = y * amplitude;
                    }
                }
            }
        }
        self.x = x;
        self.y = y;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Normal,
    Additive,
    Quadrature,
}
