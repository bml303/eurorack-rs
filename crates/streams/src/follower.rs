//! `streams/follower.{h,cc}` -- a 3-band envelope follower (low/medium/high,
//! split by two cascaded `Svf`s) that also estimates a spectral centroid to
//! drive the frequency output.

use stmlib::fixed::interpolate_824_u16;

use crate::consts::K_UNITY_GAIN;
use crate::meta_parameters::compute_amount_offset;
use crate::resources::{LUT_LP_COEFFICIENTS, LUT_SQUARE_ROOT};
use crate::svf::Svf;

pub const K_NUM_BANDS: usize = 3;

#[derive(Debug, Clone, Copy, Default)]
pub struct Follower {
    analysis_low: Svf,
    analysis_medium: Svf,
    energy: [[i32; 2]; K_NUM_BANDS],
    follower: [i64; K_NUM_BANDS],

    attack_coefficient: [i64; K_NUM_BANDS],
    decay_coefficient: [i64; K_NUM_BANDS],
    follower_lp: [i64; K_NUM_BANDS],

    spectrum: [i32; K_NUM_BANDS],

    centroid: i32,

    frequency_offset: i32,
    frequency_amount: i32,
    target_frequency_offset: i32,
    target_frequency_amount: i32,

    only_filter: bool,
}

impl Follower {
    pub fn init(&mut self) {
        self.analysis_low.init();
        self.analysis_low.set_frequency(45 << 7);
        self.analysis_low.set_resonance(0);
        self.analysis_medium.init();
        self.analysis_medium.set_frequency(86 << 7);
        self.analysis_medium.set_resonance(0);

        for i in 0..K_NUM_BANDS {
            self.energy[i] = [0, 0];
            self.follower[i] = 0;
            self.follower_lp[i] = 0;
            self.spectrum[i] = 0;
        }
        self.centroid = 0;
    }

    pub fn process(&mut self, _audio: i16, excite: i16, gain: &mut u16, frequency: &mut u16) {
        self.frequency_amount = self.frequency_amount.wrapping_add((self.target_frequency_amount - self.frequency_amount) >> 8);
        self.frequency_offset = self.frequency_offset.wrapping_add((self.target_frequency_offset - self.frequency_offset) >> 8);

        self.analysis_low.process(excite as i32);
        self.analysis_medium.process(self.analysis_low.hp());

        let channel = [self.analysis_low.lp(), self.analysis_medium.lp(), self.analysis_medium.hp()];

        let mut envelope: i32 = 0;
        let mut centroid_numerator: i32 = 0;
        let mut centroid_denominator: i32 = 0;
        // `i` indexes `channel` plus several `self` fields together.
        #[allow(clippy::needless_range_loop)]
        for i in 0..K_NUM_BANDS {
            let energy = channel[i].wrapping_mul(channel[i]);

            // Ride an ascending peak.
            if self.energy[i][0] < self.energy[i][1] && self.energy[i][1] < energy && (energy as i64) > self.follower[i] {
                self.follower[i] = energy as i64;
            }
            // Otherwise, hold and snap on local maxima.
            if self.energy[i][0] <= self.energy[i][1] && self.energy[i][1] >= energy {
                self.follower[i] = self.energy[i][1] as i64;
            }
            self.energy[i][0] = self.energy[i][1];
            self.energy[i][1] = energy;

            // Then let a low-pass filter smooth things out.
            let error = self.follower[i] - self.follower_lp[i];
            if error > 0 {
                self.follower_lp[i] = self.follower_lp[i].wrapping_add(error.wrapping_mul(self.attack_coefficient[i]) >> 31);
            } else {
                self.follower_lp[i] = self.follower_lp[i].wrapping_add(error.wrapping_mul(self.decay_coefficient[i]) >> 31);
            }
            // `envelope` (i32) += an i64 term: C widens `envelope` to i64 for
            // the addition, then narrows the sum back -- not the same as
            // narrowing the term first when the sum itself would overflow
            // i32, so match that order exactly.
            envelope = ((envelope as i64).wrapping_add(self.follower_lp[i] >> 13)) as i32;

            // Integrate more slowly for spectrum estimation. `error` stays
            // i64 here too (widened `spectrum_[i]`), truncated back to i32
            // only on the final assignment, same reasoning as `envelope` above.
            let (error, shift) = if self.only_filter {
                (self.follower_lp[i].wrapping_sub(self.spectrum[i] as i64), 6)
            } else {
                (self.follower[i].wrapping_sub(self.spectrum[i] as i64), 10)
            };
            self.spectrum[i] = ((self.spectrum[i] as i64).wrapping_add(error >> shift)) as i32;
            centroid_numerator = centroid_numerator.wrapping_add((i as i32).wrapping_mul(self.spectrum[i] >> 1) >> 16);
            centroid_denominator = centroid_denominator.wrapping_add(self.spectrum[i] >> 16);
        }

        envelope = envelope.clamp(0, 65535);

        let gain_mod = (interpolate_824_u16(&LUT_SQUARE_ROOT, (envelope as u32) << 16) >> 1) as i32;
        let centroid = (centroid_numerator << 15) / (centroid_denominator + 1);
        if gain_mod > 4096 {
            self.centroid = centroid;
        } else if gain_mod > 2048 {
            self.centroid = self.centroid.wrapping_add((centroid - self.centroid) >> 8);
        }

        *gain = (gain_mod.wrapping_mul(K_UNITY_GAIN) >> 15) as u16;
        *frequency = self.frequency_offset.wrapping_add(self.centroid.wrapping_mul(self.frequency_amount) >> 15) as u16;

        if self.only_filter {
            *gain = *frequency;
            *frequency = 65535;
        }
    }

