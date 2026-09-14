//! Rust port of Mutable Instruments **Peaks** -- a dual function
//! generator: [`processors::Processors`] dispatches to one of 12 processor
//! functions (multistage envelope, LFO/tap-LFO, 4 drum voices, 2 pulse
//! processors, bouncing ball, mini sequencer, number station), each
//! turning a per-sample [`gate_processor::GateFlags`] stream into an
//! `i16` output stream.
//!
//! # Scope
//!
//! `processors.{h,cc}`, `gate_processor.h` (`ControlMode`; `GateFlags`/
//! `extract_gate_flags` live in `mi-stmlib`, shared with other crates),
//! `calibration_data.{h,cc}`, `drums/*`, `modulations/*`,
//! `pulse_processor/*`, and `number_station/*` are ported in full. Out of
//! scope, per this workspace's DSP-library-only rule: `peaks.cc` (the
//! app-level ADC/DAC/UI main loop), `ui.{h,cc}` (the UI state machine),
//! `io_buffer.h` (the hardware ISR double-buffering scheme -- a host just
//! calls `Processors::process` with whatever block size it likes), the
//! STM32 peripheral drivers, and flash persistence
//! (`CalibrationData::Save`).
//!
//! # A cross-cutting contract: several engines assume `kBlockSize == 4`
//!
//! The firmware always calls `Process` in blocks of exactly 4 samples
//! (`peaks::kBlockSize`, from the now-out-of-scope `IOBuffer`). A few
//! engines bake that block size into their own timing rather than tracking
//! it independently:
//! - [`drums::fm_drum::FmDrum`] recomputes its FM phase increment every 4th
//!   sample, keyed off the *remaining* sample count in the call (not an
//!   independent counter) -- see its module doc comment.
//! - [`number_station::NumberStation`] downsamples its control-rate
//!   processing by exactly 4 and always emits 4 output samples per tick.
//! - [`pulse_processor::pulse_shaper::PulseShaper`] and
//!   [`pulse_processor::pulse_randomizer::PulseRandomizer`] look for a
//!   rising edge anywhere in the whole call and fill the whole output
//!   block with one flat value, so their time resolution is exactly the
//!   caller's block size.
//!
//! None of this panics or misbehaves at other block sizes -- it just means
//! bit-for-bit fidelity to the shipped firmware requires calling `process`
//! in blocks of 4 (or a multiple of 4), same as the real hardware does.
//!
//! # Status
//!
//! Fixed-point (STM32F4), `mi-braids`-style verbatim arithmetic, including
//! several spots where the C++ mixes signed/unsigned or `int16_t`/
//! `int32_t`/`uint32_t` operands and the *exact* width the intermediate
//! arithmetic happens in (not just the final narrowing) is load-bearing --
//! see the module doc comments and `PORTING.md` for specific examples.
//! No C bit-compare harness for this module; `tests/smoke.rs` exercises
//! `Processors` across every `ProcessorFunction`/`ControlMode`/parameter
//! combination, plus each engine individually.
#![no_std]

pub mod calibration_data;
pub mod drums;
pub mod gate_processor;
pub mod modulations;
pub mod number_station;
pub mod processors;
pub mod pulse_processor;
pub mod resources;

pub use calibration_data::CalibrationData;
pub use gate_processor::{ControlMode, GateFlags};
pub use number_station::NumberStation;
pub use processors::{Processors, ProcessorFunction};

pub const PORTED: bool = true;
