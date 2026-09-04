//! Rust port of Mutable Instruments **Plaits** -- a macro oscillator with 24
//! synthesis models, spanning classic analog waveforms, FM, physical
//! modelling, granular synthesis, noise and (eventually) speech/FM-8op voices.
//!
//! # Status
//!
//! Plaits runs on an STM32F3 (Cortex-M4F, hardware FPU) entirely in floating
//! point, so unlike `braids` there is no fixed-point bit-exactness to
//! preserve -- the port follows normal idiomatic-Rust judgment: methods
//! instead of free functions, runtime parameters instead of template
//! parameters (the C uses templates for compile-time specialisation/code size
//! on a Cortex-M, which doesn't apply to a portable library), `Option`/slices
//! instead of nullable pointers.
//!
//! 22 of the 24 engine models are ported; `SixOpEngine` and `SpeechEngine`
//! are documented silent stubs. See `PORTING.md` for the full status.
#![no_std]
#![allow(
    clippy::too_many_arguments,
    clippy::excessive_precision,
    clippy::needless_range_loop,
    // A few oscillators (`VariableShapeOscillator`, `SuperSquareOscillator`)
    // use the C's `while (transition_during_reset || !reset) { ... break; }`
    // BLEP loop verbatim -- the loop variables are flipped by field writes
    // the lint doesn't see as "mutating the condition", not by reassigning
    // the locals themselves.
    clippy::while_immutable_condition
)]

pub mod chords;
pub mod downsampler;
pub mod drums;
pub mod dsp;
pub mod engine;
pub mod engines;
pub mod envelope;
pub mod fx;
pub mod noise;
pub mod oscillator;
pub mod physical_modelling;
pub mod resources;
pub mod voice;

pub use engine::{Engine, EngineParameters, PostProcessingSettings};
pub use voice::{Frame, Modulations, Patch, Voice};
