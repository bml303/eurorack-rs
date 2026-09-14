//! Rust port of Mutable Instruments **Yarns** -- a monophonic/polyphonic MIDI
//! interface: per-voice pitch (portamento, vibrato/LFO, PLL-synced LFO),
//! gate/trigger envelope generation, a small BLEP oscillator for its "audio"
//! output modes, a just-intonation tuner, and an internal MIDI clock.
//!
//! # Scope
//!
//! Yarns is structurally different from this workspace's other modules: it's
//! a MIDI sequencer/router, not an audio voice engine, and most of its
//! ~10,400 lines are MIDI parsing, note allocation/arpeggiation, multi-part
//! routing, a song recorder, persisted settings and the front-panel UI --
//! not audio-rate DSP. This first increment ports only the self-contained,
//! non-hardware-specific pieces: [`Voice`]/[`oscillator::Oscillator`] (the
//! fixed-point audio-rate engine, `voice.{cc,h}`),
//! [`JustIntonationProcessor`] (`just_intonation_processor.{cc,h}`), and
//! [`InternalClock`] (`internal_clock.h`).
//!
//! Deferred to a follow-up (see `PORTING.md`): `part.{cc,h}` (per-part note
//! allocation, arpeggiator, LFOs), `multi.{cc,h}` (multi-part MIDI routing),
//! `midi_handler.{cc,h}` (MIDI byte-stream parsing), and `song/song.h` (the
//! song recorder) -- none of what's ported here depends on them.
//! `settings`/`ui`/`storage_manager`/`layout_configurator`/`drivers` are out
//! of scope entirely, per this workspace's DSP-library-only rule.
//!
//! # Status
//!
//! Fixed-point (STM32F2, ARM Cortex-M3, no FPU), `mi-braids`/`mi-edges`-style
//! verbatim arithmetic (24/32-bit phase accumulators, BLEP-antialiased
//! oscillator, calibrated DAC code lookups). No C bit-compare harness yet;
//! `tests/smoke.rs` exercises `Voice` (all audio modes, note on/off,
//! portamento, vibrato, trigger shapes) and `JustIntonationProcessor`.
#![no_std]

pub mod internal_clock;
pub mod just_intonation_processor;
pub mod oscillator;
pub mod resources;
pub mod voice;

pub use internal_clock::InternalClock;
pub use just_intonation_processor::JustIntonationProcessor;
pub use oscillator::Oscillator;
pub use voice::Voice;

pub const PORTED: bool = true;
