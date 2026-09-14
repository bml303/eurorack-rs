//! `peaks/number_station/number_station.{h,cc}` -- a shortwave
//! "number station" generator: either a synthesized voice reading digits
//! (spliced from a sample bank) or a wavering tone, both mixed with narrow-
//! band noise, an interference tone, ring modulation and wavefolding, then
//! split into two (LP/HP-filtered) output pairs.
//!
//! Downsamples its control processing 4x (`kDownsample`) and always emits
//! 4 output samples per control tick -- like `FmDrum`'s "every 4th sample"
//! trick, this only reproduces the firmware's exact behaviour when called
//! in blocks whose length is a multiple of 4 (`peaks::kBlockSize`); other
//! lengths still work (any trailing `size % 4` samples are simply never
//! written, matching the C++ exactly) but a host wanting bit-faithful
//! output should stick to multiples of 4.

use stmlib::clip16_sym;
use stmlib::fixed::interpolate_1022;
use stmlib::random::Random;

use crate::drums::svf::{Svf, SvfMode};
use crate::gate_processor::{ControlMode, GateFlags};
use crate::resources::{LUT_LFO_INCREMENTS, WAV_DIGITS, WAV_FOLD_SINE, WAV_SINE};

const K_DOWNSAMPLE: usize = 4;

const VOICE_DIGITS: [u32; 11] = [0, 4913, 7830, 11306, 14601, 18651, 22308, 26438, 30495, 33296, 36801];

#[derive(Debug, Clone, Copy, Default)]
pub struct NumberStation {
    tone: u16,
    pitch_shift: u16,
    transition_probability: u16,
    distortion: i32,
    noise: i32,
    digit: u8,

    drift: i32,

    phase: u32,
    noise_phase: u32,
    ringmod_phase: u32,
    interference_phase: u32,

    tone_amplitude: i16,
    lp_noise: i32,

    previous_inner_sample: i32,
    previous_outer_sample: i32,

    voice: bool,
    gate: bool,

    lp: Svf,
    hp: Svf,
}

impl NumberStation {
    pub fn init(&mut self) {
        self.tone_amplitude = 0;
        self.phase = 0;
        self.lp.init();
        self.lp.set_frequency(120 << 7);
        self.lp.set_resonance(16000);

        self.hp.init();
        self.hp.set_frequency(70 << 7);
        self.hp.set_resonance(8000);

        self.previous_inner_sample = 0;
        self.previous_outer_sample = 0;
    }

    pub fn configure(&mut self, parameter: &[u16; 4], control_mode: ControlMode) {
        if control_mode == ControlMode::Half {
            self.set_tone(parameter[0]);
            self.set_transition_probability(parameter[1]);
            self.set_noise(32768);
            self.set_distortion(32768);
        } else {
            self.set_tone(parameter[0]);
            self.set_transition_probability(parameter[1]);
            self.set_noise(parameter[2]);
            self.set_distortion(parameter[3]);
        }
    }

    pub fn set_tone(&mut self, tone: u16) {
        self.tone = (tone >> 2) + 32768 + 8192;
        self.pitch_shift = if tone < 32768 { 24576 + (tone >> 2) } else { 16384 + (tone >> 1) };
    }

    pub fn set_transition_probability(&mut self, transition_probability: u16) {
        self.transition_probability = transition_probability;
    }

    pub fn set_distortion(&mut self, distortion: u16) {
        self.distortion = 8192 + (((32767 - 8192) * distortion as u32) >> 16) as i32;
    }

    pub fn set_noise(&mut self, noise: u16) {
        self.noise = 256 + (noise >> 3) as i32;
    }

    pub fn set_voice(&mut self, voice: bool) {
        self.voice = voice;
    }

    pub fn digit(&self) -> u8 {
        self.digit
    }
    pub fn gate(&self) -> bool {
        self.gate
    }

