//! `clouds/dsp/audio_buffer.h` -- the circular recording buffer that the
//! granular / stretch / looping players read from.
//!
//! The C templates on [`Resolution`] (16-bit linear or 8-bit mu-law) and on
//! the [`InterpolationMethod`]; here both are runtime enums matched per call.
//! Only the two resolutions the firmware actually instantiates are
//! implemented (`quality` selects between them).

use alloc::boxed::Box;
use alloc::vec;

use stmlib::fdsp::clip16;

use crate::mu_law::{lin_to_mu_law, mu_law_to_lin};

/// `kCrossFadeSize`.
const CROSS_FADE_SIZE: usize = 256;
/// `kInterpolationTail` -- guard samples kept past `size_` for interpolation.
pub const INTERPOLATION_TAIL: usize = 8;

/// Sample storage format of a recording buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    /// `RESOLUTION_16_BIT`.
    Bit16,
    /// `RESOLUTION_8_BIT_MU_LAW`.
    Bit8MuLaw,
}

/// `InterpolationMethod` -- picked from the grain quality.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterpolationMethod {
    /// Zero-order hold (`GRAIN_QUALITY_LOW`).
    Zoh,
    /// Linear (`GRAIN_QUALITY_MEDIUM`).
    Linear,
    /// 4-point Hermite (`GRAIN_QUALITY_HIGH`).
    Hermite,
}

enum Storage {
    S16(Box<[i16]>),
    /// Mu-law bytes (the C's `int8_t*`, read back through `MuLaw2Lin`).
    S8(Box<[u8]>),
}

/// `AudioBuffer<resolution>`.
pub struct AudioBuffer {
    resolution: Resolution,
    storage: Storage,
    /// Usable length, excluding the interpolation guard tail.
    size: i32,
    write_head: i32,
    quantization_error: f32,
    tail: Box<[i16]>,
    crossfade_counter: i32,
}

impl AudioBuffer {
    /// Create an unallocated placeholder; call [`init`](Self::init) before use.
    pub fn new() -> Self {
        Self {
            resolution: Resolution::Bit16,
            storage: Storage::S16(Box::from([])),
            size: 0,
            write_head: 0,
            quantization_error: 0.0,
            // One extra guard entry: the C reads `tail_[kCrossFadeSize -
            // crossfade_counter_]` which is `tail_[256]` when the counter
            // hits 0. That read is always scaled by a zero `gain`, so only
            // its address matters -- keep it in bounds.
            tail: vec![0i16; CROSS_FADE_SIZE + 1].into_boxed_slice(),
            crossfade_counter: 0,
        }
    }

    /// `Init` -- (re)allocate for `size` samples (including the guard tail) at
    /// the given `resolution` and clear it.
    pub fn init(&mut self, resolution: Resolution, size: i32) {
        let size_usize = size.max(0) as usize;
        self.resolution = resolution;
        self.storage = match resolution {
            Resolution::Bit16 => Storage::S16(vec![0i16; size_usize].into_boxed_slice()),
            Resolution::Bit8MuLaw => Storage::S8(vec![127u8; size_usize].into_boxed_slice()),
        };
        self.size = size - INTERPOLATION_TAIL as i32;
        self.write_head = 0;
        self.quantization_error = 0.0;
        self.crossfade_counter = 0;
        for t in self.tail.iter_mut() {
            *t = 0;
        }
    }

    /// `Resync` -- move the write head (used when restoring a frozen buffer).
    #[inline]
    pub fn resync(&mut self, head: i32) {
        self.write_head = head;
        self.crossfade_counter = 0;
    }

    #[inline]
    pub fn size(&self) -> i32 {
        self.size
    }

    #[inline]
    pub fn head(&self) -> i32 {
        self.write_head
    }

    #[inline]
    fn set_s16(&mut self, index: i32, value: i16) {
        if let Storage::S16(b) = &mut self.storage {
            b[index as usize] = value;
        }
    }

    #[inline]
    fn set_s8(&mut self, index: i32, value: u8) {
        if let Storage::S8(b) = &mut self.storage {
            b[index as usize] = value;
        }
    }

    /// `Write(float in)` -- store one sample at the write head and advance it.
    #[inline]
    pub fn write_sample(&mut self, input: f32) {
        let head = self.write_head;
        match self.resolution {
            Resolution::Bit16 => {
                let v = clip16((input * 32768.0) as i32) as i16;
                self.set_s16(head, v);
            }
            Resolution::Bit8MuLaw => {
                let sample = clip16((input * 32768.0) as i32) as i16;
                self.set_s8(head, lin_to_mu_law(sample));
            }
        }

        if head < INTERPOLATION_TAIL as i32 {
            match self.resolution {
                Resolution::Bit16 => {
                    if let Storage::S16(b) = &self.storage {
                        let v = b[head as usize];
                        self.set_s16(head + self.size, v);
                    }
                }
                Resolution::Bit8MuLaw => {
                    if let Storage::S8(b) = &self.storage {
                        let v = b[head as usize];
                        self.set_s8(head + self.size, v);
                    }
                }
            }
        }
        self.write_head += 1;
        if self.write_head >= self.size {
            self.write_head = 0;
        }
    }

