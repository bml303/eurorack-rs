//! `warps/dsp/oscillator.{h,cc}` -- the carrier/vocoder-source oscillator:
//! a wavetable-interpolated sine (with through-zero-ish phase FM), a
//! polyBLEP triangle/saw/pulse, and filtered/ducked noise.
//!
//! The C dispatches shape via a `RenderFn fn_table_[]` of member-function
//! pointers (`RenderPolyblep<shape>` instantiated 3x by template, plus
//! `RenderSine`/`RenderNoise`); ported as a `match` on [`OscillatorShape`]
//! in [`Oscillator::render`] instead, calling one shared `render_polyblep`.
//! The C's `float* modulation` parameter is never written through in any of
//! the three render functions -- only ever read -- so it's ported as `&[f32]`.

use stmlib::fdsp::clip16;
use stmlib::filter::{FilterMode, FrequencyApproximation, Svf};
use stmlib::parameter_interpolator::ParameterInterpolator;
use stmlib::random::Random;

use crate::parameters::OscillatorShape;
use crate::resources::{LUT_MIDI_TO_F_HIGH, LUT_MIDI_TO_F_LOW, LUT_SIN};

const K_TO_FLOAT: f32 = 1.0 / 4294967296.0;
const K_TO_UINT32: f32 = 4294967296.0;

#[inline]
fn this_blep_sample(t: f32) -> f32 {
    0.5 * t * t
}

