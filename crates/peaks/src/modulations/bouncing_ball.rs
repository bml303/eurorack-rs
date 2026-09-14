//! `peaks/modulations/bouncing_ball.h` -- simulates a ball bouncing under
//! gravity, losing energy on each bounce, retriggered from its initial
//! height/velocity on a gate rising edge.

use stmlib::fixed::interpolate_88_u16;

use crate::gate_processor::{ControlMode, GateFlags};
use crate::resources::LUT_GRAVITY;

#[derive(Debug, Clone, Copy, Default)]
pub struct BouncingBall {
    gravity: i32,
    bounce_loss: i32,
    initial_amplitude: i32,
    initial_velocity: i32,

    velocity: i32,
    position: i32,
}

impl BouncingBall {
    pub fn init(&mut self) {
        self.initial_amplitude = 65535i32 << 14;
        self.gravity = 40;
        self.bounce_loss = 4095;
        self.initial_velocity = 0;
    }

    pub fn configure(&mut self, parameter: &[u16; 4], control_mode: ControlMode) {
        if control_mode == ControlMode::Half {
            self.set_initial_amplitude(65535);
            self.set_initial_velocity(0);
            self.set_gravity(parameter[0]);
            self.set_bounce_loss(parameter[1]);
        } else {
            self.set_gravity(parameter[0]);
            self.set_bounce_loss(parameter[1]);
            self.set_initial_amplitude(parameter[2]);
            self.set_initial_velocity((parameter[3] as i32 - 32768) as i16);
        }
    }

    pub fn process(&mut self, gate_flags: &[GateFlags], out: &mut [i16]) {
        for (&gate_flag, o) in gate_flags.iter().zip(out.iter_mut()) {
            if gate_flag.contains(GateFlags::RISING) {
                self.velocity = self.initial_velocity;
                self.position = self.initial_amplitude;
            }
            self.velocity = self.velocity.wrapping_sub(self.gravity);
            self.position = self.position.wrapping_add(self.velocity);
            if self.position < 0 {
                self.position = 0;
                self.velocity = (self.velocity >> 12).wrapping_neg().wrapping_mul(self.bounce_loss);
            }
            if self.position > (32767i32 << 15) {
                self.position = 32767i32 << 15;
                self.velocity = (self.velocity >> 12).wrapping_neg().wrapping_mul(self.bounce_loss);
            }
            *o = (self.position >> 15) as i16;
        }
    }

    pub fn set_gravity(&mut self, gravity: u16) {
        self.gravity = interpolate_88_u16(&LUT_GRAVITY, gravity) as i32;
    }

    pub fn set_bounce_loss(&mut self, bounce_loss: u16) {
        let mut b: u32 = 65535 - bounce_loss as u32;
        b = b.wrapping_mul(b) >> 16;
        self.bounce_loss = 4095 - (b >> 4) as i32;
    }

    pub fn set_initial_amplitude(&mut self, initial_amplitude: u16) {
        self.initial_amplitude = (initial_amplitude as i32) << 14;
    }

    pub fn set_initial_velocity(&mut self, initial_velocity: i16) {
        self.initial_velocity = (initial_velocity as i32) << 4;
    }

    pub fn fill_buffer(&self) -> bool {
        true
    }
}
