//! Rust port of Mutable Instruments **Clouds** -- a granular texture
//! synthesizer.
//!
//! # Status
//!
//! Clouds runs on an STM32F4 (Cortex-M4F, hardware FPU) almost entirely in
//! floating point, so -- like [`plaits`](https://docs.rs/mi-plaits) and unlike
//! [`braids`](https://docs.rs/mi-braids) -- there is no fixed-point
//! bit-exactness contract. The port is idiomatic Rust: methods instead of
//! function-pointer / template dispatch, runtime enums instead of template
//! parameters, owned buffers instead of the firmware's hand-partitioned
//! `void*` slabs. The handful of genuinely integer-exact pieces (the phase
//! accumulators, the sign-bit [`correlator`], [`mu_law`] companding) are
//! translated verbatim. Verified against the C firmware DSP: 9 of the 12
//! (playback mode x quality) renders are bit-identical, 2 more differ by 1 LSB
//! on a handful of samples (see `PORTING.md`).
//!
//! Ported and working -- the three time-domain playback modes and every
//! post-processing effect:
//!
//! * [`PlaybackMode::Granular`] -- the cloud of overlapping grains.
//! * [`PlaybackMode::Stretch`] -- WSOLA time-stretch / pitch-shift.
//! * [`PlaybackMode::LoopingDelay`] -- the looping delay / tape mode.
//! * [`fx`] -- the all-pass [`Diffuser`](fx::Diffuser),
//!   [`Reverb`](fx::Reverb) and [`PitchShifter`](fx::PitchShifter).
//!
//! **Not yet ported:** [`PlaybackMode::Spectral`] (the phase-vocoder mode).
//! Selecting it produces silence. It needs a port of `stmlib`'s `ShyFFT`
//! plus `stft` / `frame_transformation`; see `PORTING.md`.
//!
//! # Memory
//!
//! [`GranularProcessor::new`] allocates one owned slab the exact size of the
//! firmware's two memories (large 118784 B + small 65408 B) and hands
//! [`prepare`](GranularProcessor::prepare) recording buffers the exact size
//! the firmware's bump allocator would carve out, so every size-dependent
//! quantity -- maximum delay, grain count, WSOLA window -- matches the
//! hardware. The one deviation from the C's layout: the
//! [`Correlator`](correlator::Correlator) and
//! [`PitchShifter`](fx::PitchShifter) are given separate buffers instead of
//! the firmware's overlapping alias (they are used in mutually exclusive
//! modes, so the alias was a pure RAM optimisation with no audible effect).
//!
//! # Example
//!
//! ```
//! use clouds::{GranularProcessor, PlaybackMode, ShortFrame};
//!
//! let mut gp = GranularProcessor::new();
//! gp.set_playback_mode(PlaybackMode::LoopingDelay);
//! gp.set_quality(0); // stereo, full fidelity
//!
//! {
//!     let p = gp.mutable_parameters();
//!     p.position = 0.0;
//!     p.size = 0.5;
//!     p.density = 0.5;
//!     p.texture = 0.5;
//!     p.dry_wet = 1.0;
//! }
//!
//! // The firmware runs `prepare()` in a tight loop between audio blocks; the
//! // WSOLA correlator search depends on it, so call it a few times per block.
//! for _ in 0..16 {
//!     gp.prepare();
//! }
//!
//! let input = [ShortFrame { l: 0, r: 0 }; 32];
//! let mut output = [ShortFrame { l: 0, r: 0 }; 32];
//! gp.process(&input, &mut output);
//! ```
#![no_std]
#![allow(
    clippy::too_many_arguments,
    clippy::excessive_precision,
    clippy::needless_range_loop,
    clippy::manual_range_contains,
    // Several hot loops keep an explicit index to stay a line-by-line match
    // of the C's pointer walks.
    clippy::explicit_counter_loop
)]

extern crate alloc;

pub mod audio_buffer;
pub mod correlator;
pub mod dsp;
pub mod frame;
pub mod fx;
pub mod grain;
pub mod granular_processor;
pub mod mu_law;
pub mod parameters;
pub mod players;
pub mod resources;
pub mod sample_rate_converter;
pub mod window;

pub use frame::{FloatFrame, ShortFrame, MAX_BLOCK_SIZE};
pub use granular_processor::{GranularProcessor, PlaybackMode};
pub use parameters::Parameters;