    pub fn configure(&mut self, alternate: bool, parameters: &[i32; 2], globals: Option<&[i32; 4]>) {
        let attack_time: i32;
        let decay_time: i32;

        if let Some(globals) = globals {
            // Attack: 1ms to 100ms
            attack_time = globals[0] >> 8;
            // Decay: 10ms to 1000ms
            decay_time = 128 + (globals[2] >> 8);
            let (a, o) = compute_amount_offset(parameters[1]);
            self.target_frequency_amount = a;
            self.target_frequency_offset = o;
        } else {
            let mut shape = parameters[0];
            if shape < 32768 {
                // attack: 1ms to 2ms.
                attack_time = shape.wrapping_mul(39) >> 15;
                // decay: 10ms to 100ms.
                decay_time = 128 + (shape.wrapping_mul(128) >> 15);
            } else {
                shape -= 32768;
                // attack: 2ms to 20ms.
                attack_time = 39 + (shape.wrapping_mul(128) >> 15);
                // decay: 100ms to 200ms.
                decay_time = 128 + 128 + (shape.wrapping_mul(39) >> 15);
            }
            let (a, o) = compute_amount_offset(parameters[1]);
            self.target_frequency_amount = a;
            self.target_frequency_offset = o;
        }

        // Slow down the attack detection on low frequencies.
        self.attack_coefficient[0] = LUT_LP_COEFFICIENTS[(attack_time + 39) as usize] as i64;
        self.attack_coefficient[1] = LUT_LP_COEFFICIENTS[(attack_time + 19) as usize] as i64;
        self.attack_coefficient[2] = LUT_LP_COEFFICIENTS[attack_time as usize] as i64;

        // Slow down the decay detection on high frequencies as there is more noise.
        self.decay_coefficient[0] = LUT_LP_COEFFICIENTS[(decay_time + 39) as usize] as i64;
        self.decay_coefficient[1] = LUT_LP_COEFFICIENTS[(decay_time + 19) as usize] as i64;
        self.decay_coefficient[2] = LUT_LP_COEFFICIENTS[(decay_time + 99) as usize] as i64;

        self.only_filter = alternate;
    }
}
