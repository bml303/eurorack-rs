//! `elements/dsp/dsp.h` -- module-wide DSP constants.

/// `kSampleRate` -- Elements runs its engine at 32 kHz.
pub const SAMPLE_RATE: f32 = 32_000.0;

/// `kMaxBlockSize` -- the largest block [`Part::process`](crate::Part::process)
/// accepts.
pub const MAX_BLOCK_SIZE: usize = 16;
