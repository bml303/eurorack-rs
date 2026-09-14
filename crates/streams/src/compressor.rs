//! `streams/compressor.{h,cc}` -- a feed-forward RMS compressor with a
//! sidechain-vs-input auto-switch (uses `excite` as the sidechain signal
//! when present, falls back to metering `audio` itself when it isn't) and
//! an optional soft-knee / adaptive-makeup-gain mode.

use crate::consts::K_UNITY_GAIN;
use crate::resources::{LUT_EXP2, LUT_LOG2, LUT_SOFT_KNEE};

/// `1 / (1.55 / 6.0 * 65536.0 / 256.0) * 65536`, truncated to `int32_t` at
/// compile time in the C++ (256 LSB <=> 1.55 dB).
const K_GAIN_CONSTANT: i32 = 990;

#[derive(Debug, Clone, Copy, Default)]
pub struct Compressor {
    ratio: i32, // Reciprocal of the ratio, 8:8.
    threshold: i32,
    makeup_gain: i32,

    soft_knee: bool,

    attack_coefficient: i64,
    decay_coefficient: i64,
    detector: i64,
    sidechain_signal_detector: i64,
    gain_reduction: i32,
}

impl Compressor {
    pub fn init(&mut self) {
        self.detector = 0;
    }

    pub fn gain_reduction(&self) -> i32 {
        self.gain_reduction
    }

    pub fn log2(value: i32) -> i32 {
        let mut value = if value <= 0 { 1 } else { value };
        let mut log_value: i32 = 0;
        while value >= 512 {
            value >>= 1;
            log_value = log_value.wrapping_add(65536);
        }
        while value < 256 {
            value <<= 1;
            log_value = log_value.wrapping_sub(65536);
        }
        // Value is between 256 and 512, we can use the LUT.
        log_value.wrapping_add(LUT_LOG2[(value - 256) as usize] as i32)
    }

    /// Not called by `compress` (only `log2` is) -- dead code in the C++
    /// too, kept here as a public utility since `log2`'s natural
    /// counterpart is otherwise useful to have.
    pub fn exp2(value: i32) -> i32 {
        let mut num_shifts: i32 = 0;
        let mut value = value;
        while value >= 65536 {
            num_shifts += 1;
            value -= 65536;
        }
        while value < 0 {
            num_shifts -= 1;
            value += 65536;
        }

        // Value is between 0 and 65535, we can use the LUT.
        let idx = (value >> 8) as usize;
        let a = LUT_EXP2[idx] as i32;
        let b = LUT_EXP2[idx + 1] as i32;
        let mantissa = a.wrapping_add(b.wrapping_sub(a).wrapping_mul(value & 0xff) >> 8);
        if num_shifts >= 0 {
            mantissa.wrapping_shl(num_shifts as u32)
        } else {
            mantissa.wrapping_shr((-num_shifts) as u32)
        }
    }

    fn compress(squared_level: i32, threshold: i32, ratio: i32, soft_knee: bool) -> i32 {
        let level = (Self::log2(squared_level) >> 1) - 15 * 65536; // 15-bit peak
        let position = level.wrapping_sub(threshold);

        if position < 0 {
            return 0;
        }

        let mut attenuation = position.wrapping_sub(position.wrapping_mul(ratio) >> 8);
        if attenuation < 65535 && soft_knee {
            // `attenuation >> 8` isn't provably non-negative for every
            // `ratio`/`threshold` combination the public `configure` API can
            // produce -- clamp defensively rather than prove it exhaustively.
            let idx = (attenuation >> 8).clamp(0, LUT_SOFT_KNEE.len() as i32 - 2) as usize;
            let a = LUT_SOFT_KNEE[idx] as i32;
            let b = LUT_SOFT_KNEE[idx + 1] as i32;
            let soft_knee_value = a.wrapping_add(b.wrapping_sub(a).wrapping_mul(attenuation & 0xff) >> 8);
            attenuation = attenuation.wrapping_add(soft_knee_value.wrapping_sub(attenuation).wrapping_mul((65535 - attenuation) >> 1) >> 15);
        }
        attenuation.wrapping_neg()
    }

