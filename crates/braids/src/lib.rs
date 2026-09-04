//! Rust port of Mutable Instruments **Braids** -- a macro-oscillator with ~48
//! synthesis models.
//!
//! # Status
//!
//! This is the workspace's **reference port**. Ported and self-contained:
//!
//! * [`resources`] -- every lookup table, transpiled from the generated C.
//! * [`analog_oscillator`] -- all 9 BLEP waveforms.
//! * [`digital_oscillator`] -- all ~35 digital models.
//! * [`macro_oscillator`] -- the full model router (analog + digital).
//! * [`quantizer`] (+ the 50 built-in scales), [`svf`], [`excitation`],
//!   [`envelope`], [`signature_waveshaper`], [`vco_jitter_source`].
//!
//! Not ported (top-level firmware, out of scope for a DSP library): the STM32F1
//! drivers, `settings.cc` persistence, and `ui.cc`.
//!
//! # Fidelity
//!
//! Braids runs entirely in fixed point on an FPU-less Cortex-M3. This port keeps
//! that integer arithmetic **verbatim** -- identical shifts, truncation and
//! wrap-around -- so a given `(shape, pitch, parameters, sync)` sequence
//! produces bit-identical samples to the firmware. Wrap points that are UB in C
//! (signed overflow, `x / (y >> n)` with a zero divisor) are made explicit with
//! `wrapping_*` / a guarded divide; at real note pitches nothing changes.
//!
//! Only the *structure* is modernised: `match` instead of function-pointer
//! tables, enums instead of bare `int`, the `union` state becomes a flat struct,
//! the `BEGIN/INTERPOLATE/END` parameter macros become small ramp helpers.
//!
//! # Example
//!
//! ```
//! use braids::{MacroOscillator, MacroOscillatorShape};
//!
//! let mut osc = MacroOscillator::new();
//! osc.set_shape(MacroOscillatorShape::SawSquare);
//! osc.set_pitch(60 << 7); // MIDI note 60, 7 fractional bits
//! osc.set_parameters(16384, 0);
//!
//! let sync = [0u8; 24];
//! let mut block = [0i16; 24];
//! osc.render(&sync, &mut block, 24);
//! ```
#![no_std]
// This crate is a deliberate line-by-line port of fixed-point C. Several Clippy
// lints fight that goal: `a * b >> c` mirrors the C precedence exactly, the
// `while <invariant> { .. break }` BLEP loops mirror the C control flow, and
// index-based loops keep the correspondence with the C obvious.
#![allow(
    clippy::precedence,
    clippy::while_immutable_condition,
    clippy::needless_range_loop,
    clippy::identity_op,
    clippy::unnecessary_cast,
    clippy::manual_div_ceil,
    clippy::should_implement_trait,
    clippy::manual_memcpy,
    clippy::neg_multiply,
    clippy::needless_late_init,
    clippy::explicit_auto_deref
)]

pub mod analog_oscillator;
pub mod digital_oscillator;
pub mod dsp;
pub mod envelope;
pub mod excitation;
pub mod macro_oscillator;
pub mod quantizer;
pub mod resources;
pub mod shapes;
pub mod signature_waveshaper;
pub mod svf;
pub mod vco_jitter_source;

pub use analog_oscillator::AnalogOscillator;
pub use digital_oscillator::DigitalOscillator;
pub use envelope::{Envelope, EnvelopeSegment};
pub use excitation::Excitation;
pub use macro_oscillator::{MacroOscillator, MAX_BLOCK_SIZE};
pub use quantizer::{Quantizer, Scale, SCALES};
pub use shapes::{AnalogOscillatorShape, DigitalModel, MacroOscillatorShape};
pub use signature_waveshaper::SignatureWaveshaper;
pub use svf::{Svf, SvfMode};
pub use vco_jitter_source::VcoJitterSource;
