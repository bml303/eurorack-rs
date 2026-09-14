//! `stages/oscillator.h` -- single waveform oscillator, optionally with
//! audio-rate linear FM (through-zero capable, i.e. negative frequencies).
//!
//! Not used by `SegmentGenerator` (which has its own, much smaller
//! `VariableShapeOscillator`) -- in the firmware this class is only
//! instantiated by `stages.cc`'s factory-test tone player (out of scope, see
//! this crate's `PORTING.md`). Ported anyway since it's small, self-contained
//! DSP with no hardware dependency.
//!
//! The C is a function template over `<OscillatorShape, bool
//! has_external_fm, bool through_zero_fm>`; every call site in the firmware
//! ties `has_external_fm == through_zero_fm` (the two public `Render`
//! overloads either pass neither or both), so this collapses to a single
//! `render` taking `external_fm: Option<&[f32]>` and using its presence for
//! both flags.

use crate::resources::LUT_SINE;
use stmlib::fdsp::interpolate;
use stmlib::parameter_interpolator::ParameterInterpolator;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum OscillatorShape {
    #[default]
    ImpulseTrain,
    Saw,
    Sine,
    Triangle,
    Slope,
    Square,
    SquareBright,
    SquareDark,
    SquareTriangle,
}

pub const K_MAX_FREQUENCY: f32 = 0.25;
pub const K_MIN_FREQUENCY: f32 = 0.00001;

#[inline]
fn this_blep_sample(t: f32) -> f32 {
    0.5 * t * t
}

#[inline]
fn next_blep_sample(t: f32) -> f32 {
    let t = 1.0 - t;
    -0.5 * t * t
}

#[inline]
fn next_integrated_blep_sample(t: f32) -> f32 {
    let t1 = 0.5 * t;
    let t2 = t1 * t1;
    let t4 = t2 * t2;
    0.1875 - t1 + 1.5 * t2 - t4
}

