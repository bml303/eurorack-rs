//! `marbles/random/random_generator.h` -- pseudo-random generator used as a
//! fallback when we need more random values than available in the hardware
//! RNG buffer.

#[derive(Debug, Clone, Copy, Default)]
pub struct RandomGenerator {
    state: u32,
}

impl RandomGenerator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn init(&mut self, seed: u32) {
        self.state = seed;
    }

    /// `Mix` is a no-op in the original (the XOR line is commented out).
    pub fn mix(&mut self, _word: u32) {}

    pub fn get_word(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(1664525).wrapping_add(1013904223);
        self.state
    }
}
