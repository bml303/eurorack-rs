//! `plaits/dsp/dsp.h` -- global constants.

/// The engine's internal sample rate.
pub const SAMPLE_RATE: f32 = 48_000.0;
pub const INV_SAMPLE_RATE: f32 = 1.0 / SAMPLE_RATE;

// There is no proper PLL for I2S, only a divider on the system clock to derive
// the bit clock. The division ratio makes the true audio sample rate 47872.34
// Hz rather than 48000 -- 4.6 cents flat. Plaits' pitch reference (`a0`) is
// computed from that corrected rate, not the nominal one, so ported code must
// keep using `a0` for note-to-frequency conversion rather than re-deriving it
// from `SAMPLE_RATE`.
pub const CORRECTED_SAMPLE_RATE: f32 = 47_872.34;
pub const A0: f32 = (440.0 / 8.0) / CORRECTED_SAMPLE_RATE;

//pub const MAX_BLOCK_SIZE: usize = 24;
//pub const BLOCK_SIZE: usize = 12;
