//! `stmlib/utils/gate_flags.h` -- edge-tagged gate bits.
//!
//! Gate samples are pre-processed so a downstream segment generator can see, in
//! one byte, both the level and whether this sample is an edge.

/// A gate sample annotated with its edge, as consumed by the segment generators
/// (Stages, Tides, Peaks, ...).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GateFlags(pub u8);

impl GateFlags {
    pub const LOW: GateFlags = GateFlags(0);
    pub const HIGH: GateFlags = GateFlags(1);
    pub const RISING: GateFlags = GateFlags(2);
    pub const FALLING: GateFlags = GateFlags(4);

    #[inline]
    pub const fn bits(self) -> u8 {
        self.0
    }

    #[inline]
    pub const fn contains(self, other: GateFlags) -> bool {
        (self.0 & other.0) == other.0
    }
}

impl core::ops::BitOr for GateFlags {
    type Output = GateFlags;
    #[inline]
    fn bitor(self, rhs: GateFlags) -> GateFlags {
        GateFlags(self.0 | rhs.0)
    }
}

impl core::ops::BitAnd for GateFlags {
    type Output = GateFlags;
    #[inline]
    fn bitand(self, rhs: GateFlags) -> GateFlags {
        GateFlags(self.0 & rhs.0)
    }
}

/// `ExtractGateFlags(previous, current)`.
#[inline]
pub fn extract_gate_flags(previous: GateFlags, current: bool) -> GateFlags {
    let was_high = previous.contains(GateFlags::HIGH);
    if current {
        if was_high {
            GateFlags::HIGH
        } else {
            GateFlags::RISING | GateFlags::HIGH
        }
    } else if was_high {
        GateFlags::FALLING
    } else {
        GateFlags::LOW
    }
}
