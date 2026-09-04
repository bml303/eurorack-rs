//! `plaits/dsp/drums/analog_snare_drum.h` -- an 808-style snare: 5 resonant
//! modes excited by a trigger pulse, mixed with filtered noise gated by a
//! `snappy` envelope.

use stmlib::fdsp::{one_pole, soft_clip};
use stmlib::filter::{FilterMode, FrequencyApproximation, Svf};
use stmlib::parameter_interpolator::ParameterInterpolator;
use stmlib::units::semitones_to_ratio;
use stmlib::Random;

use crate::dsp::SAMPLE_RATE;
use crate::oscillator::SineOscillator;

pub const NUM_MODES: usize = 5;

#[rustfmt::skip]
const MODE_FREQUENCIES: [f32; NUM_MODES] = [1.00, 2.00, 3.18, 4.16, 5.62];

#[derive(Default)]
pub struct AnalogSnareDrum {
    pulse_remaining_samples: i32,
    pulse: f32,
    pulse_height: f32,
    pulse_lp: f32,
    noise_envelope: f32,
    sustain_gain: f32,
    resonator: [Svf; NUM_MODES],
    oscillator: [SineOscillator; NUM_MODES],
    noise_filter: Svf,
}

impl AnalogSnareDrum {
    pub fn init(&mut self) {
        *self = Self::default();
        for i in 0..NUM_MODES {
            self.resonator[i].init();
            self.oscillator[i].init();
        }
        self.noise_filter.init();
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
        snappy: f32,
        out: &mut [f32],
    ) {
        let decay_xt = decay * (1.0 + decay * (decay - 1.0));
        let trigger_pulse_duration = (1.0e-3 * SAMPLE_RATE) as i32;
        let pulse_decay_time = 0.1e-3 * SAMPLE_RATE;
        let q = 2000.0 * semitones_to_ratio(decay_xt * 84.0);
        let noise_envelope_decay = 1.0 - 0.0017 * semitones_to_ratio(-decay * (50.0 + snappy * 10.0));
        let exciter_leak = snappy * (2.0 - snappy) * 0.1;

        let snappy = (snappy * 1.1 - 0.05).clamp(0.0, 1.0);

        if trigger {
            self.pulse_remaining_samples = trigger_pulse_duration;
            self.pulse_height = 3.0 + 7.0 * accent;
            self.noise_envelope = 2.0;
        }

        let mut f = [0.0f32; NUM_MODES];
        let mut gain = [0.0f32; NUM_MODES];

        for i in 0..NUM_MODES {
            f[i] = (f0 * MODE_FREQUENCIES[i]).min(0.499);
            self.resonator[i].set_f_q(
                f[i],
                1.0 + f[i] * (if i == 0 { q } else { q * 0.25 }),
                FrequencyApproximation::Fast,
            );
        }

        let mut tone = tone;
        if tone < 0.666667 {
            // 808-style (2 modes).
            tone *= 1.5;
            gain[0] = 1.5 + (1.0 - tone) * (1.0 - tone) * 4.5;
            gain[1] = 2.0 * tone + 0.15;
            for g in gain.iter_mut().skip(2) {
                *g = 0.0;
            }
        } else {
            // What the 808 could have been, with extra modes.
            tone = (tone - 0.666667) * 3.0;
            gain[0] = 1.5 - tone * 0.5;
            gain[1] = 2.15 - tone * 0.7;
            for g in gain.iter_mut().skip(2) {
                *g = tone;
                tone *= tone;
            }
        }

        let f_noise = (f0 * 16.0).clamp(0.0, 0.499);
        self.noise_filter.set_f_q(f_noise, 1.0 + f_noise * 1.5, FrequencyApproximation::Fast);

        let size = out.len();
        let mut sustain_gain = ParameterInterpolator::new(&mut self.sustain_gain, accent * decay, size);

        for o in out.iter_mut() {
            // Q45 / Q46
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

            let sustain_gain_value = sustain_gain.next();

            one_pole(&mut self.pulse_lp, pulse, 0.75);

            let mut shell = 0.0f32;
            for i in 0..NUM_MODES {
                let excitation = if i == 0 {
                    (pulse - self.pulse_lp) + 0.006 * pulse
                } else {
                    0.026 * pulse
                };
                shell += gain[i]
                    * if sustain {
                        self.oscillator[i].next(f[i]) * sustain_gain_value * 0.25
                    } else {
                        self.resonator[i].process(FilterMode::BandPass, excitation) + excitation * exciter_leak
                    };
            }
            shell = soft_clip(shell);

            // C56 / R194 / Q48 / C54 / R188 / D54
            let mut noise = 2.0 * Random::get_float() - 1.0;
            if noise < 0.0 {
                noise = 0.0;
            }
            self.noise_envelope *= noise_envelope_decay;
            noise *= (if sustain { sustain_gain_value } else { self.noise_envelope }) * snappy * 2.0;

            let noise = self.noise_filter.process(FilterMode::BandPass, noise);

            *o = noise + shell * (1.0 - snappy);
        }
    }
}
