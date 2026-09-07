//! `edges/digital_oscillator.{h,cc}` -- the sampled oscillator on channel 4.
//!
//! Six shapes share a 24-bit phase accumulator (`{ u16 integral; u8 fractional }`
//! in the C, a plain `u32` masked to 24 bits here). The wavetables are 513 bytes
//! and are addressed exactly as the AVR inline-asm `InterpolateSample` does:
//! index `= phase.integral >> 7`, blend weight `w = (phase.integral & 0x7f) << 1`
//! (an even number in `0..=254`), result `table[i] * (255 - w) + table[i+1] * w`.

use crate::resources::{
    LUT_RES_BITCRUSHER_INCREMENTS, LUT_RES_OSCILLATOR_INCREMENTS, WAV_RES_BANDLIMITED_TRIANGLE_6,
    WAVEFORM_TABLE,
};

/// `kAudioBlockSize` -- the firmware renders this many samples per `Render()`
/// call. [`DigitalOscillator::render`] accepts any length but, like the C,
/// recomputes the phase increment once per call.
pub const AUDIO_BLOCK_SIZE: usize = 16;

const K_MAX_ZONE: usize = 7;
const K_OCTAVE: i16 = 12 * 128;
const K_PITCH_TABLE_START: i16 = 116 * 128;

/// `OscillatorShape`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OscillatorShape {
    Triangle = 0,
    NesTriangle = 1,
    PitchedNoise = 2,
    NesNoiseLong = 3,
    NesNoiseShort = 4,
    Sine = 5,
}

#[inline]
fn u24_add(a: u32, b: u32) -> u32 {
    a.wrapping_add(b) & 0x00ff_ffff
}

/// The C's `phase.integral` -- the top 16 bits of the 24-bit accumulator.
#[inline]
fn integral(phase: u32) -> u16 {
    (phase >> 8) as u16
}

/// `InterpolateSample` (AVR inline asm) -- high byte of the blended pair.
#[inline]
fn interpolate_sample_8(table: &[u8], phase: u16) -> u8 {
    let i = (phase >> 7) as usize;
    let w = (phase & 0x7f) << 1; // 0..=254, even
    let a = table[i] as u16;
    let b = table[i + 1] as u16;
    ((a * (255 - w) + b * w) >> 8) as u8
}

/// `InterpolateSample16` -- the full 16-bit blended pair.
#[inline]
fn interpolate_sample_16(table: &[u8], phase: u16) -> u16 {
    let i = (phase >> 7) as usize;
    let w = (phase & 0x7f) << 1;
    let a = table[i] as u16;
    let b = table[i + 1] as u16;
    a * (255 - w) + b * w
}

/// `InterpolateTwoTables`.
#[inline]
fn interpolate_two_tables(
    table_a: &[u8],
    table_b: &[u8],
    phase: u16,
    gain_a: u8,
    gain_b: u8,
) -> u16 {
    let mut result: u16 = 0;
    result = result.wrapping_add(interpolate_sample_8(table_a, phase) as u16 * gain_a as u16);
    result = result.wrapping_add(interpolate_sample_8(table_b, phase) as u16 * gain_b as u16);
    result
}

/// `edges::DigitalOscillator`.
#[derive(Debug, Clone)]
pub struct DigitalOscillator {
    shape: OscillatorShape,
    pitch: i16,
    gate: bool,

    note: u8,
    phase: u32,
    phase_increment: u32,
    sample: u16,
    rng_state: u16,
    aux_phase: u16,
    cv_pw: u8,
}

impl Default for DigitalOscillator {
    fn default() -> Self {
        Self::new()
    }
}

impl DigitalOscillator {
    pub fn new() -> Self {
        let mut o = Self {
            shape: OscillatorShape::Triangle,
            pitch: 60 << 7,
            gate: true,
            note: 0,
            phase: 0,
            phase_increment: 0,
            sample: 0,
            rng_state: 1,
            aux_phase: 0,
            cv_pw: 0,
        };
        o.init();
        o
    }

    /// `Init`.
    pub fn init(&mut self) {
        self.update_pitch(60 << 7, OscillatorShape::Triangle);
        self.gate(true);
        self.rng_state = 1;
    }

    /// `UpdatePitch(pitch, shape)` -- `pitch` is `midi_note << 7`.
    #[inline]
    pub fn update_pitch(&mut self, pitch: i16, shape: OscillatorShape) {
        self.pitch = pitch;
        self.shape = shape;
    }

    /// `Gate(gate)`.
    #[inline]
    pub fn gate(&mut self, gate: bool) {
        self.gate = gate;
    }

    /// `set_cv_pw(pw)` -- bit-crush rate for [`OscillatorShape::Sine`].
    #[inline]
    pub fn set_cv_pw(&mut self, pw: u8) {
        self.cv_pw = pw;
    }

