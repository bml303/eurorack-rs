//! Rust port of Mutable Instruments **Stages** -- a 6-channel segment
//! generator (envelopes, LFOs, step sequencers, sample & hold, portamento,
//! clocked delay, audio-rate oscillator), configurable per-channel and
//! chainable across multiple physical modules.
//!
//! # Scope
//!
//! This crate ports the DSP engine: [`SegmentGenerator`] (the whole point of
//! the module) plus its two small supporting oscillators and the 16-bit
//! delay line it uses for the `Delay` function. Firmware plumbing --
//! peripheral drivers (`drivers/`), the audio bootloader, `settings.{h,cc}`
//! (flash-backed calibration storage), `cv_reader.{h,cc}` (wraps the ADC
//! drivers), `chain_state.{h,cc}` (the inter-module serial-link protocol:
//! discovery, parameter binding across a physical daisy-chain, UI switch
//! handling -- all of it tied to `SerialLink`/`Settings`, not DSP),
//! `ui.{h,cc}`, `factory_test.{h,cc}` and `stages.cc` (`main`) -- stays in
//! the C repo, as with every other crate here. See `PORTING.md` for the
//! full source inventory and the two real out-of-bounds table reads this
//! port's fidelity check found (and clamps) in the C.
#![no_std]

pub mod delay_line_16_bits;
pub mod oscillator;
pub mod resources;
pub mod segment_generator;
pub mod variable_shape_oscillator;

pub use delay_line_16_bits::DelayLine16Bits;
pub use oscillator::{Oscillator, OscillatorShape};
pub use segment_generator::{segment, Output, SegmentGenerator};
pub use variable_shape_oscillator::VariableShapeOscillator;

pub const PORTED: bool = true;
