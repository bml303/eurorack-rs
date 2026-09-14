//! `streams/vactrol.{h,cc}` -- models a vactrol-based VCA/VCF (as found in
//! Mutable's earlier "Blinds"/blinds-style circuits): a photocell whose
//! resistance lags a light source driven by the input signal, with
//! first-/second-order lag stages, a "memory"/desensitization effect, and a
//! non-linear (Gompertz-curve) amplitude response. `plucked_` selects an
//! alternate "plucked" mode: a Schmitt-triggered VCF/VCA envelope pair
//! instead of the continuous photocell model.

use stmlib::fixed::interpolate_1022;

use crate::consts::{K_ABOVE_UNITY_GAIN, K_SCHMITT_TRIGGER_THRESHOLD};
use crate::meta_parameters::compute_amount_offset;
use crate::resources::{LUT_LP_COEFFICIENTS, WAV_GOMPERTZ};

#[derive(Debug, Clone, Copy, Default)]
pub struct Vactrol {
    target_frequency_amount: i32,
    target_frequency_offset: i32,
    frequency_amount: i32,
    frequency_offset: i32,

    attack_coefficient: i32,
    decay_coefficient: i32,
    fast_attack_coefficient: i32,
    fast_decay_coefficient: i32,

    state: [i32; 4],
    excite: i32,

    gate: bool,
    plucked: bool,
}

impl Vactrol {
    pub fn init(&mut self) {
        self.state = [0; 4];
        self.excite = 0;
    }

    pub fn configure(&mut self, alternate: bool, parameters: &[i32; 2], globals: Option<&[i32; 4]>) {
        let attack_time: i32;
        let decay_time: i32;

        if let Some(globals) = globals {
            // Attack: 10ms to 1000ms
            attack_time = 128 + (globals[0] >> 8);
            // Decay: 10ms to 5000ms
            decay_time = 128 + (globals[2].wrapping_mul(355) >> 16);
            let (a, o) = compute_amount_offset(parameters[1]);
            self.target_frequency_amount = a;
            self.target_frequency_offset = o;
        } else {
            let mut shape = parameters[0];
            let (a, o) = compute_amount_offset(parameters[1]);
            self.target_frequency_amount = a;
            self.target_frequency_offset = o;

            if shape < 32768 {
                // attack: 10ms
                attack_time = 128;
                // decay: 50ms to 2000ms
                decay_time = 227 + (shape.wrapping_mul(196) >> 15);
            } else if shape < 49512 {
                shape -= 32768;
                // attack: 10ms to 500ms.
                attack_time = 128 + (shape.wrapping_mul(227) >> 15);
                // decay: 2000ms to 1000ms.
                decay_time = 423 - (89i32.wrapping_mul(shape) >> 15);
            } else {
                shape -= 49512;
                // attack: 500ms to 50ms.
                attack_time = 355 - (shape >> 7);
                // decay: 1000ms to 100ms.
                decay_time = 384 - (128i32.wrapping_mul(shape) >> 15);
            }
        }

        self.attack_coefficient = LUT_LP_COEFFICIENTS[attack_time as usize] as i32;
        self.fast_attack_coefficient = LUT_LP_COEFFICIENTS[(attack_time - 128) as usize] as i32;
        self.decay_coefficient = LUT_LP_COEFFICIENTS[decay_time as usize] as i32;
        self.fast_decay_coefficient = LUT_LP_COEFFICIENTS[(decay_time - 128) as usize] as i32;

        self.plucked = alternate;
        if alternate {
            self.fast_attack_coefficient <<= 4;
        } else {
            self.decay_coefficient >>= 1;
        }

        let mut ringing_tail: i32 = 8192;
        let headroom = 65535 - self.target_frequency_offset;
        if ringing_tail > headroom {
            ringing_tail = headroom;
        }
        if ringing_tail > self.target_frequency_amount {
            ringing_tail = self.target_frequency_amount;
        }

        self.target_frequency_offset += ringing_tail;
        self.target_frequency_amount -= ringing_tail;
    }

