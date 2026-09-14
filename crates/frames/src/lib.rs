//! Rust port of Mutable Instruments **Frames** -- a keyframer/mixer:
//! [`keyframer::Keyframer`] interpolates a sorted list of up to 64
//! 4-channel keyframes with a per-channel easing curve and response
//! (linear/exponential VCA blend), plus [`poly_lfo::PolyLfo`], a 4-channel
//! wavetable LFO easter egg with adjustable phase spread and coupling.
//!
//! # Scope
//!
//! `keyframer.{h,cc}` and `poly_lfo.{h,cc}` are ported in full. Out of
//! scope, per this workspace's DSP-library-only rule: `frames.cc` (the
//! app-level ADC/DAC/UI main loop), `ui.{h,cc}` (the UI state machine), the
//! STM32 peripheral drivers, and flash persistence (`Keyframer::Save`'s
//! `stmlib::Storage` write -- `set_extra_settings`/`calibrate` update the
//! in-memory fields without it, same as every other crate's dropped
//! settings layer).
//!
//! # Status
//!
//! Fixed-point (STM32F1), `mi-braids`-style verbatim arithmetic
//! (`wrapping_*` where the C relies on 32-bit int overflow/truncation).
//! No C bit-compare harness for this module; `tests/smoke.rs` exercises
//! `Keyframer` (a full 64-keyframe timeline, every easing curve, evaluated
//! at and beyond every boundary) and `PolyLfo` (every spread/coupling/shape
//! combination over a long frequency sweep).
#![no_std]

pub mod keyframer;
pub mod poly_lfo;
pub mod resources;

pub use keyframer::{ChannelSettings, EasingCurve, Keyframe, Keyframer};
pub use poly_lfo::PolyLfo;

pub const PORTED: bool = true;
