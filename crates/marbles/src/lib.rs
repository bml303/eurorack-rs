//! Rust port of Mutable Instruments **Marbles** -- a random sampler/CV
//! generator: a "T" (trigger/gate) section producing 2 channels of
//! pseudo-random gate patterns synced to an internal or external clock, and
//! an "X/Y" section producing 4 channels of quantized/unquantized random
//! voltages.
//!
//! # Scope
//!
//! This crate ports the generator engine itself --
//! [`random::TGenerator`]/[`random::XYGenerator`] and their supporting
//! `ramp`/`random` primitives -- not the full firmware application. Per this
//! workspace's DSP-library-only scope (see the top-level `CLAUDE.md`),
//! `settings`/`ui`/`drivers`/the bootloader and the ADC-scaling `cv_reader`
//! are all out of scope: a host is expected to feed the generators
//! already-scaled control values (as the front panel's pots/CV inputs would,
//! post-`cv_reader`) and already-derived [`random::GroupSettings`] (as
//! `marbles.cc`'s `Process()` builds them from the persisted `Settings::State`
//! before calling into `XYGenerator`), the same way `mi-plaits`' engines take
//! an already-built `EngineParameters` rather than raw ADC codes.
//! `note_filter`, `clock_self_patching_detector` and `scale_recorder` (all
//! small, self-contained, non-hardware-specific helpers) are left as future
//! work -- see `PORTING.md`.
//!
//! # Status
//!
//! Floating-point (STM32F3 FPU), idiomatic Rust like `mi-plaits`/`mi-clouds`.
//! The PRNG (`RandomGenerator`'s LCG) is integer-exact (`wrapping_*`); the
//! rest of the module has no bit-exactness contract to preserve. No C
//! bit-compare harness (float, and `marbles_test.cc` drives real audio-rate
//! hardware capture) -- see `PORTING.md`.
#![no_std]
#![allow(
    clippy::too_many_arguments,
    clippy::excessive_precision,
    clippy::needless_range_loop
)]

pub mod ramp;
pub mod random;
pub mod resources;

pub use random::{
    ClockSource, ControlMode, GroupSettings, RandomGenerator, RandomStream, Ramps, Scale,
    TGenerator, TGeneratorModel, TGeneratorRange, VoltageRange, XYGenerator,
};

pub const PORTED: bool = true;
