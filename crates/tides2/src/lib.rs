//! Rust port of Mutable Instruments **Tides2** -- Tidal modulator (2018).
//!
//! Unlike `mi-tides` (2014, Cortex-M3, fixed-point), Tides2 runs on a
//! Cortex-M4F with hardware FPU and its DSP is written in plain `float`; this
//! port follows the `mi-plaits` fidelity contract (see the workspace
//! `PORTING.md`) -- ordinary idiomatic Rust, no fixed-point/wrapping
//! machinery, and no bit-exactness claim against the C.
//!
//! Ported: [`ramp_generator::RampGenerator`], [`ramp_shaper::RampShaper`] /
//! [`ramp_shaper::RampWaveshaper`], [`ramp_extractor::RampExtractor`], and the
//! top-level [`poly_slope_generator::PolySlopeGenerator`].
//!
//! Out of scope, matching the `mi-braids`/`mi-tides` precedent:
//! `cv_reader*.{h,cc}` (ADC calibration), `factory_test.{h,cc}`,
//! `settings.{h,cc}`, `ui.{h,cc}` and `tides.cc` (hardware wiring/main loop),
//! and the peripheral `drivers/`.
#![no_std]

pub mod poly_slope_generator;
pub mod ramp_extractor;
pub mod ramp_generator;
pub mod ramp_shaper;
pub mod ratio;
pub mod resources;

pub use poly_slope_generator::{OutputSample, PolySlopeGenerator};
pub use ramp_extractor::RampExtractor;
pub use ramp_generator::{OutputMode, RampGenerator, RampMode, Range};
pub use ramp_shaper::{RampShaper, RampWaveshaper};
pub use ratio::Ratio;
