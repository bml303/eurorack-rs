//! `peaks/pulse_processor/pulse_randomizer.{h,cc}` -- randomly accepts or
//! ignores incoming triggers, and randomly spawns extra repetitions of
//! accepted ones. Like `PulseShaper`, operates a block at a time.

use stmlib::fixed::interpolate_88_u16;
use stmlib::random::Random;

use crate::gate_processor::{ControlMode, GateFlags};
use crate::resources::LUT_DELAY_TIMES;

pub const K_TRIGGER_PULSE_BUFFER_SIZE: usize = 32;

#[derive(Debug, Clone, Copy)]
pub struct PulseRandomizer {
    repetition_probability: u16,
    acceptance_probability: u16,
    delay_average: u16,
    delay_randomness: u16,

    num_pulses: u16,
    retrig_counter: u16,

    delay_counter: [u16; K_TRIGGER_PULSE_BUFFER_SIZE],
}

impl Default for PulseRandomizer {
    fn default() -> Self {
        Self {
            repetition_probability: 32767,
            acceptance_probability: 65535,
            delay_average: 32767,
            delay_randomness: 0,
            num_pulses: 0,
            retrig_counter: 0,
            delay_counter: [0xffff; K_TRIGGER_PULSE_BUFFER_SIZE],
        }
    }
}

impl PulseRandomizer {
    pub fn init(&mut self) {
        self.repetition_probability = 32767;
        self.acceptance_probability = 65535;
        self.delay_average = 32767;
        self.delay_randomness = 0;

        self.delay_counter = [0xffff; K_TRIGGER_PULSE_BUFFER_SIZE];

        self.num_pulses = 0;
        self.retrig_counter = 0;
    }

    pub fn configure(&mut self, parameter: &[u16; 4], control_mode: ControlMode) {
        if control_mode == ControlMode::Half {
            self.set_acceptance_probability(65535);
            self.set_repetition_probability(parameter[0]);
            self.set_delay_average(parameter[1]);
            self.set_delay_randomness(0);
        } else {
            self.set_acceptance_probability(parameter[0]);
            self.set_repetition_probability(parameter[1]);
            self.set_delay_average(parameter[2]);
            self.set_delay_randomness(parameter[3]);
        }
    }

    pub fn set_repetition_probability(&mut self, repetition_probability: u16) {
        self.repetition_probability = repetition_probability;
    }
    pub fn set_acceptance_probability(&mut self, acceptance_probability: u16) {
        self.acceptance_probability = acceptance_probability;
    }
    pub fn set_delay_average(&mut self, delay_average: u16) {
        self.delay_average = delay_average >> 1;
    }
    pub fn set_delay_randomness(&mut self, delay_randomness: u16) {
        self.delay_randomness = delay_randomness;
    }

    fn delay(&self) -> u16 {
        let mut delay: i32 = self.delay_average as i32;
        delay += self.delay_average as i32 + ((Random::get_sample() as i32).wrapping_mul(self.delay_randomness as i32) >> 16);
        delay = delay.clamp(0, 0xffff);
        interpolate_88_u16(&LUT_DELAY_TIMES, delay as u16)
    }

    pub fn process(&mut self, gate_flags: &[GateFlags], out: &mut [i16]) {
        let mut new_pulse = false;
        for &g in gate_flags.iter() {
            new_pulse |= g.contains(GateFlags::RISING);
        }

        if (Random::get_word() >> 16) > self.acceptance_probability as u32 {
            // Randomly ignore incoming pulses.
            new_pulse = false;
        }

        if new_pulse {
            self.num_pulses += 1;
        }

        for i in 0..K_TRIGGER_PULSE_BUFFER_SIZE {
            if self.delay_counter[i] == 0xffff {
                if new_pulse {
                    self.delay_counter[i] = self.delay();
                    new_pulse = false;
                }
            } else if self.delay_counter[i] != 0 {
                self.delay_counter[i] -= 1;
            } else if (Random::get_word() >> 16) < self.repetition_probability as u32 {
                self.num_pulses += 1;
                self.delay_counter[i] = self.delay();
            } else {
                self.delay_counter[i] = 0xffff;
            }
        }

        if self.retrig_counter != 0 {
            self.retrig_counter -= 1;
        } else if self.num_pulses != 0 {
            self.retrig_counter = 12;
            self.num_pulses -= 1;
        }

        let output: i16 = if self.retrig_counter > 6 { 20480 } else { 0 };
        out.fill(output);
    }
}
