//! `marbles/ramp/*` -- timing/clock primitives. Timing information is
//! represented as a ramp from `0.0` to [`MAX_RAMP_VALUE`].

pub mod ramp_divider;
pub mod ramp_extractor;
pub mod ramp_generator;
pub mod slave_ramp;

pub use ramp_divider::{RampDivider, Ratio};
pub use ramp_extractor::RampExtractor;
pub use ramp_generator::RampGenerator;
pub use slave_ramp::SlaveRamp;

/// `kMaxRampValue`.
pub const MAX_RAMP_VALUE: f32 = 0.9999;