#[inline]
fn next_blep_sample(t: f32) -> f32 {
    let t = 1.0 - t;
    -0.5 * t * t
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Oscillator {
    high: bool,
    phase: f32,
    phase_increment: f32,
    next_sample: f32,
    lp_state: f32,
    hp_state: f32,

    external_input_level: f32,

    filter: Svf,
}

impl Oscillator {
    pub fn init(&mut self, sample_rate: f32) {
        self.next_sample = 0.0;
        self.phase = 0.0;
        self.phase_increment = 100.0 / sample_rate;
        self.hp_state = 0.0;
        self.lp_state = 0.0;

        self.high = false;

        self.external_input_level = 0.0;

        self.filter.init();
    }

    #[inline]
    pub fn midi_to_increment(&self, midi_pitch: f32) -> f32 {
        let pitch = (midi_pitch * 256.0) as i32;
        let pitch = 32768 + clip16(pitch - 20480);
        LUT_MIDI_TO_F_HIGH[(pitch >> 8) as usize] * LUT_MIDI_TO_F_LOW[(pitch & 0xff) as usize]
    }

    pub fn render(&mut self, shape: OscillatorShape, note: f32, modulation: &[f32], out: &mut [f32]) -> f32 {
        match shape {
            OscillatorShape::Sine => self.render_sine(note, modulation, out),
            OscillatorShape::Triangle => self.render_polyblep(OscillatorShape::Triangle, note, modulation, out),
            OscillatorShape::Saw => self.render_polyblep(OscillatorShape::Saw, note, modulation, out),
            OscillatorShape::Pulse => self.render_polyblep(OscillatorShape::Pulse, note, modulation, out),
            OscillatorShape::NoiseLp => self.render_noise(note, modulation, out),
        }
    }

    fn render_sine(&mut self, note: f32, modulation: &[f32], out: &mut [f32]) -> f32 {
        let mut phase = self.phase;
        let target = self.midi_to_increment(note);
        let size = out.len();
        let mut phase_increment = ParameterInterpolator::new(&mut self.phase_increment, target, size);

        for (m, o) in modulation.iter().zip(out.iter_mut()) {
            phase += phase_increment.next();
            if phase >= 1.0 {
                phase -= 1.0;
            }
            let modulated_phase = (phase * K_TO_UINT32) as u32;
            let delta = (*m * 0.5 * K_TO_UINT32) as i32;
            let modulated_phase = modulated_phase.wrapping_add(delta as u32);
            let integral = (modulated_phase >> 22) as usize;
            let fractional = (modulated_phase << 10) as f32 * K_TO_FLOAT;
            let a = LUT_SIN[integral];
            let b = LUT_SIN[integral + 1];
            *o = a + (b - a) * fractional;
        }
        self.phase = phase;
        1.0
    }

    fn render_polyblep(&mut self, shape: OscillatorShape, note: f32, modulation: &[f32], out: &mut [f32]) -> f32 {
        let mut phase = self.phase;
        let target = self.midi_to_increment(note);
        let size = out.len();
        // Captured *before* the ramp below: the C's `ParameterInterpolator`
        // is a scope guard that only writes the ramped value back into
        // `phase_increment_` when it drops at the end of the function --
        // i.e. *after* the `return` expression below has already been
        // evaluated. The pulse-width gain formula therefore deliberately
        // reads the pre-ramp increment, not the new target.
        let old_phase_increment = self.phase_increment;
        let mut phase_increment = ParameterInterpolator::new(&mut self.phase_increment, target, size);

        let mut next_sample = self.next_sample;
        let mut high = self.high;
        let mut lp_state = self.lp_state;
        let mut hp_state = self.hp_state;

        for (m, o) in modulation.iter().zip(out.iter_mut()) {
            let mut this_sample = next_sample;
            next_sample = 0.0;

            let mut modulated_increment = phase_increment.next() * (1.0 + *m);
            if modulated_increment <= 0.0 {
                modulated_increment = 1.0e-7;
            }
            phase += modulated_increment;

            if shape == OscillatorShape::Triangle {
                if !high && phase >= 0.5 {
                    let t = (phase - 0.5) / modulated_increment;
                    this_sample += this_blep_sample(t);
                    next_sample += next_blep_sample(t);
                    high = true;
                }
                if phase >= 1.0 {
                    phase -= 1.0;
                    let t = phase / modulated_increment;
                    this_sample -= this_blep_sample(t);
                    next_sample -= next_blep_sample(t);
                    high = false;
                }
                let integrator_coefficient = modulated_increment * 0.0625;
                next_sample += if phase < 0.5 { 0.0 } else { 1.0 };
                this_sample = 128.0 * (this_sample - 0.5);
                lp_state += integrator_coefficient * (this_sample - lp_state);
                *o = lp_state;
            } else {
                if phase >= 1.0 {
                    phase -= 1.0;
                    let t = phase / modulated_increment;
                    this_sample -= this_blep_sample(t);
                    next_sample -= next_blep_sample(t);
                }
                next_sample += phase;

                if shape == OscillatorShape::Saw {
                    this_sample = this_sample * 2.0 - 1.0;
                    lp_state += 0.3 * (this_sample - lp_state);
                    *o = lp_state;
                } else {
                    lp_state += 0.25 * ((hp_state - this_sample) - lp_state);
                    *o = 4.0 * lp_state;
                    hp_state = this_sample;
                }
            }
        }

        self.high = high;
        self.phase = phase;
        self.next_sample = next_sample;
        self.lp_state = lp_state;
        self.hp_state = hp_state;

        if shape == OscillatorShape::Pulse {
            0.025 / (0.0002 + old_phase_increment)
        } else {
            1.0
        }
        // `phase_increment` (the interpolator) drops here, writing the fully
        // ramped value into `self.phase_increment` -- after the expression
        // above has already been computed, matching the C++'s destruction
        // order.
    }

    /// `Duck`: reads `external`, blends into `buf` in place (the C's
    /// `internal`/`destination` are always the same buffer at the one call
    /// site, `RenderNoise`).
    fn duck(&mut self, buf: &mut [f32], external: &[f32]) -> f32 {
        let mut level = self.external_input_level;
        for (b, &e) in buf.iter_mut().zip(external.iter()) {
            let error = e * e - level;
            level += (if error > 0.0 { 0.01 } else { 0.0001 }) * error;
            let mut internal_gain = 1.0 - 32.0 * level;
            if internal_gain <= 0.0 {
                internal_gain = 0.0;
            }
            *b = e + internal_gain * (*b - e);
        }
        self.external_input_level = level;
        level
    }

    fn render_noise(&mut self, note: f32, modulation: &[f32], out: &mut [f32]) -> f32 {
        for o in out.iter_mut() {
            let noise = Random::get_word() as f32 * K_TO_FLOAT;
            *o = 2.0 * noise - 1.0;
        }
        self.duck(out, modulation);
        self.filter.set_f_q(self.midi_to_increment(note) * 4.0, 1.0, FrequencyApproximation::Accurate);
        self.filter.process_in_place(FilterMode::LowPass, out);
        1.0
    }
}
