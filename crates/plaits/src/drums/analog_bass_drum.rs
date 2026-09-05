//! `plaits/dsp/drums/analog_bass_drum.h` -- an 808-style bass drum: a trigger
//! pulse (with diode clipping) rings a resonant band-pass, an FM pulse drives
//! a self-FM "punch"; in `sustain` mode the resonator is replaced by a free
//! running sine so the drum becomes a tunable tone generator.

use stmlib::fdsp::one_pole;
use stmlib::filter::{FilterMode, FrequencyApproximation, Svf};
use stmlib::parameter_interpolator::ParameterInterpolator;
use stmlib::units::semitones_to_ratio;

use crate::dsp::SAMPLE_RATE;
use crate::oscillator::SineOscillator;

#[inline]
fn diode(x: f32) -> f32 {
    if x >= 0.0 {
        x
    } else {
        let x = x * 2.0;
        0.7 * x / (1.0 + x.abs())
    }
}

#[derive(Default, Debug)]
pub struct AnalogBassDrum {
    pulse_remaining_samples: i32,
    fm_pulse_remaining_samples: i32,
    pulse: f32,
    pulse_height: f32,
    pulse_lp: f32,
    fm_pulse_lp: f32,
    retrig_pulse: f32,
    lp_out: f32,
    tone_lp: f32,
    sustain_gain: f32,
    resonator: Svf,
    oscillator: SineOscillator,
}

impl AnalogBassDrum {
    pub fn init(&mut self) {
        *self = Self::default();
        self.resonator.init();
        self.oscillator.init();
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
        attack_fm_amount: f32,
        self_fm_amount: f32,
        out: &mut [f32],
    ) {
        let trigger_pulse_duration = (1.0e-3 * SAMPLE_RATE) as i32;
        let fm_pulse_duration = (6.0e-3 * SAMPLE_RATE) as i32;
        let pulse_decay_time = 0.2e-3 * SAMPLE_RATE;
        let pulse_filter_time = 0.1e-3 * SAMPLE_RATE;
        let retrig_pulse_duration = 0.05 * SAMPLE_RATE;

        let scale = 0.001 / f0;
        let q = 1500.0 * semitones_to_ratio(decay * 80.0);
        let tone_f = (4.0 * f0 * semitones_to_ratio(tone * 108.0)).min(1.0);
        let exciter_leak = 0.08 * (tone + 0.25);

        if trigger {
            self.pulse_remaining_samples = trigger_pulse_duration;
            self.fm_pulse_remaining_samples = fm_pulse_duration;
            self.pulse_height = 3.0 + 7.0 * accent;
            self.lp_out = 0.0;
        }

        let size = out.len();
        let mut sustain_gain =
            ParameterInterpolator::new(&mut self.sustain_gain, accent * decay, size);

        for o in out.iter_mut() {
            // Q39 / Q40
            let mut pulse;
            if self.pulse_remaining_samples != 0 {
                self.pulse_remaining_samples -= 1;
                pulse = if self.pulse_remaining_samples != 0 {
                    self.pulse_height
                } else {
                    self.pulse_height - 1.0
                };
                self.pulse = pulse;
            } else {
                self.pulse *= 1.0 - 1.0 / pulse_decay_time;
                pulse = self.pulse;
            }
            if sustain {
                pulse = 0.0;
            }

            // C40 / R163 / R162 / D83
            one_pole(&mut self.pulse_lp, pulse, 1.0 / pulse_filter_time);
            pulse = diode((pulse - self.pulse_lp) + pulse * 0.044);

            // Q41 / Q42
            let mut fm_pulse = 0.0;
            if self.fm_pulse_remaining_samples != 0 {
                self.fm_pulse_remaining_samples -= 1;
                fm_pulse = 1.0;
                // C39 / C52
                self.retrig_pulse = if self.fm_pulse_remaining_samples != 0 {
                    0.0
                } else {
                    -0.8
                };
            } else {
                // C39 / R161
                self.retrig_pulse *= 1.0 - 1.0 / retrig_pulse_duration;
            }
            if sustain {
                fm_pulse = 0.0;
            }
            one_pole(&mut self.fm_pulse_lp, fm_pulse, 1.0 / pulse_filter_time);

            // Q43 and R170 leakage
            let punch = 0.7 + diode(10.0 * self.lp_out - 1.0);

            // Q43 / R165
            let attack_fm = self.fm_pulse_lp * 1.7 * attack_fm_amount;
            let self_fm = punch * 0.08 * self_fm_amount;
            let f = (f0 * (1.0 + attack_fm + self_fm)).clamp(0.0, 0.4);

            let resonator_out;
            if sustain {
                let (s, c) = self.oscillator.next_quadrature(f, sustain_gain.next());
                resonator_out = s;
                self.lp_out = c;
            } else {
                self.resonator
                    .set_f_q(f, 1.0 + q * f, FrequencyApproximation::Dirty);
                let (bp, lp) = self.resonator.process_dual(
                    FilterMode::BandPass,
                    FilterMode::LowPass,
                    (pulse - self.retrig_pulse * 0.2) * scale,
                );
                resonator_out = bp;
                self.lp_out = lp;
            }

            one_pole(
                &mut self.tone_lp,
                pulse * exciter_leak + resonator_out,
                tone_f,
            );

            *o = self.tone_lp;
        }
    }
}