    pub fn process(&mut self, audio: i16, excite: i16, gain: &mut u16, frequency: &mut u16) {
        // Detect the RMS level on the EXCITE input.
        let mut energy: i32 = excite as i32;
        energy = energy.wrapping_mul(energy);
        let mut error: i64 = (energy as i64).wrapping_sub(self.sidechain_signal_detector);
        if error > 0 {
            self.sidechain_signal_detector = self.sidechain_signal_detector.wrapping_add(error);
        } else {
            // Decay time: 5s.
            self.sidechain_signal_detector = self.sidechain_signal_detector.wrapping_add(error.wrapping_mul(14174) >> 31);
        }

        // If there is no signal on the "excite" input, disable sidechain and
        // compress by metering input.
        if self.sidechain_signal_detector < (1024 * 1024) {
            energy = audio as i32;
            energy = energy.wrapping_mul(energy);
        }

        // Detect the RMS level on the EXCITE or AUDIO input - whichever active.
        error = (energy as i64).wrapping_sub(self.detector);
        if error > 0 {
            if self.attack_coefficient == -1 {
                self.detector = self.detector.wrapping_add(error);
            } else {
                self.detector = self.detector.wrapping_add(error.wrapping_mul(self.attack_coefficient) >> 31);
            }
        } else {
            self.detector = self.detector.wrapping_add(error.wrapping_mul(self.decay_coefficient) >> 31);
        }

        // Narrows the 64-bit detector to 32 bits, same as the C++'s implicit
        // conversion at this call (`Compress(int32_t squared_level, ...)`
        // called with an `int64_t` argument).
        let mut g = Self::compress(self.detector as i32, self.threshold, self.ratio, self.soft_knee);
        self.gain_reduction = g >> 3;
        g = K_UNITY_GAIN.wrapping_add((g.wrapping_add(self.makeup_gain)).wrapping_mul(K_GAIN_CONSTANT) >> 16);
        if g > 65535 {
            g = 65535;
        }

        *gain = g as u16;
        *frequency = 65535;
    }

    pub fn configure(&mut self, alternate: bool, parameters: &[i32; 2], globals: Option<&[i32; 4]>) {
        let attack_time: i32;
        let decay_time: i32;
        let threshold: i32;
        let mut amount: i32;

        if let Some(globals) = globals {
            attack_time = globals[0].wrapping_mul(128 + 128 + 99) >> 16; // 1ms to 500ms
            decay_time = 128 + 99 + (globals[2] >> 8); // 50ms to 5000ms
            threshold = globals[1];
            amount = globals[3];
        } else {
            attack_time = if !alternate { 1 } else { 40 }; // 0.2ms or 2ms;
            decay_time = if !alternate { 279 } else { 236 }; // 150ms or 70ms;
            threshold = parameters[0];
            amount = parameters[1];
        }

        self.attack_coefficient = crate::resources::LUT_LP_COEFFICIENTS[attack_time as usize] as i64;
        self.decay_coefficient = crate::resources::LUT_LP_COEFFICIENTS[decay_time as usize] as i64;
        self.soft_knee = alternate;
        self.threshold = (-1280 + 5 * (threshold >> 8)) << 8;

        if amount < 32768 {
            // Compression with no makeup gain.
            self.ratio = crate::resources::LUT_COMPRESSOR_RATIO[((32767 - amount) >> 7) as usize] as i32;
            self.makeup_gain = 0;
        } else {
            // Adaptive compression with makeup gain.
            amount -= 32768;

            let max_gain = crate::consts::K_MAX_EXPONENTIAL_GAIN;
            self.makeup_gain = amount.wrapping_mul(max_gain >> 8) >> 7;
            let mut knee_gain = self.threshold + self.makeup_gain;
            if knee_gain >= 0 {
                self.makeup_gain = -self.threshold;
                knee_gain = 0;
            }

            if knee_gain > -4096 {
                // So intense! Brickwall limiter mode. In this case, we use an
                // instant attack to tame transients as soon as they appear.
                self.ratio = 0;
                self.attack_coefficient = -1;
            } else {
                self.ratio = knee_gain / (self.threshold >> 8);
            }
        }
    }
}
