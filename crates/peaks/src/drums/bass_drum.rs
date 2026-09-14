//! `peaks/drums/bass_drum.{h,cc}` -- an 808-style bass drum: a band-pass
//! resonator excited by up/down pulses, with an attack-phase pitch bump.

use crate::drums::excitation::Excitation;
use crate::drums::svf::{Svf, SvfMode};
use crate::gate_processor::{ControlMode, GateFlags};

#[derive(Debug, Clone, Copy, Default)]
pub struct BassDrum {
    pulse_up: Excitation,
    pulse_down: Excitation,
    attack_fm: Excitation,
    resonator: Svf,

    frequency: i32,
    lp_coefficient: i32,
    lp_state: i32,
}

impl BassDrum {
    pub fn init(&mut self) {
        self.pulse_up.init();
        self.pulse_down.init();
        self.attack_fm.init();
        self.resonator.init();

        self.pulse_up.set_delay(0);
        self.pulse_up.set_decay(3340);

        self.pulse_down.set_delay(48); // 1.0e-3 * 48000, truncated.
        self.pulse_down.set_decay(3072);

        self.attack_fm.set_delay(192); // 4.0e-3 * 48000, truncated.
        self.attack_fm.set_decay(4093);

        self.resonator.set_punch(32768);

        self.set_frequency(0);
        self.set_decay(32768);
        self.set_tone(32768);
        self.set_punch(65535);

        self.lp_state = 0;
    }

    pub fn configure(&mut self, parameter: &[u16; 4], control_mode: ControlMode) {
        if control_mode == ControlMode::Half {
            self.set_frequency(0);
            self.set_punch(40000);
            self.set_tone(8192 + (parameter[0] >> 1));
            self.set_decay(parameter[1]);
        } else {
            self.set_frequency((parameter[0] as i32 - 32768) as i16);
            self.set_punch(parameter[1]);
            self.set_tone(parameter[2]);
            self.set_decay(parameter[3]);
        }
    }

    pub fn set_frequency(&mut self, frequency: i16) {
        self.frequency = (31 << 7) + ((frequency as i32).wrapping_mul(896) >> 15);
    }

    pub fn set_decay(&mut self, decay: u16) {
        let mut scaled: u32 = 65535 - decay as u32;
        let squared = scaled.wrapping_mul(scaled) >> 16;
        scaled = squared.wrapping_mul(scaled) >> 18;
        self.resonator.set_resonance((32768 - 128 - scaled as i32) as i16);
    }

    pub fn set_tone(&mut self, tone: u16) {
        let mut coefficient = tone as u32;
        coefficient = coefficient.wrapping_mul(coefficient) >> 16;
        self.lp_coefficient = 512 + ((coefficient >> 2).wrapping_mul(3)) as i32;
    }

    pub fn set_punch(&mut self, punch: u16) {
        self.resonator.set_punch(((punch as u32).wrapping_mul(punch as u32) >> 16) as u16);
    }

    pub fn process(&mut self, gate_flags: &[GateFlags], out: &mut [i16]) {
        for (i, &gate_flag) in gate_flags.iter().enumerate() {
            if gate_flag.contains(GateFlags::RISING) {
                self.pulse_up.trigger(275_251); // 12 * 32768 * 0.7, truncated.
                self.pulse_down.trigger(-13_763); // -19662 * 0.7, truncated.
                self.attack_fm.trigger(18000);
            }

            let mut excitation = 0i32;
            excitation = excitation.wrapping_add(self.pulse_up.process());
            excitation = excitation.wrapping_add(if !self.pulse_down.done() { 16384 } else { 0 });
            excitation = excitation.wrapping_add(self.pulse_down.process());
            self.attack_fm.process();
            self.resonator.set_frequency((self.frequency + if self.attack_fm.done() { 0 } else { 17 << 7 }) as i16);

            let resonator_output = (excitation >> 4).wrapping_add(self.resonator.process(SvfMode::Bp, excitation));
            self.lp_state = self.lp_state.wrapping_add((resonator_output - self.lp_state).wrapping_mul(self.lp_coefficient) >> 15);
            let output = stmlib::clip16_sym(self.lp_state);

            out[i] = output as i16;
        }
    }
}
