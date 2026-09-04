//! `stmlib/dsp/cosine_oscillator.h` -- a resonator-based cosine generator that
//! outputs values in `[0, 1]` with one multiply/add per sample.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CosineOscillatorMode {
    /// Cheap polynomial approximation of the IIR coefficient.
    Approximate,
    /// Exact `2 cos(2 pi f)` coefficient (uses `cosf`).
    Exact,
}

#[derive(Debug, Clone, Copy)]
pub struct CosineOscillator {
    y1: f32,
    y0: f32,
    iir_coefficient: f32,
    initial_amplitude: f32,
}

impl CosineOscillator {
    /// Build and reset for a normalised `frequency` (cycles per sample).
    pub fn new(mode: CosineOscillatorMode, frequency: f32) -> Self {
        let mut osc = Self {
            y1: 0.0,
            y0: 0.0,
            iir_coefficient: 0.0,
            initial_amplitude: 0.0,
        };
        osc.init(mode, frequency);
        osc
    }

    pub fn init(&mut self, mode: CosineOscillatorMode, frequency: f32) {
        match mode {
            CosineOscillatorMode::Approximate => self.init_approximate(frequency),
            CosineOscillatorMode::Exact => {
                self.iir_coefficient = 2.0 * libm::cosf(2.0 * core::f32::consts::PI * frequency);
                self.initial_amplitude = self.iir_coefficient * 0.25;
            }
        }
        self.start();
    }

    pub fn init_approximate(&mut self, mut frequency: f32) {
        let mut sign = 16.0;
        frequency -= 0.25;
        if frequency < 0.0 {
            frequency = -frequency;
        } else if frequency > 0.5 {
            frequency -= 0.5;
        } else {
            sign = -16.0;
        }
        self.iir_coefficient = sign * frequency * (1.0 - 2.0 * frequency);
        self.initial_amplitude = self.iir_coefficient * 0.25;
    }

    #[inline]
    pub fn start(&mut self) {
        self.y1 = self.initial_amplitude;
        self.y0 = 0.5;
    }

    #[inline]
    pub fn value(&self) -> f32 {
        self.y1 + 0.5
    }

    #[inline]
    pub fn next(&mut self) -> f32 {
        let temp = self.y0;
        self.y0 = self.iir_coefficient * self.y0 - self.y1;
        self.y1 = temp;
        temp + 0.5
    }
}
