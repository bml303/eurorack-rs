//! `stmlib/dsp/delay_line.h` -- a fixed-capacity circular delay line.
//!
//! The C is a `template<typename T, size_t max_delay>`; every user in this
//! workspace instantiates it with `T = float`, so this is `f32`-only (a generic
//! version can be added if a future port needs `T = int16_t` etc).

#[derive(Debug, Clone)]
pub struct DelayLine<const N: usize> {
    line: [f32; N],
    write_ptr: usize,
    delay: usize,
}

impl<const N: usize> Default for DelayLine<N> {
    fn default() -> Self {
        Self {
            line: [0.0; N],
            write_ptr: 0,
            delay: 1,
        }
    }
}

impl<const N: usize> DelayLine<N> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn init(&mut self) {
        self.reset();
    }

    pub fn reset(&mut self) {
        self.line = [0.0; N];
        self.delay = 1;
        self.write_ptr = 0;
    }

    #[inline]
    pub fn set_delay(&mut self, delay: usize) {
        self.delay = delay;
    }

    #[inline]
    pub fn write(&mut self, sample: f32) {
        self.line[self.write_ptr] = sample;
        self.write_ptr = (self.write_ptr + N - 1) % N;
    }

    #[inline]
    pub fn allpass(&mut self, sample: f32, delay: usize, coefficient: f32) -> f32 {
        let read = self.line[(self.write_ptr + delay) % N];
        let write = sample + coefficient * read;
        self.write(write);
        -write * coefficient + read
    }

    #[inline]
    pub fn write_read(&mut self, sample: f32, delay: f32) -> f32 {
        self.write(sample);
        self.read_frac(delay)
    }

    #[inline]
    pub fn read(&self) -> f32 {
        self.line[(self.write_ptr + self.delay) % N]
    }

    #[inline]
    pub fn read_at(&self, delay: usize) -> f32 {
        self.line[(self.write_ptr + delay) % N]
    }

    #[inline]
    pub fn read_frac(&self, delay: f32) -> f32 {
        let delay_integral = delay as i32;
        let delay_fractional = delay - delay_integral as f32;
        let a = self.line[(self.write_ptr + delay_integral as usize) % N];
        let b = self.line[(self.write_ptr + delay_integral as usize + 1) % N];
        a + (b - a) * delay_fractional
    }

    pub fn read_hermite(&self, delay: f32) -> f32 {
        let delay_integral = delay as i32;
        let delay_fractional = delay - delay_integral as f32;
        let t = self.write_ptr as i64 + delay_integral as i64 + N as i64;
        let at = |offset: i64| -> f32 { self.line[((t + offset).rem_euclid(N as i64)) as usize] };
        let xm1 = at(-1);
        let x0 = at(0);
        let x1 = at(1);
        let x2 = at(2);
        let c = (x1 - xm1) * 0.5;
        let v = x0 - x1;
        let w = c + v;
        let a = w + v + (x2 - x0) * 0.5;
        let b_neg = w + a;
        let f = delay_fractional;
        (((a * f) - b_neg) * f + c) * f + x0
    }
}
