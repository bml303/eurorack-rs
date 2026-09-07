//! `rings/dsp/dsp.h` -- module-wide DSP constants.

/// `kSampleRate` -- Rings runs its engine at 48 kHz.
pub const SAMPLE_RATE: f32 = 48_000.0;

/// `a3` -- A4 (MIDI 69, 440 Hz) as a normalised frequency; the reference for
/// `SemitonesToRatio(note - 69.0) * a3`.
pub const A3: f32 = 440.0 / SAMPLE_RATE;

/// `kMaxBlockSize`.
pub const MAX_BLOCK_SIZE: usize = 24;
