//! Rust port of Mutable Instruments **Warps** -- a meta-modulator: 6
//! cross-modulation algorithms (crossfade, wavefolder, analog/digital ring
//! modulation, XOR, comparator) at x6 oversampling, a 20-band vocoder, and a
//! frequency-shifter easter egg, all driven by [`modulator::Modulator`].
//!
//! # Scope
//!
//! `dsp/*.{h,cc}` (`modulator`, `oscillator`, `quadrature_oscillator`,
//! `quadrature_transform`, `sample_rate_converter` +
//! `sample_rate_conversion_filters`, `filter_bank`, `vocoder`, `limiter`,
//! `parameters`) are ported in full. Out of scope, per this workspace's
//! DSP-library-only rule: `warps.cc` (the app-level ADC/DAC/UI main loop),
//! `cv_scaler.{h,cc}` / `meter.h` (front-panel CV scaling and metering),
//! `settings.{h,cc}` / `ui.{h,cc}` (persisted settings and the UI state
//! machine) -- see `PORTING.md` for the full source inventory and the
//! per-module deviations from the C++.
//!
//! # Status
//!
//! Floating-point (STM32F3), idiomatic Rust like `mi-rings`/`mi-clouds`/
//! `mi-elements` (no bit-exactness contract). No C bit-compare harness for
//! this module; `tests/smoke.rs` exercises `Modulator` across the
//! cross-modulation/vocoder/easter-egg paths and the sample-rate
//! converter/oscillator/filter-bank building blocks.
#![no_std]
#![allow(clippy::excessive_precision)]

pub mod filter_bank;
pub mod limiter;
pub mod modulator;
pub mod oscillator;
pub mod parameters;
pub mod quadrature_oscillator;
pub mod quadrature_transform;
pub mod resources;
pub mod sample_rate_converter;
pub mod vocoder;

pub use modulator::{Modulator, ShortFrame};
pub use oscillator::Oscillator;
pub use parameters::{OscillatorShape, Parameters};
pub use quadrature_oscillator::QuadratureOscillator;
pub use quadrature_transform::QuadratureTransform;
pub use vocoder::Vocoder;

pub const PORTED: bool = true;
