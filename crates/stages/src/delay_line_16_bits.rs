//! `stages/delay_line_16_bits.h` -- a delay line quantized to `i16`, with
//! linear interpolation. "Like the one in stmlib/dsp, but for int16_t" (the
//! C's own comment) -- too specialized for the shared `mi-stmlib::DelayLine`
//! (which is `f32`-only), so it lives here instead.

#[derive(Debug, Clone)]
pub struct DelayLine16Bits<const MAX_DELAY: usize> {
    write_ptr: usize,
    // One extra slot: a sentinel duplicate of index 0's sample, written every
    // time the write pointer wraps, so `read()`'s `read_ptr + 1` never needs
    // a modulo.
    line: [i16; MAX_DELAY],
    sentinel: i16,
}

impl<const MAX_DELAY: usize> Default for DelayLine16Bits<MAX_DELAY> {
    fn default() -> Self {
        Self {
            write_ptr: 0,
            line: [0; MAX_DELAY],
            sentinel: 0,
        }
    }
}

impl<const MAX_DELAY: usize> DelayLine16Bits<MAX_DELAY> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn init(&mut self) {
        self.reset();
    }

    pub fn reset(&mut self) {
        self.line = [0; MAX_DELAY];
        self.sentinel = 0;
        self.write_ptr = 0;
    }

    #[inline]
    pub fn write(&mut self, sample: f32) {
        let word = (sample * 32768.0).clamp(-32768.0, 32767.0) as i32 as i16;
        self.line[self.write_ptr] = word;
        if self.write_ptr == 0 {
            self.sentinel = word;
            self.write_ptr = MAX_DELAY - 1;
        } else {
            self.write_ptr -= 1;
        }
    }

    #[inline]
    fn at(&self, index: usize) -> i16 {
        if index == MAX_DELAY {
            self.sentinel
        } else {
            self.line[index]
        }
    }

    #[inline]
    pub fn read(&self, delay: f32) -> f32 {
        let delay_integral = delay as i32;
        let delay_fractional = delay - delay_integral as f32;
        let read_ptr = (self.write_ptr + delay_integral as usize) % MAX_DELAY;
        let a = self.at(read_ptr) as f32 / 32768.0;
        let b = self.at(read_ptr + 1) as f32 / 32768.0;
        a + (b - a) * delay_fractional
    }
}
