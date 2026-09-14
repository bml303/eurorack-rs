//! `avrlib/random.h` -- a 16-bit Galois LFSR (feedback polynomial
//! `x^16 + x^14 + x^13 + x^11`, period 65535). Distinct from
//! `stmlib::random::Random` (an STM32-firmware LCG) -- AVR firmware in this
//! workspace (`mi-edges`) already inlines this exact recurrence directly at
//! its one call site rather than factoring it into `mi-stmlib`, so it's
//! kept local to this crate too rather than introducing a third copy in a
//! shared module.

#[derive(Debug, Clone, Copy, Default)]
pub struct Lfsr {
    state: u16,
}

impl Lfsr {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn seed(&mut self, seed: u16) {
        self.state = seed;
    }

    pub fn update(&mut self) {
        self.state = (self.state >> 1) ^ (0u16.wrapping_sub(self.state & 1) & 0xb400);
    }

    pub fn state(&self) -> u16 {
        self.state
    }

    pub fn state_msb(&self) -> u8 {
        (self.state >> 8) as u8
    }

    pub fn get_byte(&mut self) -> u8 {
        self.update();
        self.state_msb()
    }

    #[allow(dead_code)]
    pub fn get_word(&mut self) -> u16 {
        self.update();
        self.state()
    }
}
