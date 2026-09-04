//! `plaits/dsp/physical_modelling/*`. The plaits-local `delay_line.h` isn't
//! ported separately -- it differs from `stmlib::DelayLine` only in not
//! owning its buffer (a `BufferAllocator` detail this port doesn't need), so
//! [`string`] uses `stmlib::DelayLine` directly.

pub mod modal_voice;
pub mod resonator;
pub mod string;
pub mod string_voice;

pub use modal_voice::ModalVoice;
pub use resonator::{Resonator, ResonatorSvf};
pub use string::{String, StringNonLinearity};
pub use string_voice::StringVoice;
