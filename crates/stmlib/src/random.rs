//! `stmlib/utils/random.h` -- a fast 32-bit LCG.
//!
//! The C version is a class with `static` state: one global RNG shared by the
//! whole firmware, seeded to `0x21`. The DSP ports call it in a fixed order and
//! depend on that shared sequence, so [`Random`] keeps a process-global cell.
//! For isolated tests use [`RandomState`].

use core::cell::Cell;

const MULTIPLIER: u32 = 1_664_525;
const INCREMENT: u32 = 1_013_904_223;

/// Single-threaded MCU model: the firmware never advances the RNG from two
/// contexts that can preempt each other mid-word.
struct GlobalRng(Cell<u32>);
unsafe impl Sync for GlobalRng {}
static STATE: GlobalRng = GlobalRng(Cell::new(0x21));

/// The global RNG, matching `stmlib::Random`.
pub struct Random;

impl Random {
    #[inline]
    pub fn state() -> u32 {
        STATE.0.get()
    }

    #[inline]
    pub fn seed(seed: u32) {
        STATE.0.set(seed);
    }

    #[inline]
    pub fn get_word() -> u32 {
        let next = STATE
            .0
            .get()
            .wrapping_mul(MULTIPLIER)
            .wrapping_add(INCREMENT);
        STATE.0.set(next);
        next
    }

    #[inline]
    pub fn get_sample() -> i16 {
        (Self::get_word() >> 16) as i16
    }

    #[inline]
    pub fn get_float() -> f32 {
        Self::get_word() as f32 / 4_294_967_296.0
    }
}

/// A standalone LCG instance with the same recurrence.
#[derive(Debug, Clone)]
pub struct RandomState {
    state: Cell<u32>,
}

impl RandomState {
    pub fn with_seed(seed: u32) -> Self {
        Self {
            state: Cell::new(seed),
        }
    }
    #[inline]
    pub fn get_word(&self) -> u32 {
        let next = self
            .state
            .get()
            .wrapping_mul(MULTIPLIER)
            .wrapping_add(INCREMENT);
        self.state.set(next);
        next
    }
    #[inline]
    pub fn get_sample(&self) -> i16 {
        (self.get_word() >> 16) as i16
    }
    #[inline]
    pub fn get_float(&self) -> f32 {
        self.get_word() as f32 / 4_294_967_296.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_c_recurrence() {
        Random::seed(0x21);
        let expected = 0x21u32.wrapping_mul(MULTIPLIER).wrapping_add(INCREMENT);
        assert_eq!(Random::get_word(), expected);
        assert_eq!(Random::state(), expected);
    }
}