    fn compute_phase_increment(&mut self) {
        let mut ref_pitch: i16 = self.pitch.wrapping_sub(K_PITCH_TABLE_START);
        let mut num_shifts: i32 = if (self.shape as u8) >= (OscillatorShape::PitchedNoise as u8) {
            0
        } else {
            1
        };
        while ref_pitch < 0 {
            ref_pitch = ref_pitch.wrapping_add(K_OCTAVE);
            num_shifts += 1;
        }

        let idx_int = ((ref_pitch as u16) >> 4) as usize;
        // The C indexes `lut_res_oscillator_increments` (97 entries) without an
        // upper bound; a MIDI note stays well inside it. Clamp so an
        // out-of-range pitch reads the last cell instead of past the array.
        let idx_int = idx_int.min(LUT_RES_OSCILLATOR_INCREMENTS.len() - 2);
        let idx_frac: u8 = (ref_pitch as u8) << 4; // (ref_pitch & 0x0f) << 4

        let inc16 = LUT_RES_OSCILLATOR_INCREMENTS[idx_int];
        let inc16_next = LUT_RES_OSCILLATOR_INCREMENTS[idx_int + 1];
        let delta = inc16_next.wrapping_sub(inc16);
        let interp = ((delta as u32 * idx_frac as u32) >> 8) as u16;
        let increment_integral = inc16.wrapping_add(interp);

        let mut increment: u32 = (increment_integral as u32) << 8; // fractional = 0
        for _ in 0..num_shifts {
            increment >>= 1;
        }

        let mut note = (self.pitch as u16) >> 7;
        if note < 12 {
            note = 12;
        }
        self.note = note as u8;
        self.phase_increment = increment & 0x00ff_ffff;
    }

    /// `Render` -- fills `out` with 12-bit unsigned samples (`0..=4095`,
    /// silence = 2048).
    pub fn render(&mut self, out: &mut [u16]) {
        if self.gate {
            self.compute_phase_increment();
            match self.shape {
                OscillatorShape::Triangle | OscillatorShape::NesTriangle => {
                    self.render_bandlimited_triangle(out)
                }
                OscillatorShape::PitchedNoise => self.render_noise(out),
                OscillatorShape::NesNoiseLong | OscillatorShape::NesNoiseShort => {
                    self.render_noise_nes(out)
                }
                OscillatorShape::Sine => self.render_sine(out),
            }
        } else {
            out.fill(2048);
        }
    }

    fn render_bandlimited_triangle(&mut self, out: &mut [u16]) {
        let balance_index = swap4(self.note.wrapping_sub(12));
        let gain_2 = balance_index & 0xf0;
        let gain_1 = !gain_2;

        let wave_index = (balance_index & 0x0f) as usize;
        let base = if self.shape == OscillatorShape::NesTriangle {
            8
        } else {
            0
        };
        let last = WAVEFORM_TABLE.len() - 1;
        let wave_1 = WAVEFORM_TABLE[(base + wave_index).min(last)];
        let wave_index_2 = (wave_index + 1).min(K_MAX_ZONE);
        let wave_2 = WAVEFORM_TABLE[(base + wave_index_2).min(last)];

        let inc = self.phase_increment;
        let mut phase = self.phase;
        for o in out.iter_mut() {
            phase = u24_add(phase, inc);
            let sample = interpolate_two_tables(wave_1, wave_2, integral(phase), gain_1, gain_2);
            *o = sample >> 4;
        }
        self.phase = phase;
    }

    fn render_sine(&mut self, out: &mut [u16]) {
        let aux_phase_increment = LUT_RES_BITCRUSHER_INCREMENTS[self.cv_pw as usize];
        let inc = self.phase_increment;
        let mut phase = self.phase;
        for o in out.iter_mut() {
            phase = u24_add(phase, inc);
            self.aux_phase = self.aux_phase.wrapping_add(aux_phase_increment);
            if self.aux_phase < aux_phase_increment || aux_phase_increment == 0 {
                self.sample =
                    interpolate_sample_16(&WAV_RES_BANDLIMITED_TRIANGLE_6, integral(phase));
            }
            *o = self.sample >> 4;
        }
        self.phase = phase;
    }

    fn render_noise(&mut self, out: &mut [u16]) {
        let inc = self.phase_increment;
        let inc_integral = integral(inc);
        let mut phase = self.phase;
        let mut rng_state = self.rng_state;
        let mut sample = self.sample;
        for o in out.iter_mut() {
            phase = u24_add(phase, inc);
            if integral(phase) < inc_integral {
                rng_state = (rng_state >> 1) ^ (0u16.wrapping_sub(rng_state & 1) & 0xb400);
                let s = rng_state & 0x0fff;
                sample = 512 + ((s * 3) >> 2);
            }
            *o = sample;
        }
        self.phase = phase;
        self.rng_state = rng_state;
        self.sample = sample;
    }

    fn render_noise_nes(&mut self, out: &mut [u16]) {
        let inc = self.phase_increment;
        let inc_integral = integral(inc);
        let short = self.shape == OscillatorShape::NesNoiseShort;
        let mut phase = self.phase;
        let mut rng_state = self.rng_state;
        let mut sample = self.sample;
        for o in out.iter_mut() {
            phase = u24_add(phase, inc);
            if integral(phase) < inc_integral {
                let mut tap = rng_state >> 1;
                if short {
                    tap >>= 5;
                }
                let random_bit = (rng_state ^ tap) & 1;
                rng_state >>= 1;
                if random_bit != 0 {
                    rng_state |= 0x4000;
                    sample = 0x0300;
                } else {
                    sample = 0x0cff;
                }
            }
            *o = sample;
        }
        self.phase = phase;
        self.rng_state = rng_state;
        self.sample = sample;
    }
}

/// `U8Swap4` -- swap the nibbles of a byte.
#[inline]
fn swap4(a: u8) -> u8 {
    a.rotate_left(4)
}
