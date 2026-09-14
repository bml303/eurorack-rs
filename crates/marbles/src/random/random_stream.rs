//! `marbles/random/random_stream.h` -- stream of random values, filled from a
//! hardware RNG, with a fallback mechanism.
//!
//! Deviation from the C++: the original stores a `RandomGenerator*` fallback
//! pointer so several `RandomStream` instances could in theory share one
//! generator. In practice there is exactly one of each, `Mix()` (the only
//! thing that would let the hardware feed back into the fallback generator)
//! is a no-op in the shipped firmware, so the fallback generator is embedded
//! by value here instead of aliased through a pointer -- observably identical,
//! and it sidesteps `no_std`/no-`alloc` shared-mutable-reference plumbing.

use super::random_generator::RandomGenerator;
use stmlib::RingBuffer;

const BUFFER_SIZE: usize = 128;

#[derive(Debug, Clone, Default)]
pub struct RandomStream {
    buffer: RingBuffer<u32, BUFFER_SIZE>,
    fallback_generator: RandomGenerator,
}

impl RandomStream {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn init(&mut self, fallback_generator: RandomGenerator) {
        self.fallback_generator = fallback_generator;
        self.buffer.init();
    }

    pub fn write(&mut self, value: u32) {
        if self.buffer.writable() != 0 {
            self.buffer.overwrite(value);
        }
        self.fallback_generator.mix(value);
    }

    pub fn get_word(&mut self) -> u32 {
        if self.buffer.readable() != 0 {
            self.buffer.immediate_read()
        } else {
            self.fallback_generator.get_word()
        }
    }

    pub fn get_float(&mut self) -> f32 {
        let word = self.get_word();
        word as f32 / 4294967296.0
    }
}