    /// `WriteFade` -- write a strided block, cross-fading when recording
    /// resumes after a freeze. `input` is the strided source; `stride` counts
    /// in `f32`s.
    pub fn write_fade(&mut self, input: &[f32], count: usize, stride: usize, write: bool) {
        if !write {
            // Continue recording samples to cross-fade with when recording
            // resumes.
            if self.crossfade_counter < CROSS_FADE_SIZE as i32 {
                let mut in_ptr = 0usize;
                let mut remaining = count;
                while remaining > 0 {
                    if self.crossfade_counter < CROSS_FADE_SIZE as i32 {
                        self.tail[self.crossfade_counter as usize] =
                            clip16((input[in_ptr] * 32767.0) as i32) as i16;
                        self.crossfade_counter += 1;
                        in_ptr += stride;
                    }
                    remaining -= 1;
                }
            }
        } else if self.crossfade_counter == 0
            && self.resolution == Resolution::Bit16
            && self.write_head >= INTERPOLATION_TAIL as i32
            && self.write_head < self.size - count as i32
        {
            // Fast write for the common case.
            let mut in_ptr = 0usize;
            for _ in 0..count {
                let v = clip16((input[in_ptr] * 32767.0) as i32) as i16;
                self.set_s16(self.write_head, v);
                self.write_head += 1;
                in_ptr += stride;
            }
        } else {
            let mut in_ptr = 0usize;
            for _ in 0..count {
                let mut sample = input[in_ptr];
                if self.crossfade_counter != 0 {
                    self.crossfade_counter -= 1;
                    let tail_sample =
                        self.tail[CROSS_FADE_SIZE - self.crossfade_counter as usize] as f32;
                    let gain = self.crossfade_counter as f32 * (1.0 / CROSS_FADE_SIZE as f32);
                    sample += (tail_sample / 32768.0 - sample) * gain;
                }
                self.write_sample(sample);
                in_ptr += stride;
            }
        }
    }

    /// The C does a single `if (integral >= size_) integral -= size_;` and
    /// relies on callers keeping `integral` in `[0, 2 * size_)` (reads outside
    /// that are a latent firmware bug). `rem_euclid` is identical on that
    /// range and simply stays in bounds outside it.
    #[inline]
    fn wrap_integral(&self, integral: i32) -> i32 {
        if integral >= 0 && integral < 2 * self.size {
            if integral >= self.size {
                integral - self.size
            } else {
                integral
            }
        } else {
            integral.rem_euclid(self.size)
        }
    }

    #[inline]
    fn raw(&self, index: i32) -> f32 {
        match &self.storage {
            Storage::S16(b) => b[index as usize] as f32,
            Storage::S8(b) => mu_law_to_lin(b[index as usize]) as f32,
        }
    }

    #[inline]
    fn scale(&self) -> f32 {
        1.0 / 32768.0
    }

    /// `Read<method>(integral, fractional)`.
    #[inline]
    pub fn read(&self, method: InterpolationMethod, integral: i32, fractional: u16) -> f32 {
        match method {
            InterpolationMethod::Zoh => self.read_zoh(integral),
            InterpolationMethod::Linear => self.read_linear(integral, fractional),
            InterpolationMethod::Hermite => self.read_hermite(integral, fractional),
        }
    }

    /// `ReadZOH`.
    #[inline]
    pub fn read_zoh(&self, integral: i32) -> f32 {
        let integral = self.wrap_integral(integral);
        self.raw(integral) * self.scale()
    }

    /// `ReadLinear`.
    #[inline]
    pub fn read_linear(&self, integral: i32, fractional: u16) -> f32 {
        let integral = self.wrap_integral(integral);
        let t = fractional as f32 / 65536.0;
        let x0 = self.raw(integral);
        let x1 = self.raw(integral + 1);
        (x0 + (x1 - x0) * t) * self.scale()
    }

    /// `ReadHermite` -- Laurent de Soras's 4-point interpolator.
    #[inline]
    pub fn read_hermite(&self, integral: i32, fractional: u16) -> f32 {
        let integral = self.wrap_integral(integral);
        let t = fractional as f32 / 65536.0;
        let xm1 = self.raw(integral);
        let x0 = self.raw(integral + 1);
        let x1 = self.raw(integral + 2);
        let x2 = self.raw(integral + 3);
        let c = (x1 - xm1) * 0.5;
        let v = x0 - x1;
        let w = c + v;
        let a = w + v + (x2 - x0) * 0.5;
        let b_neg = w + a;
        ((((a * t) - b_neg) * t + c) * t + x0) * self.scale()
    }
}

impl Default for AudioBuffer {
    fn default() -> Self {
        Self::new()
    }
}
