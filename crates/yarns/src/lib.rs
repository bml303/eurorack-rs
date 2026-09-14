//! Rust port of Mutable Instruments **Yarns** -- a monophonic/polyphonic MIDI
//! interface: [`Multi`] routes MIDI to up to 4 [`Part`]s (each its own
//! mono/poly voice allocator, arpeggiator and step sequencer) across 11
//! [`multi::Layout`]s, driving up to 4 [`Voice`]s (portamento, vibrato/LFO,
//! PLL-synced LFO, gate/trigger envelope, a small BLEP oscillator for the
//! "audio" output modes), a just-intonation tuner, and an internal MIDI
//! clock -- plus the built-in demo song.
//!
//! # Scope
//!
//! Yarns is structurally different from this workspace's other modules:
//! it's a MIDI sequencer/router, not an audio voice engine. Ported: `Voice`/
//! `Oscillator` (`voice.{cc,h}`), `JustIntonationProcessor`
//! (`just_intonation_processor.{cc,h}`), `InternalClock`
//! (`internal_clock.h`), `Part` (`part.{cc,h}` -- note allocation, the
//! arpeggiator, the step sequencer), `Multi` (`multi.{cc,h}` -- multi-part
//! MIDI routing, the shared clock, CV/gate/audio-source derivation, the
//! built-in demo song from `song/song.h`).
//!
//! `midi_dispatch::MidiDispatch` (`yarns/midi_handler.{cc,h}`'s dispatch
//! functions) pairs with `stmlib::MidiStreamParser` to turn a raw MIDI byte
//! stream straight into `Multi` calls, including the selective per-message
//! MIDI thru the C++'s `MidiHandler` did.
//!
//! # Scope
//!
//! Yarns is structurally different from this workspace's other modules:
//! it's a MIDI sequencer/router, not an audio voice engine. Ported: `Voice`/
//! `Oscillator` (`voice.{cc,h}`), `JustIntonationProcessor`
//! (`just_intonation_processor.{cc,h}`), `InternalClock`
//! (`internal_clock.h`), `Part` (`part.{cc,h}` -- note allocation, the
//! arpeggiator, the step sequencer), `Multi` (`multi.{cc,h}` -- multi-part
//! MIDI routing, the shared clock, CV/gate/audio-source derivation, the
//! built-in demo song from `song/song.h`), and `midi_handler.{cc,h}`'s
//! dispatch logic (`midi_dispatch.rs`, on top of `stmlib::MidiStreamParser`
//! -- see its module doc comment for exactly what that leaves out and why).
//!
//! `settings`/`ui`/`storage_manager`/`layout_configurator`/`drivers` are out
//! of scope entirely, per this workspace's DSP-library-only rule -- see
//! `part.rs`/`multi.rs`'s module doc comments for exactly which methods/
//! fields that cost, and what replaced them.
//!
//! # Status
//!
//! Fixed-point (STM32F2, ARM Cortex-M3, no FPU), `mi-braids`/`mi-edges`-style
//! verbatim arithmetic (24/32-bit phase accumulators, BLEP-antialiased
//! oscillator, calibrated DAC code lookups). No C bit-compare harness yet;
//! `tests/smoke.rs` exercises `Voice` (all audio modes, note on/off,
//! portamento, vibrato, trigger shapes), `JustIntonationProcessor`, `Multi`
//! (every layout, every voice-allocation mode, the arpeggiator, the step
//! sequencer), the built-in demo song, and `MidiDispatch` (a raw MIDI byte
//! stream, including running status and realtime interleaving, driving a
//! `Multi` end to end).
#![no_std]

pub mod internal_clock;
pub mod just_intonation_processor;
pub mod midi_dispatch;
pub mod midi_out;
pub mod multi;
pub mod oscillator;
pub mod part;
pub mod resources;
pub mod song;
pub mod voice;

pub use internal_clock::InternalClock;
pub use just_intonation_processor::JustIntonationProcessor;
pub use midi_dispatch::MidiDispatch;
pub use midi_out::{MidiOut, NullMidiOut};
pub use multi::{Layout, Multi, MultiSettings};
pub use oscillator::Oscillator;
pub use part::{Part, TuningContext};
pub use voice::Voice;

pub const PORTED: bool = true;
