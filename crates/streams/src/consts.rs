//! `streams/gain.h` -- DAC codes for various gain reference points.

/// DAC code compensating for the offness resistor's -10V bias.
pub const K_DEFAULT_OFFSET: i32 = 655;
/// DAC code giving a unitary gain.
pub const K_UNITY_GAIN: i32 = 32767;
/// Slightly above unitary gain.
pub const K_ABOVE_UNITY_GAIN: i32 = 32896;
/// Maximum gain in dB in lin mode with a DAC code of 65535 (6 dB).
pub const K_MAX_LINEAR_GAIN: i32 = 65536;
/// Maximum gain in dB in lin mode with a DAC code of 65535 (18 dB).
pub const K_MAX_EXPONENTIAL_GAIN: i32 = 218453;

/// `32768 * 5 * 2 / 3 / 8` (integer division, left to right).
pub const K_SCHMITT_TRIGGER_THRESHOLD: i32 = 13653;
