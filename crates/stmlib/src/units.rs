//! `stmlib/dsp/units.h` -- semitones to frequency ratio via a split lookup table.
//!
//! The C tables are declared `[257]` but only 256 entries are initialised, so
//! C reads a trailing implicit `0.0`. [`ratio_high`] / [`ratio_low`] reproduce
//! that (out-of-range index -> `0.0`) to stay bit-identical at the extremes.

use crate::units_lut::{LUT_PITCH_RATIO_HIGH, LUT_PITCH_RATIO_LOW};

#[inline]
fn ratio_high(i: usize) -> f32 {
    LUT_PITCH_RATIO_HIGH.get(i).copied().unwrap_or(0.0)
}

#[inline]
fn ratio_low(i: usize) -> f32 {
    LUT_PITCH_RATIO_LOW.get(i).copied().unwrap_or(0.0)
}

/// `SemitonesToRatio(semitones)` -- valid for roughly `[-128, +128]` semitones.
#[inline]
pub fn semitones_to_ratio(semitones: f32) -> f32 {
    let pitch = semitones + 128.0;
    let integral = pitch as i32;
    let fractional = pitch - integral as f32;
    ratio_high(integral as usize) * ratio_low((fractional * 256.0) as i32 as usize)
}

/// `SemitonesToRatioSafe(semitones)` -- folds the argument into the table's
/// range and rescales, so arbitrarily large intervals work.
#[inline]
pub fn semitones_to_ratio_safe(mut semitones: f32) -> f32 {
    let mut scale = 1.0f32;
    while semitones > 120.0 {
        semitones -= 120.0;
        scale *= 1024.0;
    }
    while semitones < -120.0 {
        semitones += 120.0;
        scale *= 1.0 / 1024.0;
    }
    scale * semitones_to_ratio(semitones)
}

/// `Exp2Safe(value)`.
#[inline]
pub fn exp2_safe(value: f32) -> f32 {
    semitones_to_ratio_safe(value * 12.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unison_and_octave() {
        assert!((semitones_to_ratio(0.0) - 1.0).abs() < 1e-4);
        assert!((semitones_to_ratio(12.0) - 2.0).abs() < 1e-3);
        assert!((semitones_to_ratio(-12.0) - 0.5).abs() < 1e-3);
    }
}
