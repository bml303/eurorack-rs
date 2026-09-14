//! `peaks/modulations/mini_sequencer.h` -- a 2- or 4-step CV sequencer,
//! advanced by the main gate input and reset by an auxiliary gate.

use crate::gate_processor::{ControlMode, GateFlags};

pub const K_MAX_NUM_STEPS: usize = 4;

#[derive(Debug, Clone, Copy, Default)]
pub struct MiniSequencer {
    num_steps: u8,
    step: u8,
    steps: [i16; K_MAX_NUM_STEPS],

    reset_at_next_clock: bool,
}

impl MiniSequencer {
    pub fn init(&mut self) {
        self.steps = [0; K_MAX_NUM_STEPS];
        self.num_steps = 4;
        self.step = 0;
        self.reset_at_next_clock = false;
    }

    pub fn set_step(&mut self, index: usize, value: i16) {
        let difference = (self.steps[index] as i32 - value as i32).abs();
        if difference > 0 {
            self.steps[index] = value;
        }
    }

    pub fn set_num_steps(&mut self, num_steps: u8) {
        self.num_steps = num_steps;
    }

    pub fn configure(&mut self, parameter: &[u16; 4], control_mode: ControlMode) {
        if control_mode == ControlMode::Half {
            self.set_step(0, (parameter[0] as i32 - 32768) as i16);
            self.set_step(1, (parameter[1] as i32 - 32768) as i16);
            self.set_num_steps(2);
        } else {
            self.set_step(0, (parameter[0] as i32 - 32768) as i16);
            self.set_step(1, (parameter[1] as i32 - 32768) as i16);
            self.set_step(2, (parameter[2] as i32 - 32768) as i16);
            self.set_step(3, (parameter[3] as i32 - 32768) as i16);
            self.set_num_steps(4);
        }
    }

    pub fn process(&mut self, gate_flags: &[GateFlags], out: &mut [i16]) {
        for (&gate_flag, o) in gate_flags.iter().zip(out.iter_mut()) {
            if gate_flag.contains(GateFlags::RISING) {
                self.step += 1;
                if self.reset_at_next_clock {
                    self.reset_at_next_clock = false;
                    self.step = 0;
                }
            }
            if self.num_steps > 2 && gate_flag.contains(GateFlags::AUXILIARY_RISING) {
                self.reset_at_next_clock = true;
            }
            if self.step >= self.num_steps {
                self.step = 0;
            }
            *o = ((self.steps[self.step as usize] as i32).wrapping_mul(40960) >> 16) as i16;
        }
    }
}
