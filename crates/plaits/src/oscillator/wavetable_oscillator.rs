//! `plaits/dsp/oscillator/wavetable_oscillator.h` -- integrated (differentiated
//! parabolic wave, "DPW") wavetable synthesis: cross-fades both within a
//! table (linear) and across a stack of tables (by waveform position).
//!
//! `wavetable_size` / `num_waves` are runtime parameters here (not C++
//! template parameters); `approximate_scale` / `attenuate_high_frequencies`
//! are runtime bools for the same reason as elsewhere in this port.

use stmlib::fdsp::one_pole;
use stmlib::parameter_interpolator::ParameterInterpolator;

use super::oscillator::MAX_FREQUENCY;

/// `Differentiator` -- a leaky differentiator (`(s - previous) -> one-pole`),
/// the second half of the DPW technique.
#[derive(Debug, Clone, Copy, Default)]
pub struct Differentiator {
    lp: f32,
    previous: f32,
}

impl Differentiator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn init(&mut self) {
        self.previous = 0.0;
        self.lp = 0.0;
    }

    pub fn process(&mut self, coefficient: f32, s: f32) -> f32 {
        one_pole(&mut self.lp, s - self.previous, coefficient);
        self.previous = s;
        self.lp
    }
}

#[inline]
pub fn interpolate_wave(table: &[i16], index_integral: usize, index_fractional: f32) -> f32 {
    let a = table[index_integral] as f32;
    let b = table[index_integral + 1] as f32;
    a + (b - a) * index_fractional
}

#[inline]
pub fn interpolate_wave_hermite(
    table: &[i16],
    index_integral: usize,
    index_fractional: f32,
) -> f32 {
    let xm1 = table[index_integral] as f32;
    let x0 = table[index_integral + 1] as f32;
    let x1 = table[index_integral + 2] as f32;
    let x2 = table[index_integral + 3] as f32;
    let c = (x1 - xm1) * 0.5;
    let v = x0 - x1;
    let w = c + v;
    let a = w + v + (x2 - x0) * 0.5;
    let b_neg = w + a;
    let f = index_fractional;
    (((a * f) - b_neg) * f + c) * f + x0
}

#[derive(Debug, Clone, Copy, Default)]
pub struct WavetableOscillator {
    phase: f32,
    frequency: f32,
    amplitude: f32,
    waveform: f32,
    lp: f32,
    differentiator: Differentiator,
}

impl WavetableOscillator {
    pub fn init(&mut self) {
        *self = Self::default();
        self.differentiator.init();
    }

    /// `wavetable` must have at least `num_waves + 1` rows (the C relies on a
    /// padding row past the last real waveform); each row has
    /// `wavetable_size + 1` samples (the last a wrapped copy of the first).
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        wavetable_size: usize,
        num_waves: usize,
        approximate_scale: bool,
        attenuate_high_frequencies: bool,
        frequency: f32,
        mut amplitude: f32,
        waveform: f32,
        wavetable: &[&[i16]],
        out: &mut [f32],
    ) {
        let frequency = frequency.clamp(0.0000001, MAX_FREQUENCY);

        if attenuate_high_frequencies {
            amplitude *= 1.0 - 2.0 * frequency;
        }
        if approximate_scale {
            amplitude *= 1.0 / (frequency * 131072.0);
        }

        let size = out.len();
        let mut fm = ParameterInterpolator::new(&mut self.frequency, frequency, size);
        let mut am = ParameterInterpolator::new(&mut self.amplitude, amplitude, size);
        let mut wm = ParameterInterpolator::new(
            &mut self.waveform,
            waveform * (num_waves as f32 - 1.0001),
            size,
        );

        let mut lp = self.lp;
        let mut phase = self.phase;

        for o in out.iter_mut() {
            let f0 = fm.next();
            let cutoff = (wavetable_size as f32 * f0).min(1.0);
            let scale = if approximate_scale {
                1.0
            } else {
                1.0 / (f0 * 131072.0)
            };

            phase += f0;
            if phase >= 1.0 {
                phase -= 1.0;
            }

            let waveform = wm.next();
            let waveform_integral = waveform as i32 as usize;
            let waveform_fractional = waveform - waveform_integral as f32;

            let p = phase * wavetable_size as f32;
            let p_integral = p as i32 as usize;
            let p_fractional = p - p_integral as f32;

            let x0 = interpolate_wave(wavetable[waveform_integral], p_integral, p_fractional);
            let x1 = interpolate_wave(wavetable[waveform_integral + 1], p_integral, p_fractional);

            let s = self
                .differentiator
                .process(cutoff, (x0 + (x1 - x0) * waveform_fractional) * scale);
            one_pole(&mut lp, s, cutoff);
            *o += am.next() * lp;
        }
        self.lp = lp;
        self.phase = phase;
    }
}
