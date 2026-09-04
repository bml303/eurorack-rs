//! `plaits/dsp/drums/*` -- the 808-ish drum voices.

pub mod analog_bass_drum;
pub mod analog_snare_drum;
pub mod hi_hat;
pub mod synthetic_bass_drum;
pub mod synthetic_snare_drum;

pub use analog_bass_drum::AnalogBassDrum;
pub use analog_snare_drum::AnalogSnareDrum;
pub use hi_hat::{HiHat, MetallicNoise, Vca};
pub use synthetic_bass_drum::SyntheticBassDrum;
pub use synthetic_snare_drum::SyntheticSnareDrum;
