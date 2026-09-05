//! `plaits/dsp/drums/synthetic_snare_drum.h` -- a naive (909-inspired) snare:
//! two coupled distorted-sine oscillators (ratio 1.47) plus filtered noise.

use stmlib::fdsp::sqrt;
use stmlib::filter::{FilterMode, FrequencyApproximation, OnePole, Svf};
use stmlib::parameter_interpolator::ParameterInterpolator;
use stmlib::units::semitones_to_ratio;
use stmlib::Random;

use crate::dsp::SAMPLE_RATE;

#[derive(Default, Debug)]
pub struct SyntheticSnareDrum {
    phase: [f32; 2],
    drum_amplitude: f32,
    snare_amplitude: f32,
    fm: f32,
    sustain_gain: f32,
    hold_counter: i32,
    drum_lp: OnePole,
    snare_hp: OnePole,
    snare_lp: Svf,
}

#[inline]
fn distorted_sine(phase: f32) -> f32 {
    let triangle = (if phase < 0.5 { phase } else { 1.0 - phase }) * 4.0 - 1.3;
    2.0 * triangle / (1.0 + triangle.abs())
}

impl SyntheticSnareDrum {
    pub fn init(&mut self) {
        *self = Self::default();
        self.drum_lp.init();
        self.snare_hp.init();
        self.snare_lp.init();
    }

    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        sustain: bool,
        trigger: bool,
        accent: f32,
        f0: f32,
        fm_amount: f32,
        decay: f32,
        snappy: f32,
        out: &mut [f32],
    ) {
        let decay_xt = decay * (1.0 + decay * (decay - 1.0));
        let fm_amount = fm_amount * fm_amount;
        let drum_decay = 1.0
            - 1.0 / (0.015 * SAMPLE_RATE)
                * semitones_to_ratio(-decay_xt * 72.0 - fm_amount * 12.0 + snappy * 7.0);
        let snare_decay =
            1.0 - 1.0 / (0.01 * SAMPLE_RATE) * semitones_to_ratio(-decay * 60.0 - snappy * 7.0);
        let fm_decay = 1.0 - 1.0 / (0.007 * SAMPLE_RATE);

        let snappy = (snappy * 1.1 - 0.05).clamp(0.0, 1.0);

        let drum_level = sqrt(1.0 - snappy);
        let snare_level = sqrt(snappy);

        let snare_f_min = (10.0 * f0).min(0.5);
        let snare_f_max = (35.0 * f0).min(0.5);

        self.snare_hp
            .set_f(snare_f_min, FrequencyApproximation::Fast);
        self.snare_lp.set_f_q(
            snare_f_max,
            0.5 + 2.0 * snappy,
            FrequencyApproximation::Fast,
        );
        self.drum_lp.set_f(3.0 * f0, FrequencyApproximation::Fast);

        if trigger {
            self.snare_amplitude = 0.3 + 0.7 * accent;
            self.drum_amplitude = self.snare_amplitude;
            self.fm = 1.0;
            self.phase[0] = 0.0;
            self.phase[1] = 0.0;
            self.hold_counter = ((0.04 + decay * 0.03) * SAMPLE_RATE) as i32;
        }

        let block_size = out.len();
        let mut sustain_gain =
            ParameterInterpolator::new(&mut self.sustain_gain, accent * decay, block_size);

        // Mirrors the C's `while (size--)`: `size` is the remaining-samples
        // countdown (its parity gates the drum decay below), not a forward index.
        let mut size = block_size;
        for o in out.iter_mut() {
            size -= 1;
            if sustain {
                self.snare_amplitude = sustain_gain.next();
                self.drum_amplitude = self.snare_amplitude;
                self.fm = 0.0;
            } else {
                // The drum envelope has a long tail; the snare has a 40-70ms hold.
                self.drum_amplitude *= if self.drum_amplitude > 0.03 || (size & 1) == 0 {
                    drum_decay
                } else {
                    1.0
                };
                if self.hold_counter != 0 {
                    self.hold_counter -= 1;
                } else {
                    self.snare_amplitude *= snare_decay;
                }
                self.fm *= fm_decay;
            }

            // The 909 circuit couples the two oscillators' resets via some
            // intermodulation noise from the collector of Q40.
            let mut reset_noise = 0.0f32;
            let mut reset_noise_amount = ((0.125 - f0) * 8.0).clamp(0.0, 1.0);
            reset_noise_amount *= reset_noise_amount;
            reset_noise_amount *= fm_amount;
            reset_noise += if self.phase[0] > 0.5 { -1.0 } else { 1.0 };
            reset_noise += if self.phase[1] > 0.5 { -1.0 } else { 1.0 };
            reset_noise *= reset_noise_amount * 0.025;

            let f = f0 * (1.0 + fm_amount * (4.0 * self.fm));
            self.phase[0] += f;
            self.phase[1] += f * 1.47;
            if reset_noise_amount > 0.1 {
                if self.phase[0] >= 1.0 + reset_noise {
                    self.phase[0] = 1.0 - self.phase[0];
                }
                if self.phase[1] >= 1.0 + reset_noise {
                    self.phase[1] = 1.0 - self.phase[1];
                }
            } else {
                if self.phase[0] >= 1.0 {
                    self.phase[0] -= 1.0;
                }
                if self.phase[1] >= 1.0 {
                    self.phase[1] -= 1.0;
                }
            }

            let mut drum = -0.1f32;
            drum += distorted_sine(self.phase[0]) * 0.60;
            drum += distorted_sine(self.phase[1]) * 0.25;
            drum *= self.drum_amplitude * drum_level;
            drum = self.drum_lp.process(FilterMode::LowPass, drum);

            let noise = Random::get_float();
            let mut snare = self.snare_lp.process(FilterMode::LowPass, noise);
            snare = self.snare_hp.process(FilterMode::HighPass, snare);
            snare = (snare + 0.1) * (self.snare_amplitude + self.fm) * snare_level;

            *o = snare + drum;
        }
    }
}
