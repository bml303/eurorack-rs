//! `peaks/drums/excitation.h` -- a triggerable, delayed exponential-decay
//! impulse, the shared building block of the 808-style drum voices.

#[derive(Debug, Clone, Copy, Default)]
pub struct Excitation {
    delay: u32,
    decay: u32,
    counter: i32,
    state: i32,
    level: i32,
}

impl Excitation {
    pub fn init(&mut self) {
        self.delay = 0;
        self.decay = 4093;
        self.counter = 0;
        self.state = 0;
    }

    pub fn set_delay(&mut self, delay: u32) {
        self.delay = delay;
    }

    pub fn set_decay(&mut self, decay: u32) {
        self.decay = decay;
    }

    pub fn trigger(&mut self, level: i32) {
        self.level = level;
        self.counter = self.delay as i32 + 1;
    }

    pub fn done(&self) -> bool {
        self.counter == 0
    }

    pub fn process(&mut self) -> i32 {
        // `state_ * decay_` is `int32_t * uint32_t` in the C++, which the
        // usual arithmetic conversions make a *32-bit unsigned* multiply
        // (wrapping at 2^32), not a widening one -- match that exactly
        // rather than reaching for i64 headroom that isn't actually there.
        self.state = ((self.state as u32).wrapping_mul(self.decay) >> 12) as i32;
        if self.counter > 0 {
            self.counter -= 1;
            if self.counter == 0 {
                self.state = self.state.wrapping_add(self.level.abs());
            }
        }
        if self.level < 0 {
            self.state.wrapping_neg()
        } else {
            self.state
        }
    }
}
