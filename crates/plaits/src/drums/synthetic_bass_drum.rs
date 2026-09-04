//! `plaits/dsp/drums/synthetic_bass_drum.h` -- a naive bass drum (FM'd
//! oscillator + amplitude envelope), "inadvertently 909-ish" per upstream.

use stmlib::fdsp::{one_pole, slope};
use stmlib::filter::{FilterMode, FrequencyApproximation, Svf};
use stmlib::parameter_interpolator::ParameterInterpolator;
use stmlib::units::semitones_to_ratio;
use stmlib::Random;

use crate::dsp::SAMPLE_RATE;
use crate::oscillator::sine;

#[derive(Default)]
pub struct SyntheticBassDrumClick {
    lp: f32,
    hp: f32,
    filter: Svf,
}

impl SyntheticBassDrumClick {
    pub fn init(&mut self) {
        self.lp = 0.0;
        self.hp = 0.0;
        self.filter.init();
        self.filter.set_f_q(5000.0 / SAMPLE_RATE, 2.0, FrequencyApproximation::Fast);
    }

    pub fn process(&mut self, input: f32) -> f32 {
        slope(&mut self.lp, input, 0.5, 0.1);
        one_pole(&mut self.hp, self.lp, 0.04);
        self.filter.process(FilterMode::LowPass, self.lp - self.hp)
    }
}

#[derive(Default)]
pub struct SyntheticBassDrumAttackNoise {
    lp: f32,
    hp: f32,
}

impl SyntheticBassDrumAttackNoise {
    pub fn init(&mut self) {
        self.lp = 0.0;
        self.hp = 0.0;
    }

    pub fn render(&mut self) -> f32 {
        let sample = Random::get_float();
        one_pole(&mut self.lp, sample, 0.05);
        one_pole(&mut self.hp, self.lp, 0.005);
        self.lp - self.hp
    }
}

#[derive(Default)]
pub struct SyntheticBassDrum {
    f0: f32,
    phase: f32,
    phase_noise: f32,
    fm: f32,
    fm_lp: f32,
    body_env: f32,
    body_env_lp: f32,
    transient_env: f32,
    transient_env_lp: f32,
    sustain_gain: f32,
    tone_lp: f32,
    click: SyntheticBassDrumClick,
    noise: SyntheticBassDrumAttackNoise,
    body_env_pulse_width: i32,
    fm_pulse_width: i32,
}

impl SyntheticBassDrum {
    pub fn init(&mut self) {
        *self = Self::default();
        self.click.init();
        self.noise.init();
    }

    fn distorted_sine(phase: f32, phase_noise: f32, dirtiness: f32) -> f32 {
        let phase = phase + phase_noise * dirtiness;
        let phase = phase - (phase as i32) as f32;
        let triangle = (if phase < 0.5 { phase } else { 1.0 - phase }) * 4.0 - 1.0;
        let sine_v = 2.0 * triangle / (1.0 + triangle.abs());
        let clean_sine = sine(phase + 0.75);
        sine_v + (1.0 - dirtiness) * (clean_sine - sine_v)
    }

    fn transistor_vca(s: f32, gain: f32) -> f32 {
        let s = (s - 0.6) * gain;
        3.0 * s / (2.0 + s.abs()) + gain * 0.3
    }

    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        sustain: bool,
        trigger: bool,
        accent: f32,
        f0: f32,
        tone: f32,
        decay: f32,
        mut dirtiness: f32,
        fm_envelope_amount: f32,
        fm_envelope_decay: f32,
        out: &mut [f32],
    ) {
        let decay = decay * decay;
        let fm_envelope_decay = fm_envelope_decay * fm_envelope_decay;

        let size = out.len();
        let mut f0_mod = ParameterInterpolator::new(&mut self.f0, f0, size);

        dirtiness *= (1.0 - 8.0 * f0).max(0.0);

        let fm_decay = 1.0 - 1.0 / (0.008 * (1.0 + fm_envelope_decay * 4.0) * SAMPLE_RATE);

        let body_env_decay = 1.0 - 1.0 / (0.02 * SAMPLE_RATE) * semitones_to_ratio(-decay * 60.0);
        let transient_env_decay = 1.0 - 1.0 / (0.005 * SAMPLE_RATE);
        let tone_f = (4.0 * f0 * semitones_to_ratio(tone * 108.0)).min(1.0);
        let transient_level = tone;

        if trigger {
            self.fm = 1.0;
            self.body_env = 0.3 + 0.7 * accent;
            self.transient_env = self.body_env;
            self.body_env_pulse_width = (SAMPLE_RATE * 0.001) as i32;
            self.fm_pulse_width = (SAMPLE_RATE * 0.0013) as i32;
        }

        let mut sustain_gain = ParameterInterpolator::new(&mut self.sustain_gain, accent * decay, size);

        for o in out.iter_mut() {
            one_pole(&mut self.phase_noise, Random::get_float() - 0.5, 0.002);

            let mut mix = 0.0f32;

            if sustain {
                self.phase += f0_mod.next();
                if self.phase >= 1.0 {
                    self.phase -= 1.0;
                }
                let body = Self::distorted_sine(self.phase, self.phase_noise, dirtiness);
                mix -= Self::transistor_vca(body, sustain_gain.next());
            } else {
                if self.fm_pulse_width != 0 {
                    self.fm_pulse_width -= 1;
                    self.phase = 0.25;
                } else {
                    self.fm *= fm_decay;
                    let fm = 1.0 + fm_envelope_amount * 3.5 * self.fm_lp;
                    self.phase += (f0_mod.next() * fm).min(0.5);
                    if self.phase >= 1.0 {
                        self.phase -= 1.0;
                    }
                }

                if self.body_env_pulse_width != 0 {
                    self.body_env_pulse_width -= 1;
                } else {
                    self.body_env *= body_env_decay;
                    self.transient_env *= transient_env_decay;
                }

                const ENVELOPE_LP_F: f32 = 0.1;
                one_pole(&mut self.body_env_lp, self.body_env, ENVELOPE_LP_F);
                one_pole(&mut self.transient_env_lp, self.transient_env, ENVELOPE_LP_F);
                one_pole(&mut self.fm_lp, self.fm, ENVELOPE_LP_F);

                let body = Self::distorted_sine(self.phase, self.phase_noise, dirtiness);
                let transient = self.click.process(if self.body_env_pulse_width != 0 { 0.0 } else { 1.0 })
                    + self.noise.render();

                mix -= Self::transistor_vca(body, self.body_env_lp);
                mix -= transient * self.transient_env_lp * transient_level;
            }

            one_pole(&mut self.tone_lp, mix, tone_f);
            *o = self.tone_lp;
        }
    }
}
