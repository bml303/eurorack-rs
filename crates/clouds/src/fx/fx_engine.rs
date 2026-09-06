//! `clouds/dsp/fx/fx_engine.h` -- the tiny "delay-memory + accumulator"
//! virtual machine every clouds effect is written against (Dattorro-style).
//!
//! The C is template metaprogramming: `Reserve<...>` chains compute each named
//! delay line's base offset at compile time, and a `Context` object threads an
//! accumulator through `Read` / `Write` / `WriteAllPass` / `Interpolate` /
//! `Lp` / `Hp` steps. Here the base offsets are `const` arrays each effect
//! computes with [`bases`], and [`Context`] is a short-lived borrow of the
//! ring buffer. Sample storage format (12- / 16- / 32-bit) is a generic
//! [`Format`] parameter, preserved because the 12-bit reverb's quantisation is
//! audible.

use alloc::boxed::Box;
use alloc::vec;

use stmlib::fdsp::clip16;
use stmlib::{CosineOscillator, CosineOscillatorMode};

/// Offset meaning "the tail of the delay line" (`TAIL` in the C).
pub const TAIL: i32 = -1;

/// Compile-time base offset of each delay line, matching the C's
/// `DelayLine<Memory, i>::base` recurrence (`+1` guard word per line).
pub const fn bases<const N: usize>(lengths: [usize; N]) -> [usize; N] {
    let mut b = [0usize; N];
    let mut i = 1;
    while i < N {
        b[i] = b[i - 1] + lengths[i - 1] + 1;
        i += 1;
    }
    b
}

/// Ring-buffer sample storage format (`DataType<Format>` in the C).
pub trait Format {
    /// Stored element type.
    type T: Copy + Default;
    /// `Compress` -- float sample to stored word.
    fn compress(value: f32) -> Self::T;
    /// `Decompress` -- stored word to float sample.
    fn decompress(value: Self::T) -> f32;
}

/// `FORMAT_12_BIT`.
pub struct Format12;
impl Format for Format12 {
    type T = u16;
    #[inline]
    fn compress(value: f32) -> u16 {
        clip16((value * 4096.0) as i32) as u16
    }
    #[inline]
    fn decompress(value: u16) -> f32 {
        (value as i16) as f32 / 4096.0
    }
}

/// `FORMAT_16_BIT`.
pub struct Format16;
impl Format for Format16 {
    type T = u16;
    #[inline]
    fn compress(value: f32) -> u16 {
        clip16((value * 32768.0) as i32) as u16
    }
    #[inline]
    fn decompress(value: u16) -> f32 {
        (value as i16) as f32 / 32768.0
    }
}

/// `FORMAT_32_BIT` -- no compression.
pub struct Format32;
impl Format for Format32 {
    type T = f32;
    #[inline]
    fn compress(value: f32) -> f32 {
        value
    }
    #[inline]
    fn decompress(value: f32) -> f32 {
        value
    }
}

/// `FxEngine<size, format>`.
pub struct FxEngine<F: Format, const SIZE: usize> {
    buffer: Box<[F::T]>,
    write_ptr: i32,
    lfo: [CosineOscillator; 2],
}

impl<F: Format, const SIZE: usize> FxEngine<F, SIZE> {
    /// `FxEngine()` + `Init` -- allocate and clear the ring buffer.
    pub fn new() -> Self {
        let mut e = Self {
            buffer: vec![F::T::default(); SIZE].into_boxed_slice(),
            write_ptr: 0,
            lfo: [
                CosineOscillator::new(CosineOscillatorMode::Approximate, 0.0),
                CosineOscillator::new(CosineOscillatorMode::Approximate, 0.0),
            ],
        };
        e.clear();
        e
    }

    /// `Clear`.
    pub fn clear(&mut self) {
        for s in self.buffer.iter_mut() {
            *s = F::T::default();
        }
        self.write_ptr = 0;
    }

    /// `SetLFOFrequency`.
    pub fn set_lfo_frequency(&mut self, index: usize, frequency: f32) {
        self.lfo[index].init(CosineOscillatorMode::Approximate, frequency * 32.0);
    }

    /// `Start` -- advance the write pointer, refresh the LFOs and hand back a
    /// [`Context`] bound to the buffer for this sample.
    #[inline]
    pub fn start(&mut self) -> Context<'_, F, SIZE> {
        self.write_ptr -= 1;
        if self.write_ptr < 0 {
            self.write_ptr += SIZE as i32;
        }
        let lfo_value = if self.write_ptr & 31 == 0 {
            [self.lfo[0].next(), self.lfo[1].next()]
        } else {
            [self.lfo[0].value(), self.lfo[1].value()]
        };
        Context {
            buffer: &mut self.buffer,
            write_ptr: self.write_ptr as usize,
            accumulator: 0.0,
            previous_read: 0.0,
            lfo_value,
        }
    }
}