    pub fn process(&mut self, gate_flags: &[GateFlags], out: &mut [i16]) {
        let phase_increment: u32 = if self.voice {
            self.pitch_shift as u32
        } else {
            let frequency = self.tone;
            let idx = (frequency >> 8) as usize;
            let a = LUT_LFO_INCREMENTS[idx] as i32;
            let b = LUT_LFO_INCREMENTS[idx + 1] as i32;
            let mut pi = a.wrapping_add(((b - a) >> 1).wrapping_mul((frequency & 0xff) as i32) >> 7);
            pi <<= 6;
            pi = pi.wrapping_mul(self.digit as i32 + 1);
            pi as u32
        };

        let drift_target = Random::get_sample() as i32;
        if drift_target > self.drift {
            self.drift = self.drift.wrapping_add((drift_target - self.drift) >> 13);
        } else {
            self.drift = self.drift.wrapping_sub((self.drift - drift_target) >> 13);
        }
        // Computed and clipped, then never actually used anywhere below --
        // dead code in the C++ too (no side effects beyond consuming
        // `self.drift`, which is otherwise unused here besides driving
        // `ringmod_delta` later). Kept for fidelity, same reasoning as the
        // other vestigial-code findings in this port.
        let _slow_noise = clip16_sym(self.drift << 5);

        let ticks = gate_flags.len() / K_DOWNSAMPLE;
        let mut gf = 0usize;
        let mut oi = 0usize;
        for _ in 0..ticks {
            for _ in 0..K_DOWNSAMPLE {
                let gate_flag = gate_flags[gf];
                gf += 1;
                if gate_flag.contains(GateFlags::RISING) {
                    let random = Random::get_sample() as u16;
                    if random < self.transition_probability {
                        self.digit = (random >> 2) as u8;
                        self.digit = if self.voice { self.digit % 10 } else { self.digit & 3 };
                    }
                    if self.voice {
                        self.phase = (VOICE_DIGITS[self.digit as usize]) << 16;
                    }
                }
                if gate_flag.contains(GateFlags::HIGH) {
                    let delta = (32767i32 - self.tone_amplitude as i32) >> 6;
                    self.tone_amplitude = (self.tone_amplitude as i32).wrapping_add(delta) as i16;
                } else {
                    let delta = self.tone_amplitude as i32 >> 6;
                    self.tone_amplitude = (self.tone_amplitude as i32).wrapping_sub(delta) as i16;
                    if self.tone_amplitude < 64 && self.tone_amplitude != 0 {
                        self.tone_amplitude -= 1;
                    }
                }
            }

            // Generate a distorted sine wave with fluctuating frequency.
            let mut digit: i32;
            if self.voice {
                let integral = self.phase >> 16;
                let fractional = (self.phase & 0xffff) as i32;
                if integral < VOICE_DIGITS[self.digit as usize + 1] {
                    let mask_a = (integral.wrapping_mul(53)) as u8;
                    let mask_b = mask_a.wrapping_add(53);
                    let a = (WAV_DIGITS[integral as usize] ^ mask_a) as i32;
                    let b = (WAV_DIGITS[integral as usize + 1] ^ mask_b) as i32;
                    digit = (a << 8).wrapping_add((b.wrapping_sub(a)).wrapping_mul(fractional) >> 8);
                    digit -= 32768;
                    self.phase = self.phase.wrapping_add(phase_increment);
                    self.gate = true;
                } else {
                    digit = 0;
                    self.gate = false;
                }
            } else {
                let lp_term = (self.lp_noise << 10) as u32;
                self.phase = self.phase.wrapping_add(phase_increment.wrapping_add(lp_term));
                digit = interpolate_1022(&WAV_SINE, self.phase) as i32;
                digit = digit.wrapping_mul(self.tone_amplitude as i32) >> 16;
                self.gate = self.tone_amplitude > 0;
            }
            let xor_term = (digit.wrapping_add(4096)) ^ 0x055a;
            digit = digit.wrapping_add((digit.wrapping_sub(xor_term)).wrapping_mul(self.distortion) >> 15);

            // Generate narrow-band noise.
            let random_sample = Random::get_sample() as i32;
            self.lp_noise = self.lp_noise.wrapping_add((random_sample - self.lp_noise) >> 6);
            self.noise_phase = self.noise_phase.wrapping_add(238370685);
            let mut noise_val = self.lp_noise.wrapping_mul(interpolate_1022(&WAV_SINE, self.noise_phase) as i32) >> 12;

            // Generate an interference tone.
            self.interference_phase = self.interference_phase.wrapping_add(710101260);
            noise_val = noise_val.wrapping_add(self.distortion.wrapping_mul(WAV_SINE[(self.interference_phase >> 22) as usize] as i32) >> 18);

            // Mix signal and noise.
            let mut inner_sample = digit.wrapping_add((noise_val.wrapping_sub(digit)).wrapping_mul(self.noise) >> 15);

            if random_sample >= 32767 - (self.noise >> 7) {
                inner_sample = 0;
            }

            // Final ringmod.
            let ringmod_delta = 38654706i32.wrapping_add(38654706i32.wrapping_mul(self.drift) >> 10);
            self.ringmod_phase = self.ringmod_phase.wrapping_add(ringmod_delta as u32);
            let mut ringmod = (interpolate_1022(&WAV_SINE, self.ringmod_phase) as i32) >> 1;
            ringmod = ringmod.wrapping_mul(inner_sample) >> 15;
            inner_sample = inner_sample.wrapping_add(ringmod.wrapping_mul(self.distortion) >> 15);
            inner_sample = clip16_sym(inner_sample);

            // And a pass of wavefolding...
            let fold_phase = (inner_sample.wrapping_mul(8192)).wrapping_add(1i32 << 31) as u32;
            inner_sample = interpolate_1022(&WAV_FOLD_SINE, fold_phase) as i32;

            let mut outer_sample = self.lp.process(SvfMode::Lp, self.hp.process(SvfMode::Hp, (inner_sample.wrapping_add(self.previous_inner_sample)) >> 1));
            out[oi] = ((self.previous_outer_sample + outer_sample) >> 1) as i16;
            out[oi + 1] = outer_sample as i16;
            self.previous_outer_sample = outer_sample;
            oi += 2;

            outer_sample = self.lp.process(SvfMode::Lp, self.hp.process(SvfMode::Hp, inner_sample));
            out[oi] = ((self.previous_outer_sample + outer_sample) >> 1) as i16;
            out[oi + 1] = outer_sample as i16;
            self.previous_outer_sample = outer_sample;
            oi += 2;

            self.previous_inner_sample = inner_sample;
        }
    }
}
