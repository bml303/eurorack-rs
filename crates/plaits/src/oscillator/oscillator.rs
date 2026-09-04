//! `plaits/dsp/oscillator/oscillator.h` -- single-waveform BLEP oscillator,
//! optionally with audio-rate (through-zero) linear FM.
//!
//! The C selects the waveform and the two FM modes via template parameters,
//! resolved at compile time inside the hot loop. Here `shape` is a runtime
//! enum and `has_external_fm`/`through_zero_fm` are runtime bools; the branch
//! structure (and so the generated samples) is identical, just evaluated per
//! call instead of per instantiation.

use stmlib::parameter_interpolator::ParameterInterpolator;
use stmlib::polyblep::{
    next_blep_sample, next_integrated_blep_sample, this_blep_sample, this_integrated_blep_sample,
};

pub const MAX_FREQUENCY: f32 = 0.25;
pub const MIN_FREQUENCY: f32 = 0.000001;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum OscillatorShape {
    ImpulseTrain = 0,
    Saw,
    Triangle,
    Slope,
    Square,
    SquareBright,
    SquareDark,
    SquareTriangle,
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

    /// `Render<shape>(frequency, pw, out, size)` -- no FM.
    pub fn render(&mut self, shape: OscillatorShape, frequency: f32, pw: f32, out: &mut [f32]) {
        self.render_fm(shape, frequency, pw, None, false, out);
    }

    /// `Render<shape>(frequency, pw, fm, out, size)` -- with linear FM.
    /// `through_zero_fm` allows the frequency to go negative (hard sync-like
    /// tricks); pass `false` for the common "FM depth stays audio-rate but
    /// positive" case.
    pub fn render_fm(
        &mut self,
        shape: OscillatorShape,
        mut frequency: f32,
        mut pw: f32,
        external_fm: Option<&[f32]>,
        through_zero_fm: bool,
        out: &mut [f32],
    ) {
        let has_external_fm = external_fm.is_some();
        let size = out.len();

        if !has_external_fm {
            if !through_zero_fm {
                frequency = frequency.clamp(MIN_FREQUENCY, MAX_FREQUENCY);
            } else {
                frequency = frequency.clamp(-MAX_FREQUENCY, MAX_FREQUENCY);
            }
            pw = pw.clamp(frequency.abs() * 2.0, 1.0 - 2.0 * frequency.abs());
        }

        let mut fm = ParameterInterpolator::new(&mut self.frequency, frequency, size);
        let mut pwm = ParameterInterpolator::new(&mut self.pw, pw, size);
        let mut next_sample = self.next_sample;
        let fm_buf = external_fm.unwrap_or(&[]);

        for (i, o) in out.iter_mut().enumerate() {
            let mut this_sample = next_sample;
            next_sample = 0.0;

            let mut frequency = fm.next();
            if has_external_fm {
                frequency *= 1.0 + fm_buf[i];
                frequency = if !through_zero_fm {
                    frequency.clamp(MIN_FREQUENCY, MAX_FREQUENCY)
                } else {
                    frequency.clamp(-MAX_FREQUENCY, MAX_FREQUENCY)
                };
            }
            let mut pw = if shape == OscillatorShape::SquareTriangle || shape == OscillatorShape::Triangle {
                0.5
            } else {
                pwm.next()
            };
            if has_external_fm {
                pw = pw.clamp(frequency.abs() * 2.0, 1.0 - 2.0 * frequency.abs());
            }
            self.phase += frequency;

            if shape <= OscillatorShape::Saw {
                if self.phase >= 1.0 {
                    self.phase -= 1.0;
                    let t = self.phase / frequency;
                    this_sample -= this_blep_sample(t);
                    next_sample -= next_blep_sample(t);
                } else if through_zero_fm && self.phase < 0.0 {
                    let t = self.phase / frequency;
                    self.phase += 1.0;
                    this_sample += this_blep_sample(t);
                    next_sample += next_blep_sample(t);
                }
                next_sample += self.phase;

                if shape == OscillatorShape::Saw {
                    *o = 2.0 * this_sample - 1.0;
                } else {
                    self.lp_state += 0.25 * ((self.hp_state - this_sample) - self.lp_state);
                    *o = 4.0 * self.lp_state;
                    self.hp_state = this_sample;
                }
            } else if shape <= OscillatorShape::Slope {
                let mut slope_up = 2.0;
                let mut slope_down = 2.0;
                if shape == OscillatorShape::Slope {
                    slope_up = 1.0 / pw;
                    slope_down = 1.0 / (1.0 - pw);
                }
                if self.high ^ (self.phase < pw) {
                    let t = (self.phase - pw) / frequency;
                    let mut discontinuity = (slope_up + slope_down) * frequency;
                    if through_zero_fm && frequency < 0.0 {
                        discontinuity = -discontinuity;
                    }
                    this_sample -= this_integrated_blep_sample(t) * discontinuity;
                    next_sample -= next_integrated_blep_sample(t) * discontinuity;
                    self.high = self.phase < pw;
                }
                if self.phase >= 1.0 {
                    self.phase -= 1.0;
                    let t = self.phase / frequency;
                    let discontinuity = (slope_up + slope_down) * frequency;
                    this_sample += this_integrated_blep_sample(t) * discontinuity;
                    next_sample += next_integrated_blep_sample(t) * discontinuity;
                    self.high = true;
                } else if through_zero_fm && self.phase < 0.0 {
                    let t = self.phase / frequency;
                    self.phase += 1.0;
                    let discontinuity = (slope_up + slope_down) * frequency;
                    this_sample -= this_integrated_blep_sample(t) * discontinuity;
                    next_sample -= next_integrated_blep_sample(t) * discontinuity;
                    self.high = false;
                }
                next_sample += if self.high {
                    self.phase * slope_up
                } else {
                    1.0 - (self.phase - pw) * slope_down
                };
                *o = 2.0 * this_sample - 1.0;
            } else {
                if self.high ^ (self.phase >= pw) {
                    let t = (self.phase - pw) / frequency;
                    let mut discontinuity = 1.0;
                    if through_zero_fm && frequency < 0.0 {
                        discontinuity = -discontinuity;
                    }
                    this_sample += this_blep_sample(t) * discontinuity;
                    next_sample += next_blep_sample(t) * discontinuity;
                    self.high = self.phase >= pw;
                }
                if self.phase >= 1.0 {
                    self.phase -= 1.0;
                    let t = self.phase / frequency;
                    this_sample -= this_blep_sample(t);
                    next_sample -= next_blep_sample(t);
                    self.high = false;
                } else if through_zero_fm && self.phase < 0.0 {
                    let t = self.phase / frequency;
                    self.phase += 1.0;
                    this_sample += this_blep_sample(t);
                    next_sample += next_blep_sample(t);
                    self.high = true;
                }
                next_sample += if self.phase < pw { 0.0 } else { 1.0 };

                match shape {
                    OscillatorShape::SquareTriangle => {
                        let integrator_coefficient = frequency * 0.0625;
                        this_sample = 128.0 * (this_sample - 0.5);
                        self.lp_state += integrator_coefficient * (this_sample - self.lp_state);
                        *o = self.lp_state;
                    }
                    OscillatorShape::SquareDark => {
                        let integrator_coefficient = frequency * 2.0;
                        this_sample = 4.0 * (this_sample - 0.5);
                        self.lp_state += integrator_coefficient * (this_sample - self.lp_state);
                        *o = self.lp_state;
                    }
                    OscillatorShape::SquareBright => {
                        let integrator_coefficient = frequency * 2.0;
                        this_sample = 2.0 * this_sample - 1.0;
                        self.lp_state += integrator_coefficient * (this_sample - self.lp_state);
                        *o = (this_sample - self.lp_state) * 0.5;
                    }
                    _ => {
                        this_sample = 2.0 * this_sample - 1.0;
                        *o = this_sample;
                    }
                }
            }
        }
        self.next_sample = next_sample;
    }
}