impl<F: Format, const SIZE: usize> Default for FxEngine<F, SIZE> {
    fn default() -> Self {
        Self::new()
    }
}

/// `FxEngine::Context` -- the accumulator machine for one output sample.
pub struct Context<'a, F: Format, const SIZE: usize> {
    buffer: &'a mut [F::T],
    write_ptr: usize,
    accumulator: f32,
    previous_read: f32,
    lfo_value: [f32; 2],
}

impl<F: Format, const SIZE: usize> Context<'_, F, SIZE> {
    const MASK: usize = SIZE - 1;

    #[inline]
    fn addr(&self, base: usize, length: usize, offset: i32) -> usize {
        let raw = if offset == TAIL {
            self.write_ptr + base + length - 1
        } else {
            self.write_ptr + base + offset as usize
        };
        raw & Self::MASK
    }

    /// `Load(value)`.
    #[inline]
    pub fn load(&mut self, value: f32) {
        self.accumulator = value;
    }

    /// `Read(value)` -- accumulate a bare float.
    #[inline]
    pub fn read(&mut self, value: f32) {
        self.accumulator += value;
    }

    /// `Read(value, scale)`.
    #[inline]
    pub fn read_scaled(&mut self, value: f32, scale: f32) {
        self.accumulator += value * scale;
    }

    /// `Write(float& value)`.
    #[inline]
    pub fn write_out(&mut self, value: &mut f32) {
        *value = self.accumulator;
    }

    /// `Write(float& value, scale)`.
    #[inline]
    pub fn write_out_scaled(&mut self, value: &mut f32, scale: f32) {
        *value = self.accumulator;
        self.accumulator *= scale;
    }

    /// `Read(DelayLine, offset, scale)`.
    #[inline]
    pub fn read_line(&mut self, base: usize, length: usize, offset: i32, scale: f32) {
        let r = F::decompress(self.buffer[self.addr(base, length, offset)]);
        self.previous_read = r;
        self.accumulator += r * scale;
    }

    /// `Write(DelayLine, offset, scale)`.
    #[inline]
    pub fn write_line(&mut self, base: usize, length: usize, offset: i32, scale: f32) {
        let idx = self.addr(base, length, offset);
        self.buffer[idx] = F::compress(self.accumulator);
        self.accumulator *= scale;
    }

    /// `WriteAllPass(DelayLine, offset, scale)`.
    #[inline]
    pub fn write_all_pass(&mut self, base: usize, length: usize, offset: i32, scale: f32) {
        self.write_line(base, length, offset, scale);
        self.accumulator += self.previous_read;
    }

    /// `Lp(state, coefficient)`.
    #[inline]
    pub fn lp(&mut self, state: &mut f32, coefficient: f32) {
        *state += coefficient * (self.accumulator - *state);
        self.accumulator = *state;
    }

    /// `Hp(state, coefficient)`.
    #[inline]
    pub fn hp(&mut self, state: &mut f32, coefficient: f32) {
        *state += coefficient * (self.accumulator - *state);
        self.accumulator -= *state;
    }

    /// `Interpolate(DelayLine, offset, scale)` -- linear tap at a fractional
    /// delay.
    #[inline]
    pub fn interpolate(&mut self, base: usize, offset: f32, scale: f32) {
        let offset_integral = offset as i32;
        let offset_fractional = offset - offset_integral as f32;
        let i0 = (self.write_ptr + offset_integral as usize + base) & Self::MASK;
        let i1 = (self.write_ptr + offset_integral as usize + base + 1) & Self::MASK;
        let a = F::decompress(self.buffer[i0]);
        let b = F::decompress(self.buffer[i1]);
        let x = a + (b - a) * offset_fractional;
        self.previous_read = x;
        self.accumulator += x * scale;
    }

    /// `Interpolate(DelayLine, offset, LFOIndex, amplitude, scale)`.
    #[inline]
    pub fn interpolate_lfo(
        &mut self,
        base: usize,
        offset: f32,
        lfo_index: usize,
        amplitude: f32,
        scale: f32,
    ) {
        let offset = offset + amplitude * self.lfo_value[lfo_index];
        self.interpolate(base, offset, scale);
    }
}
