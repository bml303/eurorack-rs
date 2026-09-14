//! Rust port of Mutable Instruments **Grids** -- a topographic drum
//! sequencer: [`pattern_generator::PatternGenerator`] evaluates one of 25
//! pre-recorded density maps (interpolated 2D over an X/Y "topography" pad)
//! or a Euclidean-rhythm generator for 3 drum parts, driven by
//! [`clock::Clock`]'s tempo/swing phase accumulator.
//!
//! # Scope
//!
//! `clock.{cc,h}` and `pattern_generator.{cc,h}` (the actual sequencer) are
//! ported in full. Out of scope, per this workspace's DSP-library-only
//! rule: `grids.cc` (the app-level ADC/DAC/UI/shift-register main loop),
//! `hardware_config.h`'s AVR GPIO/SPI/serial typedefs (only its LED bit
//! constants are kept, as `pattern_generator::led_bits`), and
//! `PatternGenerator::LoadSettings`/`SaveSettings` (raw AVR EEPROM I/O) --
//! see `pattern_generator.rs`'s module doc comment for exactly what that
//! means for `init()`/persisted settings.
//!
//! # Status
//!
//! Fixed-point (AVR, `mi-edges`-style verbatim arithmetic: 8-bit multiply-
//! mix helpers ported from `avrlib/op.h`'s portable path, a 16-bit Galois
//! LFSR from `avrlib/random.h`, transpiled `PROGMEM` lookup tables
//! including the 25 drum density maps). No C bit-compare harness yet;
//! `tests/smoke.rs` exercises `PatternGenerator` across both output modes,
//! every clock resolution, swing on/off, gate mode on/off, and a long
//! clock/tick sweep.
#![no_std]

pub mod clock;
pub mod pattern_generator;
pub mod random;
pub mod resources;

pub use clock::Clock;
pub use pattern_generator::{
    ClockResolution, DrumsSettings, Options, OutputBits, OutputMode, PatternGenerator,
    PatternGeneratorOptions, PatternGeneratorSettings,
};

pub const PORTED: bool = true;
