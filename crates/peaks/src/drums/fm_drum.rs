//! `peaks/drums/fm_drum.{h,cc}` -- a sine-FM drum voice (similar to the
//! BD/SD in Anushri): a sine carrier FM'd by its own decaying envelope,
//! AM'd by a second envelope, with an auxiliary low-frequency-boost
//! envelope, optional noise blend and wavefold overdrive.

use stmlib::fixed::{interpolate_1022, interpolate_824_u16, mix_i16};
use stmlib::random::Random;

use crate::gate_processor::{ControlMode, GateFlags};
use crate::resources::{LUT_ENV_EXPO, LUT_ENV_INCREMENTS, LUT_OSCILLATOR_INCREMENTS, WAV_OVERDRIVE, WAV_SINE};

const K_HIGHEST_NOTE: i32 = 128 * 128;
const K_PITCH_TABLE_START: i32 = 116 * 128;
const K_OCTAVE: i32 = 128 * 12;

#[rustfmt::skip]
const BD_MAP: [[u16; 4]; 10] = [
    [4096, 0, 65535, 32768],
    [8192 + 4096, 0, 65535, 32768],

    [8192, 4096, 49512, 32768],
    [8192, 16384, 40960, 32768],

    [10240, 4096, 24576, 32768],
    [10240, 16384, 24576, 16384],

    [8192, 8192, 32768, 16384],
    [8192, 24576, 49152, 8192],

    [4096, 16384, 40960, 16384],
    [8192, 24576, 49152, 0],
];

#[rustfmt::skip]
const SD_MAP: [[u16; 4]; 10] = [
    [24576, 0, 24576, 36864],
    [24576, 0, 16384, 65535],

    [28672, 0, 16384, 36864],
    [28672, 0, 16384, 65535],

    [20488, 0, 32768, 57344],
    [28672, 0, 24576, 65535],

    [20488, 0, 24576, 65535],
    [28672, 0, 32768, 65535],

    [20488, 65535, 16384, 0],
    [65535, 0, 8192, 32768],
];

#[derive(Debug, Clone, Copy, Default)]
pub struct FmDrum {
    sd_range: bool,

    aux_envelope_strength: u16,
    frequency: u16,
    fm_amount: u16,
    am_decay: u16,
    fm_decay: u16,
    noise: u16,
    overdrive: u16,
    previous_sample: i16,

    phase: u32,
    fm_envelope_phase: u32,
    am_envelope_phase: u32,
    aux_envelope_phase: u32,
    phase_increment: u32,
}

impl FmDrum {
    pub fn init(&mut self) {
        self.phase = 0;
        self.fm_envelope_phase = 0xffffffff;
        self.am_envelope_phase = 0xffffffff;
        self.previous_sample = 0;
    }

    pub fn set_sd_range(&mut self, sd_range: bool) {
        self.sd_range = sd_range;
    }

    pub fn configure(&mut self, parameter: &[u16; 4], control_mode: ControlMode) {
        if control_mode == ControlMode::Half {
            self.morph(parameter[0], parameter[1]);
        } else {
            self.set_frequency(parameter[0]);
            self.set_fm_amount((parameter[1] >> 2).wrapping_mul(3));
            self.set_decay(parameter[2]);
            self.set_noise(parameter[3]);
        }
    }

    pub fn set_frequency(&mut self, frequency: u16) {
        self.aux_envelope_strength = if frequency <= 16384 {
            1024
        } else if frequency <= 32768 {
            2048 - (frequency >> 4)
        } else {
            0
        };
        self.frequency = ((24 << 7) + ((72u32 << 7).wrapping_mul(frequency as u32) >> 16)) as u16;
    }

    pub fn set_fm_amount(&mut self, fm_amount: u16) {
        self.fm_amount = fm_amount >> 2;
    }

    pub fn set_decay(&mut self, decay: u16) {
        self.am_decay = 16384 + (decay >> 1);
        self.fm_decay = 8192 + (decay >> 2);
    }

    pub fn set_noise(&mut self, noise: u16) {
        let n = noise as u32;
        self.noise = if noise >= 32768 { ((n - 32768).wrapping_mul(n - 32768) >> 15) as u16 } else { 0 };
        self.noise = (self.noise >> 2).wrapping_mul(5);
        self.overdrive = if noise <= 32767 { ((32767 - n).wrapping_mul(32767 - n) >> 14) as u16 } else { 0 };
    }

    fn morph(&mut self, x: u16, y: u16) {
        let map: &[[u16; 4]; 10] = if self.sd_range { &SD_MAP } else { &BD_MAP };
        let mut parameters = [0u16; 4];
        for i in 0..4 {
            let x_integral = ((x >> 14) << 1) as usize;
            let x_fractional = (x << 2) as u32;
            let a = map[x_integral][i] as u32;
            let b = map[x_integral + 2][i] as u32;
            let c = map[x_integral + 1][i] as u32;
            let d = map[x_integral + 3][i] as u32;

            let e = a.wrapping_add(b.wrapping_sub(a).wrapping_mul(x_fractional) >> 16);
            let f = c.wrapping_add(d.wrapping_sub(c).wrapping_mul(x_fractional) >> 16);
            parameters[i] = e.wrapping_add(f.wrapping_sub(e).wrapping_mul(y as u32) >> 16) as u16;
        }
        self.configure(&parameters, ControlMode::Full);
    }

