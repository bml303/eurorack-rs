//! `plaits/dsp/fm/dx_units.h` -- the "magic number" conversions from raw DX7
//! patch bytes (0-99 envelope rates/levels, 0-31 coarse ratios, ...) to the
//! floating-point quantities the rest of `fm` renders with.

#![allow(clippy::excessive_precision)]

use crate::utils::interpolate;
use crate::utils::units::semitones_to_ratio_safe;

use super::patch::{KeyboardScaling, Operator};

/// Computes `2^x` via a polynomial approximation of `2^frac(x)`, then shifts
/// that result's IEEE-754 exponent by `int(x)` directly -- cheaper than
/// `libm::exp2f` and accurate enough for envelope/pitch curves.
///
/// `ORDER` selects the approximation: `1` builds the result's bit pattern
/// directly (`(1 << 23) * (127 + x)` as a *float value*, not a bit-cast,
/// lands `int(127 + x)` exactly in the exponent field and the fractional
/// part as a linear ramp across the mantissa's top bit -- no separate
/// exponent-shift step needed); anything else uses the quadratic polynomial
/// approximation of `2^frac(x)`, exponent-shifted by `int(x)`. The C also
/// has a cubic (`ORDER == 3`) variant, never actually instantiated anywhere
/// in `fm` (every call site uses `1` or `2`), so it's dropped here.
#[inline]
pub fn pow_2_fast<const ORDER: i32>(x: f32) -> f32 {
    if ORDER == 1 {
        let bit_pattern = ((1u32 << 23) as f32 * (127.0 + x)) as i32;
        return f32::from_bits(bit_pattern as u32);
    }

    let mut x = x;
    let mut exponent = x as i32;
    if x < 0.0 {
        exponent -= 1;
    }
    x -= exponent as f32;

    let mantissa: f32 = 1.0 + x * (0.6565 + x * 0.3435);
    f32::from_bits((mantissa.to_bits() as i32).wrapping_add(exponent << 23) as u32)
}

/// Converts an operator envelope level (0-99) to the complement of the "TL"
/// (total level) value the hardware's log-domain envelope actually ramps:
/// `0 -> 0` (TL 127), `20 -> 48` (TL 79), `50 -> 78` (TL 49), `99 -> 127` (TL 0).
#[inline]
pub fn operator_level(level: u8) -> u8 {
    let level = level as u32;
    (if level < 20 {
        if level < 15 {
            (level * (36 - level)) >> 3
        } else {
            27 + level
        }
    } else {
        level + 28
    }) as u8
}

/// Converts a pitch-envelope level (0-99) to an octave shift: `0 = -4oct`,
/// `18 = -1oct`, `50 = 0`, `82 = +1oct`, `99 = +4oct`.
#[inline]
pub fn pitch_envelope_level(level: u8) -> f32 {
    let l = (level as f32 - 50.0) / 32.0;
    let tail = (f32::abs(l + 0.02) - 1.0).max(0.0);
    l * (1.0 + tail * tail * 5.3056)
}

/// Converts an operator envelope rate (0-99) to a per-sample increment.
#[inline]
pub fn operator_envelope_increment(rate: u8) -> f32 {
    let rate_scaled = (rate as i32 * 41) >> 6;
    let mantissa = 4 + (rate_scaled & 3);
    let exponent = 2 + (rate_scaled >> 2);
    (mantissa << exponent) as f32 / (1 << 24) as f32
}

/// Converts a pitch-envelope rate (0-99) to a per-sample increment.
#[inline]
pub fn pitch_envelope_increment(rate: u8) -> f32 {
    let r = rate as f32 * 0.01;
    (1.0 + 192.0 * r * (r * r * r * r + 0.3333)) / (21.3 * 44100.0)
}

const MIN_LFO_FREQUENCY: f32 = 0.005865;

/// Converts an LFO rate (0-99) to a frequency (Hz).
#[inline]
pub fn lfo_frequency(rate: u8) -> f32 {
    let mut rate_scaled = if rate == 0 { 1 } else { (rate as u32 * 165) >> 6 };
    rate_scaled *= if rate_scaled < 160 {
        11
    } else {
        11 + ((rate_scaled - 160) >> 4)
    };
    rate_scaled as f32 * MIN_LFO_FREQUENCY
}

/// Converts an LFO delay (0-99) to the fade-in's two ramp segment rates.
#[inline]
pub fn lfo_delay(delay: u8) -> [f32; 2] {
    if delay == 0 {
        return [100_000.0; 2];
    }
    let d = 99 - delay as i32;
    let d = (16 + (d & 15)) << (1 + (d >> 4));
    [
        d as f32 * MIN_LFO_FREQUENCY,
        i32::max(0x80, d & 0xff80) as f32 * MIN_LFO_FREQUENCY,
    ]
}

