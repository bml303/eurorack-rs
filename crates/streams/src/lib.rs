//! Rust port of Mutable Instruments **Streams** -- a dual dynamics
//! processor: [`processor::Processor`] dispatches to one of 6 algorithms
//! (envelope, vactrol VCA/VCF model, envelope follower, compressor, plain
//! filter, Lorenz-attractor chaotic generator), each turning an
//! audio/excite pair into a gain/frequency pair for the analog VCA/VCF that
//! follows it in hardware.
//!
//! # Scope
//!
//! `processor.{h,cc}`, `envelope.{h,cc}`, `vactrol.{h,cc}`,
//! `follower.{h,cc}`, `compressor.{h,cc}`, `filter_controller.h`,
//! `lorenz_generator.{h,cc}`, `svf.{h,cc}`, `meta_parameters.h`, `gain.h`
//! and `audio_cv_meter.h` are ported in full. Out of scope, per this
//! workspace's DSP-library-only rule: `streams.cc` (the app-level ADC/DAC/
//! UI main loop), `cv_scaler.{h,cc}` (front-panel CV scaling), `ui.{h,cc}`
//! (the UI state machine), and the STM32 peripheral drivers.
//!
//! # Status
//!
//! Fixed-point (STM32F105), `mi-braids`-style verbatim arithmetic --
//! several algorithms (`Vactrol`, `Follower`, `Compressor`) do real
//! intermediate arithmetic in 64-bit, matching the C++'s explicit
//! `int64_t` casts exactly, including the points where the 64-to-32-bit
//! narrowing on assignment back into a 32-bit state variable is load-
//! bearing (not just a formality). No C bit-compare harness for this
//! module; `tests/smoke.rs` exercises `Processor` across every
//! `ProcessorFunction`, `alternate`/`linked` combination, and a long
//! parameter/global sweep.
#![no_std]

pub mod audio_cv_meter;
pub mod compressor;
pub mod consts;
pub mod envelope;
pub mod filter_controller;
pub mod follower;
pub mod lorenz_generator;
pub mod meta_parameters;
pub mod processor;
pub mod resources;
pub mod svf;
pub mod vactrol;

pub use audio_cv_meter::AudioCvMeter;
pub use compressor::Compressor;
pub use envelope::{Envelope, EnvelopeShape};
pub use filter_controller::FilterController;
pub use follower::Follower;
pub use lorenz_generator::LorenzGenerator;
pub use processor::{Processor, ProcessorFunction};
pub use svf::Svf;
pub use vactrol::Vactrol;

pub const PORTED: bool = true;
