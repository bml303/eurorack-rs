//! `peaks/pulse_processor/pulse_shaper.{h,cc}` -- trigger-to-gate converter
//! with pre-delay, duration, and repetitions: each incoming trigger spawns
//! a "pulse" tracked in a 32-slot ring, which are then all summed into a
//! single high/low output for the block.
//!
//! Operates on whole blocks: a rising edge anywhere in `gate_flags` spawns
//! (at most) one new pulse for the call, and the whole `out` slice is
//! filled with one flat value -- like `mi-peaks`' other engines whose
//! timing constants (the "6-sample retrigger dip", the 4-sample-block
//! assumption baked into `FmDrum`) are tuned to the firmware's fixed
//! `kBlockSize == 4`, this reproduces the block-at-a-time C++ exactly
//! rather than "fixing" it to be sample-accurate.

use stmlib::fixed::interpolate_88_u16;

use crate::gate_processor::{ControlMode, GateFlags};
use crate::resources::LUT_DELAY_TIMES;

pub const K_PULSE_BUFFER_SIZE: usize = 32;

#[derive(Debug, Clone, Copy, Default)]
struct Pulse {
    initial_delay_counter: u16,
    duration_counter: u16,
    delay_counter: u16,
    repetition_counter: u16,
}

#[derive(Debug, Clone, Copy)]
pub struct PulseShaper {
    initial_delay: u16,
    duration: u16,
    delay: u16,
    num_repetitions: u16,

    previous_num_pulses: u16,
    retrig_counter: u16,

    pulse_buffer: [Pulse; K_PULSE_BUFFER_SIZE],
}

impl Default for PulseShaper {
    fn default() -> Self {
        Self {
            initial_delay: 0,
            duration: 0,
            delay: 0,
            num_repetitions: 0,
            previous_num_pulses: 0,
            retrig_counter: 0,
            pulse_buffer: [Pulse::default(); K_PULSE_BUFFER_SIZE],
        }
    }
}

impl PulseShaper {
    pub fn init(&mut self) {
        self.initial_delay = 0;
        self.duration = 0;
        self.delay = 0;
        self.num_repetitions = 0;
        self.pulse_buffer = [Pulse::default(); K_PULSE_BUFFER_SIZE];
        self.previous_num_pulses = 0;
        self.retrig_counter = 0;
    }

    pub fn configure(&mut self, parameter: &[u16; 4], control_mode: ControlMode) {
        if control_mode == ControlMode::Half {
            self.set_initial_delay(0);
            self.set_duration(parameter[0] >> 1);
            self.set_delay((parameter[0] >> 1) + 2048);
            self.set_num_repetitions(parameter[1]);
        } else {
            self.set_initial_delay(parameter[0]);
            self.set_duration(parameter[1] >> 1);
            self.set_delay(parameter[2] >> 1);
            self.set_num_repetitions(parameter[3]);
        }
    }

    pub fn set_initial_delay(&mut self, initial_delay: u16) {
        self.initial_delay = initial_delay;
    }
    pub fn set_duration(&mut self, duration: u16) {
        self.duration = duration;
    }
    pub fn set_delay(&mut self, delay: u16) {
        self.delay = delay;
    }
    pub fn set_num_repetitions(&mut self, num_repetitions: u16) {
        self.num_repetitions = num_repetitions >> 13;
    }

    fn delay(&self) -> u16 {
        interpolate_88_u16(&LUT_DELAY_TIMES, self.delay).wrapping_sub(1)
    }
    fn duration(&self) -> u16 {
        interpolate_88_u16(&LUT_DELAY_TIMES, self.duration)
    }
    fn initial_delay(&self) -> u16 {
        interpolate_88_u16(&LUT_DELAY_TIMES, self.initial_delay)
    }

    pub fn process(&mut self, gate_flags: &[GateFlags], out: &mut [i16]) {
        let mut new_pulse = false;
        for &g in gate_flags.iter() {
            new_pulse |= g.contains(GateFlags::RISING);
        }

        let mut num_pulses: u8 = 0;
        for i in 0..K_PULSE_BUFFER_SIZE {
            if self.pulse_buffer[i].repetition_counter != 0 {
                // Handle the case when the duration of the pulse is larger than
                // the delay time. In this case, set the duration to just a
                // sample below the delay time.
                if self.pulse_buffer[i].delay_counter < self.pulse_buffer[i].duration_counter && self.pulse_buffer[i].repetition_counter > 1 {
                    self.pulse_buffer[i].duration_counter = self.pulse_buffer[i].delay_counter;
                }

                if self.pulse_buffer[i].initial_delay_counter == 0 {
                    if self.pulse_buffer[i].duration_counter != 0 {
                        // ON
                        self.pulse_buffer[i].duration_counter -= 1;
                        num_pulses += 1;
                    }
                    if self.pulse_buffer[i].delay_counter != 0 {
                        self.pulse_buffer[i].delay_counter -= 1;
                    } else {
                        // Retrigger
                        self.pulse_buffer[i].repetition_counter -= 1;
                        self.pulse_buffer[i].duration_counter = self.duration();
                        self.pulse_buffer[i].delay_counter = self.delay();
                    }
                } else {
                    // Still in pre-delay phase...
                    self.pulse_buffer[i].initial_delay_counter -= 1;
                }
            } else if new_pulse {
                self.pulse_buffer[i].repetition_counter = self.num_repetitions + 1;
                self.pulse_buffer[i].initial_delay_counter = self.initial_delay();
                self.pulse_buffer[i].duration_counter = self.duration();
                self.pulse_buffer[i].delay_counter = self.delay();
                new_pulse = false;
                num_pulses += if self.pulse_buffer[i].initial_delay_counter != 0 { 0 } else { 1 };
            }
        }

        // The output is already high, but a new pulse is arriving. Create
        // a short dip in the output to retrigger.
        if self.previous_num_pulses != 0 && num_pulses as u16 > self.previous_num_pulses {
            self.retrig_counter = 6;
        }
        self.previous_num_pulses = num_pulses as u16;

        if self.retrig_counter != 0 {
            self.retrig_counter -= 1;
        }

        let output: i16 = if num_pulses > 0 && self.retrig_counter == 0 { 20480 } else { 0 };
        out.fill(output);
    }
}
