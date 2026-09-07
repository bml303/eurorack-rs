//! Rust port of Mutable Instruments **Elements** -- a modal / physical-modelling
//! synthesizer voice.
//!
//! # Fidelity
//!
//! Elements runs on an STM32F4 with a hardware FPU, so -- like `mi-plaits` and
//! `mi-clouds` -- the DSP is floating-point and there is **no bit-exactness
//! contract**. The port is ordinary idiomatic Rust: methods and enums instead of
//! function-pointer tables, `match` instead of template specialisation, slices
//! instead of raw pointer + size pairs. The genuinely integer-exact pieces
//! (oscillator phase accumulators, the granular sample-player read indices, the
//! FM operator phase words) are still translated verbatim with `wrapping_*`.
//!
//! # Scope
//!
//! This crate is the sound engine only -- the `drivers/`, `cv_scaler`, `ui` and
//! bootloader of the firmware stay in the C repo. A host feeds
//! [`Part::process`] blocks of at most [`MAX_BLOCK_SIZE`] samples plus the two
//! audio-rate excitation inputs (`blow_in`, `strike_in`) and gets a stereo pair
//! (`main`, `aux`) back.
//!
//! # Allocation
//!
//! The delay lines and the 64 KB reverb buffer are heap-allocated
//! (`extern crate alloc`) so a [`Part`] can be moved cheaply and does not blow a
//! small embedded stack. Build a `Part` once (behind a `Box` if your target is
//! tight).

#![no_std]
#![allow(clippy::excessive_precision)]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::needless_range_loop)]

extern crate alloc;

pub mod dsp;
pub mod exciter;
pub mod fx;
pub mod multistage_envelope;
pub mod ominous_voice;
pub mod part;
pub mod resonator;
pub mod resources;
pub mod string;
pub mod tube;
pub mod voice;

pub use dsp::{MAX_BLOCK_SIZE, SAMPLE_RATE};
pub use exciter::{Exciter, ExciterModel};
pub use ominous_voice::OminousVoice;
pub use part::{Part, PerformanceState};
pub use resonator::Resonator;
pub use string::String;
pub use voice::{ResonatorModel, Voice};

/// Set once the DSP is ported; kept for parity with the other scaffold crates.
pub const PORTED: bool = true;