/// Pre-warps a linear velocity (`0..1`) into the perceptual curve DX7
/// velocity scaling expects.
///
/// The C looks this up as `Interpolate(lut_cube_root, velocity, 16)` with no
/// clamp -- `velocity == 1.0` reads `lut_cube_root[16 + 1]`, one past that
/// 17-entry table. This port clamps just below the top entry instead of
/// reproducing the out-of-bounds read (compare the `mi-tides`
/// `lut_cutoff`/`WaveLine`-in-`mi-braids` precedent).
#[inline]
pub fn normalize_velocity(velocity: f32) -> f32 {
    let cube_root = interpolate(&LUT_CUBE_ROOT, velocity.min(1.0 - f32::EPSILON), 16.0);
    16.0 * (cube_root - 0.918)
}

/// MIDI note number to envelope-rate scaling multiplier.
#[inline]
pub fn rate_scaling(note: f32, rate_scaling: u8) -> f32 {
    pow_2_fast::<1>(rate_scaling as f32 * (note * 0.33333 - 7.0) * 0.03125)
}

/// Operator amplitude-modulation sensitivity (0-3).
#[inline]
pub fn amp_mod_sensitivity(amp_mod_sensitivity: u8) -> f32 {
    LUT_AMP_MOD_SENSITIVITY[amp_mod_sensitivity as usize]
}

/// LFO pitch-modulation sensitivity (0-7).
#[inline]
pub fn pitch_mod_sensitivity(pitch_mod_sensitivity: u8) -> f32 {
    LUT_PITCH_MOD_SENSITIVITY[pitch_mod_sensitivity as usize]
}

/// Keyboard-tracking level adjustment (in TL units) at `note`.
#[inline]
pub fn keyboard_scaling(note: f32, ks: &KeyboardScaling) -> f32 {
    let x = note - ks.break_point as f32 - 15.0;
    let curve = if x > 0.0 { ks.right_curve } else { ks.left_curve };

    let mut t = f32::abs(x);
    if curve == 1 || curve == 2 {
        t = (t * 0.010467).min(1.0);
        t = t * t * t * 96.0;
    }
    if curve < 2 {
        t = -t;
    }

    let depth = (if x > 0.0 { ks.right_depth } else { ks.left_depth }) as f32;
    t * depth * 0.02677
}

/// An operator's frequency ratio (or, for a fixed-frequency operator,
/// absolute frequency encoded as a *negative* ratio -- see [`super::voice`]).
#[inline]
pub fn frequency_ratio(op: &Operator) -> f32 {
    let detune = if op.mode == 0 && op.fine != 0 {
        1.0 + 0.01 * op.fine as f32
    } else {
        1.0
    };

    let mut base = if op.mode == 0 {
        LUT_COARSE[op.coarse as usize]
    } else {
        ((op.coarse & 3) as i32 * 100 + op.fine as i32) as f32 * 0.39864
    };
    base += (op.detune as f32 - 7.0) * 0.015;

    semitones_to_ratio_safe(base) * detune
}

const LUT_COARSE: [f32; 32] = [
    -12.000000, 0.000000, 12.000000, 19.019550, 24.000000, 27.863137, 31.019550, 33.688259,
    36.000000, 38.039100, 39.863137, 41.513180, 43.019550, 44.405276, 45.688259, 46.882687,
    48.000000, 49.049554, 50.039100, 50.975130, 51.863137, 52.707809, 53.513180, 54.282743,
    55.019550, 55.726274, 56.405276, 57.058650, 57.688259, 58.295772, 58.882687, 59.450356,
];

const LUT_AMP_MOD_SENSITIVITY: [f32; 4] = [0.0, 0.2588, 0.4274, 1.0];

const LUT_PITCH_MOD_SENSITIVITY: [f32; 8] =
    [0.0, 0.0781250, 0.1562500, 0.2578125, 0.4296875, 0.7187500, 1.1953125, 2.0];

const LUT_CUBE_ROOT: [f32; 17] = [
    0.0, 0.39685062976, 0.50000000000, 0.57235744065, 0.62996081605, 0.67860466725,
    0.72112502092, 0.75914745216, 0.79370070937, 0.82548197054, 0.85498810729, 0.88258719406,
    0.90856038354, 0.93312785379, 0.95646563396, 0.97871693135, 1.0,
];
