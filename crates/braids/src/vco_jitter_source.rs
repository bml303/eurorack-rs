//! `braids/vco_jitter_source.h` -- slow pseudo-random pitch drift ("analog
//! temperature"). Only used by the top-level firmware; ported for completeness.

use stmlib::Random;

use crate::resources::WAV_SINE;

#[derive(Debug, Clone, Default)]
pub struct VcoJitterSource {
    phase_step: u32,
    phase: u32,
    external_temperature: i32,
    room_temperature: i32,
}

impl VcoJitterSource {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn init(&mut self) {
        *self = Self::default();
    }

    #[inline]
    pub fn render(&mut self, intensity: i32) -> i16 {
        let external_temperature_toss = Random::get_word() as u16;
        if external_temperature_toss == 0 {
            self.phase_step = self
                .phase_step
                .wrapping_mul(1_664_525)
                .wrapping_add(1_013_904_223);
            self.phase = self
                .phase
                .wrapping_add((self.phase_step >> 16).wrapping_mul(self.phase_step >> 16));
            self.external_temperature = (WAV_SINE[(self.phase >> 24) as usize] as i32) << 8;
        }
        self.room_temperature += (self.external_temperature - self.room_temperature) >> 16;
        (self.room_temperature.wrapping_mul(intensity) >> 19) as i16
    }
}
