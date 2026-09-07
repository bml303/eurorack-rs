//! Rust port of Mutable Instruments **Edges** -- a quad chiptune oscillator.
//!
//! Edges has five oscillators: four [`TimerOscillator`]s (hardware dual-slope
//! PWM square waves, on separate outputs -- one of them a hidden `/16`
//! sub-oscillator used to build an NES-style triangle) and one sampled
//! [`DigitalOscillator`] (band-limited triangle, NES triangle, three noise
//! flavours and a bit-crushed sine) feeding the main audio DAC.
//!
//! # Fidelity
//!
//! Edges runs on an 8-bit AVR (ATxmega), so -- like [`mi-braids`] -- the
//! arithmetic is fixed-point and reproduced **verbatim**: 24-bit phase
//! accumulators, `u8`/`u16` wrap, the exact table-lookup interpolation the AVR
//! inline-asm `InterpolateSample` performs (index `phase >> 7` into the
//! 513-byte wavetables, an 8-bit even blend weight). Output samples are
//! bit-identical to the firmware's `DigitalOscillator::Render`.
//!
//! [`mi-braids`]: https://docs.rs/mi-braids
//!
//! # Scope
//!
//! The sound engine only: the MIDI stack, `note_stack`, `voice_allocator`,
//! `settings` (flash), `ui` and the AVR peripheral drivers stay in the C repo.
//! [`DigitalOscillator::render`] fills a buffer of 12-bit unsigned samples;
//! [`TimerOscillator`] computes the firmware's timer `period` / `value` /
//! prescaler, and -- as a port-only convenience the AVR does in hardware --
//! [`TimerOscillator::render_square`] synthesises the square wave from them.

#![no_std]
#![allow(clippy::excessive_precision)]

pub mod digital_oscillator;
pub mod resources;
pub mod timer_oscillator;

pub use digital_oscillator::{AUDIO_BLOCK_SIZE, DigitalOscillator, OscillatorShape};
pub use timer_oscillator::{PulseWidth, TimerOscillator, TimerPrescaler};

/// Set once the DSP is ported; kept for parity with the other scaffold crates.
pub const PORTED: bool = true;
