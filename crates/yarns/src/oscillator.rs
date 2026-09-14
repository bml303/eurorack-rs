//! `yarns/voice.{h,cc}` -- `Oscillator`: the audio-rate BLEP oscillator
//! driving Yarns' 4 "audio" modes (saw, two square variants, sine) plus a
//! silence/noise fallback, feeding a small ring buffer the DAC ISR drains.

use stmlib::RingBuffer;
use stmlib::fixed::interpolate_1022;
use stmlib::random::Random;

use crate::resources::WAV_SINE;

pub const AUDIO_BLOCK_SIZE: usize = 64;

const OCTAVE: i32 = 12 << 7;
const HIGHEST_NOTE: i16 = 128 * 128;
const PITCH_TABLE_START: i32 = 116 * 128;

/// `AudioMode` raw values, matching what `Oscillator::render`'s dispatch
/// (`match (mode & 0x0f) - 1`) actually implements -- **not** the C's
/// `enum AudioMode { AUDIO_MODE_OFF, _SAW, _SQUARE, _TRIANGLE, _SINE }` in
/// `voice.h`, whose declared values (0-4) are a *different*, UI-facing
/// numbering that `settings.cc` (out of scope for this port, see
/// `PORTING.md`) translates into the byte `Voice::set_audio_mode`/
/// `Oscillator::render` actually take (`Voice::set_audio_mode`'s own
/// parameter is already typed `uint8_t`, not `AudioMode`, in the C++ -- a
/// tell that the caller is expected to pass an already-translated code, not
/// the enum value). Reproducing the enum's numbers here as if they were
/// dispatch codes would be a real footgun: `AUDIO_MODE_SINE == 4` would
/// dispatch to the *triangle* branch below, not sine.
pub mod audio_mode {
    pub const OFF: u8 = 0;
    pub const SAW: u8 = 1;
    /// 25% duty pulse.
    pub const SQUARE_NARROW: u8 = 2;
    /// 50% duty pulse.
    pub const SQUARE: u8 = 3;
    /// 50% duty pulse, leaky-integrated into a triangle-ish shape.
    pub const TRIANGLE: u8 = 4;
    pub const SINE: u8 = 5;
    /// Anything else (low nibble `>= 6`) renders noise.
    pub const NOISE: u8 = 6;
    /// OR'd into the low nibble: silence (rather than the waveform) while
    /// the gate is low.
    pub const GATED: u8 = 0x80;
}

#[inline]
fn this_blep_sample(t: u32) -> i32 {
    let t = t.min(65535);
    ((t * t) >> 18) as i32
}

#[inline]
fn next_blep_sample(t: u32) -> i32 {
    let t = t.min(65535);
    let t = 65535 - t;
    -(((t * t) >> 18) as i32)
}

#[derive(Debug, Clone, Default)]
pub struct Oscillator {
    scale: i32,
    offset: i32,
    phase: u32,
    next_sample: i32,
    integrator_state: i32,
    high: bool,
    audio_buffer: RingBuffer<u16, { AUDIO_BLOCK_SIZE * 2 }>,
}

