//! `plaits/dsp/fx/fx_engine.h` -- shared plumbing for [`super::diffuser`] and
//! [`super::ensemble`].
//!
//! The C is a template-metaprogramming DSL: `Reserve<126, Reserve<180, ...>>`
//! computes, at compile time, `base`/`length` offsets for several named delay
//! lines packed into *one* shared circular buffer, and `Context` is a
//! sample-at-a-time accumulator with `Read`/`Write`/`WriteAllPass`/`Interpolate`
//! operating on those offsets. None of that machinery is meaningful outside
//! C++ template instantiation, so this port keeps only what it computes: one
//! `[f32; N]` ring buffer per effect (`N` a power of two) plus the handful of
//! accumulator operations `Diffuser`/`Ensemble` actually call, taking a
//! `(base, length)` pair instead of a `D: DelayLine` type parameter. Compressed
//! 12-bit storage isn't reproduced (this port always stores `f32`); it was a
//! RAM-saving trick, not part of the audible algorithm's shape.

/// One shared power-of-two ring buffer plus the running accumulator state,
/// matching `FxEngine<N>::Context` for one sample.
#[derive(Debug)]
pub struct FxBuffer<const N: usize> {
    buffer: [f32; N],
    write_ptr: i64,
    lfo: [stmlib::CosineOscillator; 2],
    lfo_value: [f32; 2],
}

impl<const N: usize> Default for FxBuffer<N> {
    fn default() -> Self {
        assert!(N.is_power_of_two());
        Self {
            buffer: [0.0; N],
            write_ptr: 0,
            lfo: [
                stmlib::CosineOscillator::new(stmlib::CosineOscillatorMode::Approximate, 0.001),
                stmlib::CosineOscillator::new(stmlib::CosineOscillatorMode::Approximate, 0.001),
            ],
            lfo_value: [0.0; 2],
        }
    }
}

/// A named delay line's `(base, length)` within the shared buffer -- the
/// runtime equivalent of the C's `DelayLine<Memory, index>` type.
#[derive(Debug, Clone, Copy)]
pub struct Tap {
    pub base: i64,
    pub length: i64,
}

impl<const N: usize> FxBuffer<N> {
    pub fn clear(&mut self) {
        self.buffer = [0.0; N];
        self.write_ptr = 0;
    }

    /// `SetLFOFrequency(index, frequency)`.
    pub fn set_lfo_frequency(&mut self, index: usize, frequency: f32) {
        self.lfo[index].init(stmlib::CosineOscillatorMode::Approximate, frequency * 32.0);
    }

    #[inline]
    fn index(&self, offset: i64) -> usize {
        ((self.write_ptr + offset) & (N as i64 - 1)) as usize
    }

    /// `engine_.Start(&c)`.
    #[inline]
    pub fn start(&mut self) -> FxContext<'_, N> {
        self.write_ptr -= 1;
        if self.write_ptr < 0 {
            self.write_ptr += N as i64;
        }
        if self.write_ptr & 31 == 0 {
            self.lfo_value[0] = self.lfo[0].next();
            self.lfo_value[1] = self.lfo[1].next();
        } else {
            self.lfo_value[0] = self.lfo[0].value();
            self.lfo_value[1] = self.lfo[1].value();
        }
        let lfo_value = self.lfo_value;
        FxContext {
            buf: self,
            accumulator: 0.0,
            previous_read: 0.0,
            lfo_value,
        }
    }
}

/// `FxEngine::Context` for one sample.
pub struct FxContext<'a, const N: usize> {
    buf: &'a mut FxBuffer<N>,
    accumulator: f32,
    previous_read: f32,
    lfo_value: [f32; 2],
}

/// `TAIL` (`, -1`): read/write the last sample of a delay line's segment
/// (`D::base + D::length - 1`) instead of its head (`D::base`).
pub const TAIL: i64 = -1;

impl<const N: usize> FxContext<'_, N> {
    #[inline]
    fn resolve(&self, tap: Tap, offset: i64) -> i64 {
        if offset == TAIL {
            tap.base + tap.length - 1
        } else {
            tap.base + offset
        }
    }

    /// `c.Read(value)` / `c.Read(value, scale)`.
    #[inline]
    pub fn read_value(&mut self, value: f32, scale: f32) {
        self.accumulator += value * scale;
    }

    /// `c.Read(tap, offset, scale)`.
    #[inline]
    pub fn read(&mut self, tap: Tap, offset: i64, scale: f32) {
        let idx = self.buf.index(self.resolve(tap, offset));
        let r = self.buf.buffer[idx];
        self.previous_read = r;
        self.accumulator += r * scale;
    }

    /// `c.Write(out)` / `c.Write(out, scale)` -- write the accumulator to an
    /// output sample, then (optionally) scale it for further use.
    #[inline]
    pub fn write_out(&mut self, out: &mut f32, scale: f32) {
        *out = self.accumulator;
        self.accumulator *= scale;
    }

    /// `c.Write(tap, offset, scale)`.
    #[inline]
    pub fn write(&mut self, tap: Tap, offset: i64, scale: f32) {
        let idx = self.buf.index(self.resolve(tap, offset));
        self.buf.buffer[idx] = self.accumulator;
        self.accumulator *= scale;
    }

    /// `c.WriteAllPass(tap, offset, scale)`.
    #[inline]
    pub fn write_allpass(&mut self, tap: Tap, offset: i64, scale: f32) {
        self.write(tap, offset, scale);
        self.accumulator += self.previous_read;
    }

    /// `c.Lp(state, coefficient)`.
    #[inline]
    pub fn lp(&mut self, state: &mut f32, coefficient: f32) {
        *state += coefficient * (self.accumulator - *state);
        self.accumulator = *state;
    }

    /// `c.Interpolate(tap, offset, scale)` -- fractional read.
    #[inline]
    pub fn interpolate(&mut self, tap: Tap, offset: f32, scale: f32) {
        let offset_integral = offset as i64;
        let offset_fractional = offset - offset_integral as f32;
        let base = self.resolve(tap, offset_integral);
        let ia = self.buf.index(base);
        let ib = self.buf.index(base + 1);
        let a = self.buf.buffer[ia];
        let b = self.buf.buffer[ib];
        let x = a + (b - a) * offset_fractional;
        self.previous_read = x;
        self.accumulator += x * scale;
    }

    /// `c.Interpolate(tap, offset, lfo_index, amplitude, scale)`.
    #[inline]
    pub fn interpolate_lfo(
        &mut self,
        tap: Tap,
        offset: f32,
        lfo_index: usize,
        amplitude: f32,
        scale: f32,
    ) {
        self.interpolate(tap, offset + amplitude * self.lfo_value[lfo_index], scale);
    }
}
