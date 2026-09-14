//! `branches.cc`'s free-running 32-bit Galois LFSR (`rng_state`, feedback
//! polynomial `0xd0000001`). Distinct from `mi-edges`'/`mi-grids`' 16-bit
//! `avrlib::Random` LFSR -- this one is hand-inlined directly in
//! `branches.cc` with its own (different) polynomial and width, so it's
//! kept crate-local here too rather than introducing a third copy in a
//! shared module, following the same precedent.
//!
//! The firmware's main loop captures one 32-bit snapshot per iteration
//! (`random_words = rng_state`), consumes its low 16 bits for channel 0 and
//! next 16 bits for channel 1 (`& 0xffff`, then `>>= 16`), and only then
//! advances `rng_state` for the next iteration. [`Rng::next_words`]
//! reproduces exactly that: it returns the *pre-advance* snapshot and
//! updates the internal state as a side effect, so nothing in between can
//! observe an intermediate state.

#[derive(Debug, Clone, Copy)]
pub struct Rng {
    state: u32,
}

impl Default for Rng {
    fn default() -> Self {
        Self { state: 1 }
    }
}

impl Rng {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&mut self) {
        self.state = 1;
    }

    pub fn next_words(&mut self) -> u32 {
        let words = self.state;
        self.state = (self.state >> 1) ^ (0u32.wrapping_sub(self.state & 1) & 0xd000_0001);
        words
    }
}