    fn compute_envelope_increment(decay: u16) -> u32 {
        let idx = (decay >> 8) as usize;
        let a = LUT_ENV_INCREMENTS[idx];
        let b = LUT_ENV_INCREMENTS[idx + 1];
        a.wrapping_sub(a.wrapping_sub(b).wrapping_mul((decay & 0xff) as u32) >> 8)
    }

    /// `midi_pitch` is `int16_t` in the C++ -- the caller's `uint32_t`
    /// running sum genuinely narrows (with sign reinterpretation) into that
    /// width at the call site, which is replicated at the one call site
    /// below rather than widening this parameter to avoid it.
    fn compute_phase_increment(midi_pitch: i16) -> u32 {
        let midi_pitch = if midi_pitch as i32 >= K_HIGHEST_NOTE { (K_HIGHEST_NOTE - 1) as i16 } else { midi_pitch };

        let mut ref_pitch = midi_pitch as i32;
        ref_pitch -= K_PITCH_TABLE_START;

        let mut num_shifts: u32 = 0;
        while ref_pitch < 0 {
            ref_pitch += K_OCTAVE;
            num_shifts += 1;
        }

        let idx = (ref_pitch >> 4) as usize;
        let a = LUT_OSCILLATOR_INCREMENTS[idx];
        let b = LUT_OSCILLATOR_INCREMENTS[idx + 1];
        let phase_increment = a.wrapping_add((((b as i32).wrapping_sub(a as i32)).wrapping_mul(ref_pitch & 0xf) >> 4) as u32);
        phase_increment.wrapping_shr(num_shifts)
    }

    pub fn process(&mut self, gate_flags: &[GateFlags], out: &mut [i16]) {
        let am_envelope_increment = Self::compute_envelope_increment(self.am_decay);
        let fm_envelope_increment = Self::compute_envelope_increment(self.fm_decay);
        let mut phase = self.phase;
        let mut fm_envelope_phase = self.fm_envelope_phase;
        let mut am_envelope_phase = self.am_envelope_phase;
        let mut aux_envelope_phase = self.aux_envelope_phase;
        let mut phase_increment = self.phase_increment;

        let size = gate_flags.len();
        for (n, (&gate_flag, o)) in gate_flags.iter().zip(out.iter_mut()).enumerate() {
            // The C keys the "recompute every 4th sample" cadence off the
            // *remaining* sample count in a `while (size--)` loop, not an
            // independent counter -- so it only lines up with the original
            // firmware's behaviour when called in blocks of exactly 4
            // (`peaks::kBlockSize`). Replicated exactly: `remaining` here is
            // the same value the C's `size` holds at that point in the loop.
            let remaining = size - 1 - n;

            if gate_flag.contains(GateFlags::RISING) {
                fm_envelope_phase = 0;
                am_envelope_phase = 0;
                aux_envelope_phase = 0;
                phase = 0x3fffu32.wrapping_mul(self.fm_amount as u32) >> 16;
            }

            fm_envelope_phase = fm_envelope_phase.wrapping_add(fm_envelope_increment);
            if fm_envelope_phase < fm_envelope_increment {
                fm_envelope_phase = 0xffffffff;
            }
            aux_envelope_phase = aux_envelope_phase.wrapping_add(4473924);
            if aux_envelope_phase < 4473924 {
                aux_envelope_phase = 0xffffffff;
            }
            if (remaining & 3) == 0 {
                let aux_envelope: u32 = 65535 - interpolate_824_u16(&LUT_ENV_EXPO, aux_envelope_phase) as u32;
                let fm_envelope: u32 = 65535 - interpolate_824_u16(&LUT_ENV_EXPO, fm_envelope_phase) as u32;
                let sum: u32 = (self.frequency as u32)
                    .wrapping_add(fm_envelope.wrapping_mul(self.fm_amount as u32) >> 16)
                    .wrapping_add(aux_envelope.wrapping_mul(self.aux_envelope_strength as u32) >> 15)
                    .wrapping_add(((self.previous_sample as i32) >> 6) as u32);
                phase_increment = Self::compute_phase_increment(sum as i16);
            }
            phase = phase.wrapping_add(phase_increment);

            let mut mix = interpolate_1022(&WAV_SINE, phase);
            if self.noise != 0 {
                mix = mix_i16(mix, Random::get_sample(), self.noise);
            }

            am_envelope_phase = am_envelope_phase.wrapping_add(am_envelope_increment);
            if am_envelope_phase < am_envelope_increment {
                am_envelope_phase = 0xffffffff;
            }
            let am_envelope: u32 = 65535 - interpolate_824_u16(&LUT_ENV_EXPO, am_envelope_phase) as u32;
            mix = ((mix as i32).wrapping_mul(am_envelope as i32) >> 16) as i16;
            if self.overdrive != 0 {
                let phi = ((mix as i32) << 16).wrapping_add(1i32 << 31);
                let overdriven = interpolate_1022(&WAV_OVERDRIVE, phi as u32);
                mix = mix_i16(mix, overdriven, self.overdrive);
            }
            self.previous_sample = mix;
            *o = mix;
        }

        self.phase = phase;
        self.fm_envelope_phase = fm_envelope_phase;
        self.am_envelope_phase = am_envelope_phase;
        self.aux_envelope_phase = aux_envelope_phase;
        self.phase_increment = phase_increment;
    }
}
