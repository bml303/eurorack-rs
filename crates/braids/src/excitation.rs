//! `braids/excitation.h` -- an exponentially-decaying impulse used to excite the
//! drum resonators.

#[derive(Debug, Clone)]
pub struct Excitation {
    delay: u32,
    decay: u32,
    counter: i32,
    state: i32,
    level: i32,
}

impl Default for Excitation {
    fn default() -> Self {
        Self::new()
    }
}

impl Excitation {
    pub fn new() -> Self {
        let mut e = Self {
            delay: 0,
            decay: 4093,
            counter: 0,
            state: 0,
            level: 0,
        };
        e.init();
        e
    }

    pub fn init(&mut self) {
        self.delay = 0;
        self.decay = 4093;
        self.counter = 0;
        self.state = 0;
    }

    #[inline]
    pub fn set_delay(&mut self, delay: u32) {
        self.delay = delay;
    }

    #[inline]
    pub fn set_decay(&mut self, decay: u32) {
        self.decay = decay;
    }

    #[inline]
    pub fn trigger(&mut self, level: i32) {
        self.level = level;
        self.counter = self.delay as i32 + 1;
    }

    #[inline]
    pub fn done(&self) -> bool {
        self.counter == 0
    }

    #[inline]
    pub fn process(&mut self) -> i32 {
        self.state = self.state.wrapping_mul(self.decay as i32) >> 12;
        if self.counter > 0 {
            self.counter -= 1;
            if self.counter == 0 {
                self.state += self.level.abs();
            }
        }
        if self.level < 0 {
            -self.state
        } else {
            self.state
        }
    }
}