    pub fn process(&mut self, _audio: i16, excite: i16, gain: &mut u16, frequency: &mut u16) {
        self.frequency_amount = self.frequency_amount.wrapping_add((self.target_frequency_amount - self.frequency_amount) >> 8);
        self.frequency_offset = self.frequency_offset.wrapping_add((self.target_frequency_offset - self.frequency_offset) >> 8);

        let excite = if excite < 0 { 0 } else { excite as i32 };

        if self.plucked {
            if !self.gate {
                if excite > K_SCHMITT_TRIGGER_THRESHOLD {
                    self.gate = true;
                    self.state[0] = 32767 << 16;
                    self.state[1] = 32767 << 16;
                }
            } else if excite < (K_SCHMITT_TRIGGER_THRESHOLD >> 1) {
                self.gate = false;
            }

            // Filter the excitation pulses.
            self.state[0] = self.state[0].wrapping_sub(((self.state[0] as i64).wrapping_mul(self.fast_decay_coefficient as i64) >> 31) as i32);
            self.state[1] = self.state[1].wrapping_sub(((self.state[1] as i64).wrapping_mul(self.decay_coefficient as i64) >> 31) as i32);

            // VCF envelope.
            let mut error = self.state[0].wrapping_sub(self.state[2]);
            let mut coefficient = if error > 0 { self.fast_attack_coefficient } else { self.fast_decay_coefficient };
            self.state[2] = self.state[2].wrapping_add(((error as i64).wrapping_mul(coefficient as i64) >> 31) as i32);

            // VCA envelope.
            error = self.state[1].wrapping_sub(self.state[3]);
            coefficient = if error > 0 { self.fast_attack_coefficient } else { self.decay_coefficient };
            // Increase the duration of the tail.
            let strength: i64 = if error > 0 { error as i64 } else { -(error as i64) };
            let coefficient64 = ((coefficient as i64) >> 1).wrapping_add((coefficient as i64).wrapping_mul(strength) >> 31);
            self.state[3] = self.state[3].wrapping_add(((error as i64).wrapping_mul(coefficient64) >> 31) as i32);

            let vcf_amount = (self.state[2] >> 16) as u16;
            let vca_amount = interpolate_1022(&WAV_GOMPERTZ, ((self.state[3] >> 2).wrapping_mul(3)) as u32) as u16;

            *gain = (K_ABOVE_UNITY_GAIN.wrapping_mul(vca_amount as i32) >> 15) as u16;
            *frequency = self.frequency_offset.wrapping_add(self.frequency_amount.wrapping_mul(vcf_amount as i32) >> 15) as u16;

            return;
        }

        // Low-pass filter the negative edges to prevent fast pulse to
        // immediately decay before the vactrol has started reacting. This
        // allows the EXCITE input to be used for both controlling the
        // vactrol or just plucking it from a trigger.
        let mut error = excite.wrapping_sub(self.excite);
        let mut coefficient: i64 = if error > 0 { 1 << 30 } else { (self.decay_coefficient as i64) << 1 };
        self.excite = self.excite.wrapping_add(((error as i64).wrapping_mul(coefficient) >> 31) as i32);
        let excite = self.excite;

        let mut input = self.frequency_offset;
        input = input.wrapping_add(self.frequency_amount >> 1);
        input = (65535 + input) >> 1;
        input = input.wrapping_mul(excite);

        self.state[3] = self.state[3].wrapping_add((((input - self.state[3]) as i64).wrapping_mul(67976239) >> 31) as i32);

        error = input.wrapping_sub(self.state[0]);
        if error > 0 {
            if self.state[1] > 0 {
                coefficient = self.attack_coefficient as i64;
                coefficient = coefficient.wrapping_add(coefficient.wrapping_mul(255 - (self.state[2] >> 23) as i64) >> 6);
            } else {
                coefficient = self.fast_attack_coefficient as i64;
            }
        } else if self.state[1] < 0 {
            coefficient = self.decay_coefficient as i64;
        } else {
            coefficient = self.fast_decay_coefficient as i64;
        }
        // First order.
        self.state[0] = self.state[0].wrapping_add(((error as i64).wrapping_mul(coefficient) >> 31) as i32);
        // Second order.
        self.state[1] = self.state[1].wrapping_add((((error - self.state[1]) as i64).wrapping_mul(coefficient) >> 31) as i32);

        // Memory effect.
        let mut sensitivity = self.state[0];
        if sensitivity > (1 << 28) {
            sensitivity = 1 << 31;
        } else {
            sensitivity <<= 3;
        }
        error = sensitivity.wrapping_sub(self.state[2]);
        if error > 0 {
            // Get into the "sensitized" state in 1s.
            self.state[2] = self.state[2].wrapping_add(((error as i64).wrapping_mul(138132) >> 31) as i32);
        } else {
            // Get out of the "sensitized" state in 60s.
            self.state[2] = self.state[2].wrapping_add(((error as i64).wrapping_mul(1151) >> 31) as i32);
        }

        // Apply non-linearity.
        let mut index = self.state[0] >> 1;

        // A little hack to add overshoot...
        index = index.wrapping_add((self.state[3] >> 15).wrapping_mul(self.state[1] >> 15) >> 1);
        if index < 0 {
            index = 0;
        } else if index >= (1 << 30) {
            index = (1 << 30) - 1;
        }
        let amplitude: u16 = if index < 536_870_912 { interpolate_1022(&WAV_GOMPERTZ, (index as u32) << 3) as u16 } else { 32767 };
        let mut cutoff = (index >> 14) as u16;
        if cutoff >= 32767 {
            cutoff = 32767;
        }
        let cutoff = ((cutoff as i32).wrapping_mul(cutoff as i32) >> 15) as u16;
        *gain = (K_ABOVE_UNITY_GAIN.wrapping_mul(amplitude as i32) >> 15) as u16;
        *frequency = self.frequency_offset.wrapping_add(self.frequency_amount.wrapping_mul(cutoff as i32) >> 15) as u16;
    }
}
