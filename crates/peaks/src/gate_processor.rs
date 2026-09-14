//! `peaks/gate_processor.h` -- shared declarations for all trigger/gate
//! processors. `GateFlags`/`extract_gate_flags` already live in
//! `mi-stmlib` (`edge-tagged gate bits`, shared with `mi-grids`/
//! `mi-tides`/...); this just adds [`ControlMode`].

pub use stmlib::gate_flags::{extract_gate_flags, GateFlags};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ControlMode {
    #[default]
    Full,
    Half,
}
