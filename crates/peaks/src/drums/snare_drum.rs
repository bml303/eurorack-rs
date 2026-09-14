//! `peaks/drums/snare_drum.{h,cc}` -- an 808-style snare drum: two tuned
//! band-pass resonators (body) plus a band-passed noise burst (snappy).

use stmlib::clip16_sym;
use stmlib::random::Random;

use crate::drums::excitation::Excitation;
use crate::drums::svf::{Svf, SvfMode};
use crate::gate_processor::{ControlMode, GateFlags};

#[derive(Debug, Clone, Copy, Default)]
pub struct SnareDrum {
    excitation_1_up: Excitation,
    excitation_1_down: Excitation,
    excitation_2: Excitation,
    excitation_noise: Excitation,
    body_1: Svf,
    body_2: Svf,
    noise: Svf,

    gain_1: i32,
    gain_2: i32,

    snappy: u16,
}

impl SnareDrum {
    pub fn init(&mut self) {
        self.excitation_1_up.init();
        self.excitation_1_up.set_delay(0);
        self.excitation_1_up.set_decay(1536);

        self.excitation_1_down.init();
        self.excitation_1_down.set_delay(48); // 1e-3 * 48000, truncated.
        self.excitation_1_down.set_decay(3072);

        self.excitation_2.init();
        self.excitation_2.set_delay(48); // 1e-3 * 48000, truncated.
        self.excitation_2.set_decay(1200);

        self.excitation_noise.init();
        self.excitation_noise.set_delay(0);

        self.body_1.init();
        self.body_2.init();

        self.noise.init();
        self.noise.set_resonance(2000);

        self.set_tone(0);
        self.set_snappy(32768);
        self.set_decay(32768);
        self.set_frequency(0);
    }

    pub fn configure(&mut self, parameter: &[u16; 4], control_mode: ControlMode) {
        if control_mode == ControlMode::Half {
            self.set_frequency(0);
            self.set_decay(32768);
            self.set_tone(parameter[0]);
            self.set_snappy(parameter[1]);
        } else {
            self.set_frequency((parameter[0] as i32 - 32768) as i16);
            self.set_tone(parameter[1]);
            self.set_snappy(parameter[2]);
            self.set_decay(parameter[3]);
        }
    }

    pub fn set_tone(&mut self, tone: u16) {
        self.gain_1 = 22000 - (tone >> 2) as i32;
        self.gain_2 = 22000 + (tone >> 2) as i32;
    }

    pub fn set_snappy(&mut self, snappy: u16) {
        let mut snappy = snappy >> 1;
        if snappy >= 28672 {
            snappy = 28672;
        }
        self.snappy = 512 + snappy;
    }

    pub fn set_decay(&mut self, decay: u16) {
        self.body_1.set_resonance((29000 + (decay >> 5) as i32) as i16);
        self.body_2.set_resonance((26500 + (decay >> 5) as i32) as i16);
        self.excitation_noise.set_decay((4092 + (decay >> 14)) as u32);
    }

    pub fn set_frequency(&mut self, frequency: i16) {
        // `base_note` is `int16_t` in the C++, and truncates back to that
        // width on the `+=` below before the later offsets are added --
        // matched explicitly rather than left in i32 the whole way, in
        // case a large `transposition` pushes the sum outside i16 range.
        let base_note: i16 = 52 << 7;
        let transposition = frequency as i32;
        let base_note = ((base_note as i32).wrapping_add(transposition.wrapping_mul(896) >> 15)) as i16;
        self.body_1.set_frequency(base_note);
        self.body_2.set_frequency(((base_note as i32) + (12 << 7)) as i16);
        self.noise.set_frequency(((base_note as i32) + (48 << 7)) as i16);
    }

    pub fn process(&mut self, gate_flags: &[GateFlags], out: &mut [i16]) {
        for (i, &gate_flag) in gate_flags.iter().enumerate() {
            if gate_flag.contains(GateFlags::RISING) {
                self.excitation_1_up.trigger(15 * 32768);
                self.excitation_1_down.trigger(-32768);
                self.excitation_2.trigger(13107);
                self.excitation_noise.trigger(self.snappy as i32);
            }

            let mut excitation_1 = 0i32;
            excitation_1 = excitation_1.wrapping_add(self.excitation_1_up.process());
            excitation_1 = excitation_1.wrapping_add(self.excitation_1_down.process());
            excitation_1 = excitation_1.wrapping_add(if !self.excitation_1_down.done() { 2621 } else { 0 });

            let body_1 = self.body_1.process(SvfMode::Bp, excitation_1).wrapping_add(excitation_1 >> 4);

            let mut excitation_2 = 0i32;
            excitation_2 = excitation_2.wrapping_add(self.excitation_2.process());
            excitation_2 = excitation_2.wrapping_add(if !self.excitation_2.done() { 13107 } else { 0 });

            let body_2 = self.body_2.process(SvfMode::Bp, excitation_2).wrapping_add(excitation_2 >> 4);
            let noise_sample = Random::get_sample() as i32;
            let noise = self.noise.process(SvfMode::Bp, noise_sample);
            let noise_envelope = self.excitation_noise.process();
            let mut sd = 0i32;
            sd = sd.wrapping_add(body_1.wrapping_mul(self.gain_1) >> 15);
            sd = sd.wrapping_add(body_2.wrapping_mul(self.gain_2) >> 15);
            sd = sd.wrapping_add(noise_envelope.wrapping_mul(noise) >> 15);
            out[i] = clip16_sym(sd) as i16;
        }
    }
}
