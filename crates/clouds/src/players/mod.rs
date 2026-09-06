//! `clouds/dsp/*_sample_player.h` -- the three time-domain playback engines.

pub mod granular;
pub mod looping;
pub mod wsola;

pub use granular::GranularSamplePlayer;
pub use looping::LoopingSamplePlayer;
pub use wsola::WSOLASamplePlayer;
