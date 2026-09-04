//! Idiomatic Rust port of [`stmlib`](https://github.com/pichenettes/stmlib), the
//! DSP / utility library shared by every Mutable Instruments Eurorack module.
//!
//! # Fidelity
//!
//! The fixed-point routines ([`fixed`]) preserve the *exact* integer arithmetic
//! of the C originals -- shift amounts, truncation and wrap-around all match, so
//! output is bit-identical to the firmware. Wrapping is explicit
//! ([`i32::wrapping_add`] & friends) because the C relies on 2's-complement
//! overflow that Rust would otherwise trap in debug builds.
//!
//! The floating-point helpers ([`fdsp`]) and the higher-level building blocks
//! ([`ParameterInterpolator`], [`CosineOscillator`], ...) are translated to
//! idiomatic Rust: methods instead of free functions, `Option` instead of null
//! pointers, iterators where they read cleanly.
//!
//! Everything here is `#![no_std]`; nothing allocates.
#![no_std]
#![allow(clippy::excessive_precision)]

pub mod cosine_oscillator;
pub mod fdsp;
pub mod fixed;
pub mod gate_flags;
pub mod parameter_interpolator;
pub mod random;
pub mod units;

mod units_lut;

pub use cosine_oscillator::{CosineOscillator, CosineOscillatorMode};
pub use fixed::{
    crossfade, crossfade_u8, interpolate_1022, interpolate_115, interpolate_824_i16,
    interpolate_824_u16, interpolate_824_u8, interpolate_88_i16, interpolate_88_u16, mix_i16,
    mix_u16,
};
pub use parameter_interpolator::ParameterInterpolator;
pub use random::Random;

/// `CLIP(x)` from `stmlib.h`: clamp to the 16-bit *signed* range, but using the
/// asymmetric bound `[-32767, 32767]` that the original macro uses.
#[inline]
pub fn clip16_sym(x: i32) -> i32 {
    x.clamp(-32767, 32767)
}

/// `CONSTRAIN(var, min, max)` from `stmlib.h`.
#[inline]
pub fn constrain<T: PartialOrd>(value: T, min: T, max: T) -> T {
    if value < min {
        min
    } else if value > max {
        max
    } else {
        value
    }
}
