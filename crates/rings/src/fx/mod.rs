//! `rings/dsp/fx/` -- the Dattorro accumulator machine ([`fx_engine`]) and the
//! three effects built on it: [`Reverb`] (used by `Part`'s string+reverb model
//! and `StringSynthPart`), [`Chorus`] and [`Ensemble`] (`StringSynthPart` only).

pub mod chorus;
pub mod ensemble;
pub mod fx_engine;
pub mod reverb;

pub use chorus::Chorus;
pub use ensemble::Ensemble;
pub use reverb::Reverb;
