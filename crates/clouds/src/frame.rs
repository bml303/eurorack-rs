//! `clouds/dsp/frame.h` -- interleaved stereo audio frames.

/// `kMaxBlockSize` -- the largest block `Process` accepts.
pub const MAX_BLOCK_SIZE: usize = 32;

/// `kMaxNumChannels`.
pub const MAX_NUM_CHANNELS: usize = 2;

/// `ShortFrame` -- a 16-bit interleaved stereo sample, the hardware I/O format.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ShortFrame {
    pub l: i16,
    pub r: i16,
}

/// `FloatFrame` -- the internal processing format.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FloatFrame {
    pub l: f32,
    pub r: f32,
}
