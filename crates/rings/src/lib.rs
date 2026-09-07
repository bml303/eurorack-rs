//! Rust port of Mutable Instruments **Rings** -- a modal / sympathetic-string
//! resonator.
//!
//! # Fidelity
//!
//! Rings runs on a hardware-FPU STM32F373, so -- like `mi-plaits`, `mi-clouds`
//! and `mi-elements` -- the DSP is floating-point and there is **no
//! bit-exactness contract**. The port is idiomatic Rust. The genuinely
//! integer-exact pieces (the FM operator phase words, the ensemble/chorus LFO
//! table indices) are still translated verbatim with `wrapping_*`.
//!
//! # Scope
//!
//! The sound engine only -- `cv_scaler`, `ui`, `settings` and the STM32
//! peripheral drivers stay in the C repo. Two top-level types:
//!
//! * [`Part`] -- the resonator, six models (modal, sympathetic string, string,
//!   FM voice, quantised sympathetic string, string + reverb), 1-4 voice
//!   polyphony, internal exciter ([`Plucker`]) and strum detection
//!   ([`Strummer`]).
//! * [`StringSynthPart`] -- the "Disastrous Peace" easter egg: a polyphonic
//!   PolyBLEP string-ensemble / organ with formant, chorus, ensemble and reverb
//!   effects.
//!
//! A host feeds [`Part::process`] / [`StringSynthPart::process`] blocks of at
//! most [`MAX_BLOCK_SIZE`] samples of audio input plus a [`PerformanceState`]
//! and a [`Patch`], and gets a stereo pair back.

#![no_std]
#![allow(clippy::excessive_precision)]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::needless_range_loop)]

extern crate alloc;

pub mod dsp;
pub mod fm_voice;
pub mod follower;
pub mod fx;
pub mod limiter;
pub mod note_filter;
pub mod onset_detector;
pub mod part;
pub mod plucker;
pub mod resonator;
pub mod resources;
pub mod string;
pub mod string_synth_envelope;
pub mod string_synth_oscillator;
pub mod string_synth_part;
pub mod string_synth_voice;
pub mod strummer;

pub use dsp::{A3, MAX_BLOCK_SIZE, SAMPLE_RATE};
pub use fm_voice::FmVoice;
pub use limiter::Limiter;
pub use note_filter::NoteFilter;
pub use part::{NUM_CHORDS, Part, Patch, PerformanceState, ResonatorModel};
pub use plucker::Plucker;
pub use resonator::Resonator;
pub use string::String;
pub use string_synth_part::{FxType, StringSynthPart};
pub use strummer::Strummer;

/// Set once the DSP is ported; kept for parity with the other scaffold crates.
pub const PORTED: bool = true;
