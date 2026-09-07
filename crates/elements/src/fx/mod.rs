//! `elements/dsp/fx/` -- the Dattorro-style delay-memory virtual machine
//! ([`fx_engine`]) and the two effects built on it: the input [`Diffuser`] and
//! the output [`Reverb`].

pub mod diffuser;
pub mod fx_engine;
pub mod reverb;

pub use diffuser::Diffuser;
pub use reverb::Reverb;
