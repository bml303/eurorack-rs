//! `plaits/dsp/dsp.h` -- global constants.

/// The engine's internal sample rate.
pub const SAMPLE_RATE: f32 = 48_000.0;
pub const INV_SAMPLE_RATE: f32 = 1.0 / SAMPLE_RATE;

// Plaits' own hardware has no proper PLL for I2S, only a divider on the
// system clock, which makes its true sample rate 47872.34 Hz rather than
// 48000, so the C computes its pitch reference `a0` from that corrected
// rate (4.6 cents) rather than the nominal one. That is a property of that
// module's clock tree, not of the DSP: a host that really plays the samples
// at `SAMPLE_RATE` must not apply the correction, or every engine that
// derives its pitch from `A0` comes out 4.6 cents flat (and, since the
// six-op FM voice takes the rate it is given, flat relative to that engine
// too). The default therefore keeps `A0` consistent with `SAMPLE_RATE`;
// the `original-module-sample-rate` feature restores the module's
// corrected rate for a host that reproduces its clocking exactly. Ported
// code must keep using `A0` for note-to-frequency conversion rather than
// re-deriving it from `SAMPLE_RATE`, so that one switch covers it.
#[cfg(feature = "original-module-sample-rate")]
pub const CORRECTED_SAMPLE_RATE: f32 = 47_872.34;
#[cfg(not(feature = "original-module-sample-rate"))]
pub const CORRECTED_SAMPLE_RATE: f32 = SAMPLE_RATE;
pub const A0: f32 = (440.0 / 8.0) / CORRECTED_SAMPLE_RATE;

//pub const MAX_BLOCK_SIZE: usize = 24;
//pub const BLOCK_SIZE: usize = 12;
