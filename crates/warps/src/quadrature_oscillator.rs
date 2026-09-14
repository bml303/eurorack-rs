//! `warps/dsp/quadrature_oscillator.h` -- a wavetable oscillator with
//! through-zero FM and quadrature (I/Q) outputs, morphing between the 3
//! `wav_*` table pairs in `resources.rs` as `shape` sweeps `[0, 2)`.

use stmlib::fdsp::interpolate;
use stmlib::parameter_interpolator::ParameterInterpolator;

use crate::resources::WAV_TABLE;

#[derive(Debug, Clone, Copy, Default)]
pub struct QuadratureOscillator {
    one_hertz: f32,
    phase: f32,
    frequency: f32,
    shape: f32,
}

impl QuadratureOscillator {
    pub fn init(&mut self, sample_rate: f32) {
        self.one_hertz = 1.0 / sample_rate;
        self.frequency = 0.0;
        self.shape = 0.0;
        self.phase = 0.0;
    }

    pub fn render(&mut self, shape: f32, frequency: f32, i_out: &mut [f32], q_out: &mut [f32], size: usize) {
        let normalized_frequency = (frequency * self.one_hertz).clamp(-0.25, 0.25);
        let mut frequency_parameter = ParameterInterpolator::new(&mut self.frequency, normalized_frequency, size);
        let mut shape_parameter = ParameterInterpolator::new(&mut self.shape, shape, size);

        let mut phase = self.phase;
        for i in 0..size {
            phase += frequency_parameter.next();

            if phase <= 0.0 {
                phase += 1.0;
            } else if phase >= 1.0 {
                phase -= 1.0;
            }

            let shape = shape_parameter.next() * 1.9999;
            let shape_integral = shape as i32;
            let shape_fractional = shape - shape_integral as f32;

            let mut iq = [0.0f32; 2];
            for (component, slot) in iq.iter_mut().enumerate() {
                let a = interpolate(WAV_TABLE[(2 * shape_integral) as usize + component], phase, 1024.0);
                let b = interpolate(WAV_TABLE[(2 * shape_integral + 2) as usize + component], phase, 1024.0);
                *slot = a + (b - a) * shape_fractional;
            }

            i_out[i] = iq[0];
            q_out[i] = iq[1];
        }
        self.phase = phase;
    }
}