impl Oscillator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn init(&mut self, scale: i32, offset: i32) {
        self.audio_buffer.init();
        self.phase = 0;
        self.next_sample = 0;
        self.high = false;
        self.scale = scale;
        self.offset = offset;
        self.integrator_state = 0;
    }

    pub fn read_sample(&mut self) -> u16 {
        self.audio_buffer.immediate_read()
    }

    fn compute_phase_increment(&self, midi_pitch: i16) -> u32 {
        let midi_pitch = midi_pitch.min(HIGHEST_NOTE - 1);

        let mut ref_pitch = midi_pitch as i32 - PITCH_TABLE_START;

        let mut num_shifts = 0u32;
        while ref_pitch < 0 {
            ref_pitch += OCTAVE;
            num_shifts += 1;
        }

        let table = &crate::resources::LUT_OSCILLATOR_INCREMENTS;
        let a = table[(ref_pitch >> 4) as usize];
        let b = table[(ref_pitch >> 4) as usize + 1];
        let phase_increment =
            a.wrapping_add(((b.wrapping_sub(a) as i32).wrapping_mul(ref_pitch & 0xf) >> 4) as u32);
        phase_increment >> num_shifts
    }

    fn render_silence(&mut self) {
        for _ in 0..AUDIO_BLOCK_SIZE {
            self.audio_buffer.overwrite(self.offset as u16);
        }
    }

    fn render_sine(&mut self, phase_increment: u32) {
        for _ in 0..AUDIO_BLOCK_SIZE {
            self.phase = self.phase.wrapping_add(phase_increment);
            let sample = interpolate_1022(&WAV_SINE, self.phase) as i32;
            let out = self.offset.wrapping_sub((self.scale * sample) >> 16);
            self.audio_buffer.overwrite(out as u16);
        }
    }

    fn render_noise(&mut self) {
        for _ in 0..AUDIO_BLOCK_SIZE {
            let sample = Random::get_sample() as i32;
            let out = self.offset.wrapping_sub((self.scale * sample) >> 16);
            self.audio_buffer.overwrite(out as u16);
        }
    }

    fn render_saw(&mut self, phase_increment: u32) {
        let mut phase = self.phase;
        let mut next_sample = self.next_sample;

        for _ in 0..AUDIO_BLOCK_SIZE {
            let mut this_sample = next_sample;
            next_sample = 0;
            phase = phase.wrapping_add(phase_increment);
            if phase < phase_increment {
                let t = phase / (phase_increment >> 16);
                this_sample -= this_blep_sample(t);
                next_sample -= next_blep_sample(t);
            }
            next_sample += (phase >> 17) as i32;
            this_sample = (this_sample - 16384) << 1;
            let out = self.offset.wrapping_sub((self.scale * this_sample) >> 16);
            self.audio_buffer.overwrite(out as u16);
        }
        self.next_sample = next_sample;
        self.phase = phase;
    }

    fn render_square(&mut self, phase_increment: u32, pw: u32, integrate: bool) {
        let mut phase = self.phase;
        let mut next_sample = self.next_sample;
        let mut integrator_state = self.integrator_state;
        let integrator_coefficient = (phase_increment >> 18) as i16 as i32;

        for _ in 0..AUDIO_BLOCK_SIZE {
            let mut this_sample = next_sample;
            next_sample = 0;
            phase = phase.wrapping_add(phase_increment);

            if !self.high && phase >= pw {
                let t = (phase - pw) / (phase_increment >> 16);
                this_sample += this_blep_sample(t);
                next_sample += next_blep_sample(t);
                self.high = true;
            }
            if self.high && phase < phase_increment {
                let t = phase / (phase_increment >> 16);
                this_sample -= this_blep_sample(t);
                next_sample -= next_blep_sample(t);
                self.high = false;
            }
            next_sample += if phase < pw { 0 } else { 32767 };
            this_sample = (this_sample - 16384) << 1;
            if integrate {
                integrator_state += (integrator_coefficient * (this_sample - integrator_state)) >> 15;
                this_sample = integrator_state << 3;
            }
            let out = self.offset.wrapping_sub((self.scale * this_sample) >> 16);
            self.audio_buffer.overwrite(out as u16);
        }
        self.integrator_state = integrator_state;
        self.next_sample = next_sample;
        self.phase = phase;
    }

    pub fn render(&mut self, mode: u8, note: i16, gate: bool) {
        if mode == 0 || self.audio_buffer.writable() < AUDIO_BLOCK_SIZE {
            return;
        }

        if (mode & 0x80) != 0 && !gate {
            self.render_silence();
            return;
        }

        let phase_increment = self.compute_phase_increment(note);
        match (mode & 0x0f) - 1 {
            0 => self.render_saw(phase_increment),
            1 => self.render_square(phase_increment, 0x4000_0000, false),
            2 => self.render_square(phase_increment, 0x8000_0000, false),
            3 => self.render_square(phase_increment, 0x8000_0000, true),
            4 => self.render_sine(phase_increment),
            _ => self.render_noise(),
        }
    }
}