#[inline]
fn this_integrated_blep_sample(t: f32) -> f32 {
    next_integrated_blep_sample(1.0 - t)
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Oscillator {
    phase: f32,
    next_sample: f32,
    lp_state: f32,
    hp_state: f32,
    high: bool,

    frequency: f32,
    pw: f32,
}

impl Oscillator {
    pub fn init(&mut self) {
        self.phase = 0.5;
        self.next_sample = 0.0;
        self.lp_state = 1.0;
        self.hp_state = 0.0;
        self.high = true;

        self.frequency = 0.001;
        self.pw = 0.5;
    }

    pub fn render(
        &mut self,
        shape: OscillatorShape,
        mut frequency: f32,
        mut pw: f32,
        external_fm: Option<&[f32]>,
        out: &mut [f32],
    ) {
        let has_external_fm = external_fm.is_some();
        let through_zero_fm = has_external_fm;

        if !has_external_fm {
            if !through_zero_fm {
                frequency = frequency.clamp(K_MIN_FREQUENCY, K_MAX_FREQUENCY);
            } else {
                frequency = frequency.clamp(-K_MAX_FREQUENCY, K_MAX_FREQUENCY);
            }
            pw = pw.clamp(frequency.abs() * 2.0, 1.0 - 2.0 * frequency.abs());
        }

        let size = out.len();
        let mut fm = ParameterInterpolator::new(&mut self.frequency, frequency, size);
        let mut pwm = ParameterInterpolator::new(&mut self.pw, pw, size);

        let mut next_sample = self.next_sample;
        let mut phase = self.phase;
        let mut high = self.high;
        let mut lp_state = self.lp_state;
        let mut hp_state = self.hp_state;

        for (i, o) in out.iter_mut().enumerate() {
            let this_sample_initial = next_sample;
            next_sample = 0.0;

            let mut frequency = fm.next();
            if has_external_fm {
                frequency *= 1.0 + external_fm.unwrap()[i];
                if !through_zero_fm {
                    frequency = frequency.clamp(K_MIN_FREQUENCY, K_MAX_FREQUENCY);
                } else {
                    frequency = frequency.clamp(-K_MAX_FREQUENCY, K_MAX_FREQUENCY);
                }
            }
            let pw = if shape == OscillatorShape::SquareTriangle || shape == OscillatorShape::Triangle {
                0.5
            } else {
                pwm.next()
            };
            let pw = if has_external_fm {
                pw.clamp(frequency.abs() * 2.0, 1.0 - 2.0 * frequency.abs())
            } else {
                pw
            };
            phase += frequency;

            let mut this_sample = this_sample_initial;

            if shape <= OscillatorShape::Saw {
                if phase >= 1.0 {
                    phase -= 1.0;
                    let t = phase / frequency;
                    this_sample -= this_blep_sample(t);
                    next_sample -= next_blep_sample(t);
                } else if through_zero_fm && phase < 0.0 {
                    let t = phase / frequency;
                    phase += 1.0;
                    this_sample += this_blep_sample(t);
                    next_sample += next_blep_sample(t);
                }
                next_sample += phase;

                if shape == OscillatorShape::Saw {
                    *o = 2.0 * this_sample - 1.0;
                } else {
                    lp_state += 0.25 * ((hp_state - this_sample) - lp_state);
                    *o = 4.0 * lp_state;
                    hp_state = this_sample;
                }
            } else if shape == OscillatorShape::Sine {
                if phase >= 1.0 {
                    phase -= 1.0;
                }
                next_sample = interpolate(&LUT_SINE, phase, 1024.0);
                *o = this_sample;
            } else if shape <= OscillatorShape::Slope {
                let mut slope_up = 2.0;
                let mut slope_down = 2.0;
                if shape == OscillatorShape::Slope {
                    slope_up = 1.0 / pw;
                    slope_down = 1.0 / (1.0 - pw);
                }
                if high ^ (phase < pw) {
                    let t = (phase - pw) / frequency;
                    let mut discontinuity = (slope_up + slope_down) * frequency;
                    if through_zero_fm && frequency < 0.0 {
                        discontinuity = -discontinuity;
                    }
                    this_sample -= this_integrated_blep_sample(t) * discontinuity;
                    next_sample -= next_integrated_blep_sample(t) * discontinuity;
                    high = phase < pw;
                }
                if phase >= 1.0 {
                    phase -= 1.0;
                    let t = phase / frequency;
                    let discontinuity = (slope_up + slope_down) * frequency;
                    this_sample += this_integrated_blep_sample(t) * discontinuity;
                    next_sample += next_integrated_blep_sample(t) * discontinuity;
                    high = true;
                } else if through_zero_fm && phase < 0.0 {
                    let t = phase / frequency;
                    phase += 1.0;
                    let discontinuity = (slope_up + slope_down) * frequency;
                    this_sample -= this_integrated_blep_sample(t) * discontinuity;
                    next_sample -= next_integrated_blep_sample(t) * discontinuity;
                    high = false;
                }
                next_sample += if high {
                    phase * slope_up
                } else {
                    1.0 - (phase - pw) * slope_down
                };
                *o = 2.0 * this_sample - 1.0;
            } else {
                if high ^ (phase >= pw) {
                    let t = (phase - pw) / frequency;
                    let mut discontinuity = 1.0;
                    if through_zero_fm && frequency < 0.0 {
                        discontinuity = -discontinuity;
                    }
                    this_sample += this_blep_sample(t) * discontinuity;
                    next_sample += next_blep_sample(t) * discontinuity;
                    high = phase >= pw;
                }
                if phase >= 1.0 {
                    phase -= 1.0;
                    let t = phase / frequency;
                    this_sample -= this_blep_sample(t);
                    next_sample -= next_blep_sample(t);
                    high = false;
                } else if through_zero_fm && phase < 0.0 {
                    let t = phase / frequency;
                    phase += 1.0;
                    this_sample += this_blep_sample(t);
                    next_sample += next_blep_sample(t);
                    high = true;
                }
                next_sample += if phase < pw { 0.0 } else { 1.0 };

                if shape == OscillatorShape::SquareTriangle {
                    let integrator_coefficient = frequency * 0.0625;
                    this_sample = 128.0 * (this_sample - 0.5);
                    lp_state += integrator_coefficient * (this_sample - lp_state);
                    *o = lp_state;
                } else if shape == OscillatorShape::SquareDark {
                    let integrator_coefficient = frequency * 2.0;
                    this_sample = 4.0 * (this_sample - 0.5);
                    lp_state += integrator_coefficient * (this_sample - lp_state);
                    *o = lp_state;
                } else if shape == OscillatorShape::SquareBright {
                    let integrator_coefficient = frequency * 2.0;
                    this_sample = 2.0 * this_sample - 1.0;
                    lp_state += integrator_coefficient * (this_sample - lp_state);
                    *o = (this_sample - lp_state) * 0.5;
                } else {
                    this_sample = 2.0 * this_sample - 1.0;
                    *o = this_sample;
                }
            }
        }
        self.next_sample = next_sample;
        self.phase = phase;
        self.high = high;
        self.lp_state = lp_state;
        self.hp_state = hp_state;
    }
}
