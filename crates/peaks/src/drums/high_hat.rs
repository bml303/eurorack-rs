//! `peaks/drums/high_hat.{h,cc}` -- an 808-style hi-hat: 6 fixed square-wave
//! phasors XORed together for metallic noise, band-passed, gated by a VCA
//! envelope that only lets the positive half through, then high-passed
//! twice for "coloration". Has no configurable parameters.

use crate::drums::svf::{Svf, SvfMode};
use crate::drums::excitation::Excitation;
use crate::gate_processor::{ControlMode, GateFlags};

const PHASE_INCREMENTS: [u32; 6] = [48318382, 71582788, 37044092, 54313440, 66214079, 93952409];

#[derive(Debug, Clone, Copy, Default)]
pub struct HighHat {
    noise: Svf,
    vca_coloration: Svf,
    vca_envelope: Excitation,

    phase: [u32; 6],
}

impl HighHat {
    pub fn init(&mut self) {
        self.noise.init();
        self.noise.set_frequency(105 << 7); // 8kHz
        self.noise.set_resonance(24000);

        self.vca_coloration.init();
        self.vca_coloration.set_frequency(110 << 7); // 13kHz
        self.vca_coloration.set_resonance(0);

        self.vca_envelope.init();
        self.vca_envelope.set_delay(0);
        self.vca_envelope.set_decay(4093);
    }

    pub fn configure(&mut self, _parameter: &[u16; 4], _control_mode: ControlMode) {}

    pub fn process(&mut self, gate_flags: &[GateFlags], out: &mut [i16]) {
        for (i, &gate_flag) in gate_flags.iter().enumerate() {
            if gate_flag.contains(GateFlags::RISING) {
                self.vca_envelope.trigger(32768 * 15);
            }

            for (p, inc) in self.phase.iter_mut().zip(PHASE_INCREMENTS.iter()) {
                *p = p.wrapping_add(*inc);
            }

            let mut noise: i16 = 0;
            for &p in self.phase.iter() {
                noise = noise.wrapping_add((p >> 31) as i16);
            }
            noise <<= 12;

            // Run the SVF at double the original sample rate for stability.
            let mut filtered_noise = 0i32;
            filtered_noise = filtered_noise.wrapping_add(self.noise.process(SvfMode::Bp, noise as i32));
            filtered_noise = filtered_noise.wrapping_add(self.noise.process(SvfMode::Bp, noise as i32));

            // The 808-style VCA amplifies only the positive section of the signal.
            filtered_noise = filtered_noise.clamp(0, 32767);

            let envelope = self.vca_envelope.process() >> 4;
            let vca_noise = stmlib::clip16_sym(envelope.wrapping_mul(filtered_noise) >> 14);
            let mut hh = 0i32;
            hh = hh.wrapping_add(self.vca_coloration.process(SvfMode::Hp, vca_noise));
            hh = hh.wrapping_add(self.vca_coloration.process(SvfMode::Hp, vca_noise));
            hh <<= 1;
            out[i] = stmlib::clip16_sym(hh) as i16;
        }
    }
}
