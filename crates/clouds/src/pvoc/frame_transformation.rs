//! `clouds/dsp/pvoc/frame_transformation.{h,cc}` -- the per-STFT-slice
//! spectral processing: rectangular<->polar, magnitude store/replay across a
//! bank of "texture" buffers, warp / shift / quantize / glitch, and phase
//! (re)synthesis.
//!
//! The C type-puns a `float*` slice as `uint32_t*` to stash phase words; here
//! that region is a separate `Vec<u16>` (magnitudes) / bit-punned `f32` slots
//! (synthesis phases), which is bit-for-bit equivalent.

use alloc::vec;
use alloc::vec::Vec;

use stmlib::atan::fast_atan2r;
use stmlib::constrain;
use stmlib::fdsp::crossfade;
use stmlib::units::semitones_to_ratio;
use stmlib::Random;

use crate::dsp::interpolate_raw;
use crate::parameters::Parameters;
use crate::resources::LUT_SIN;

/// `kMaxNumTextures`.
pub const MAX_NUM_TEXTURES: usize = 7;
/// `kHighFrequencyTruncation`.
pub const HIGH_FREQUENCY_TRUNCATION: i32 = 16;

/// `kWarpPolynomials`.
const WARP_POLYNOMIALS: [[f32; 4]; 6] = [
    [10.5882, -14.8824, 5.29412, 0.0],
    [-7.3333, 9.0, -1.79167, 0.125],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.5, 0.5, 0.0],
    [-7.3333, 9.5, -2.416667, 0.25],
    [-7.3333, 9.5, -2.416667, 0.25],
];

/// `FrameTransformation`.
pub struct FrameTransformation {
    fft_size: i32,
    /// `num_textures_` -- magnitude textures (`num_textures - 1`; the last one
    /// held the phase words in the C).
    num_textures: i32,
    size: i32,

    /// `num_textures_` rows of `size` magnitudes, flat.
    textures: Vec<f32>,
    /// `phases_` (`[0..size]`) then `phases_delta_` (`[size..2*size]`).
    phases: Vec<u16>,

    glitch_algorithm: i8,
}

impl FrameTransformation {
    pub fn new() -> Self {
        Self {
            fft_size: 0,
            num_textures: 0,
            size: 0,
            textures: Vec::new(),
            phases: Vec::new(),
            glitch_algorithm: 0,
        }
    }

    /// `Init`.
    pub fn init(&mut self, fft_size: i32, num_textures: i32) {
        self.fft_size = fft_size;
        self.size = (fft_size >> 1) - HIGH_FREQUENCY_TRUNCATION;
        self.num_textures = num_textures - 1;
        let size = self.size as usize;
        self.textures = vec![0.0; self.num_textures.max(0) as usize * size];
        self.phases = vec![0u16; 2 * size];
        self.glitch_algorithm = 0;
        self.reset();
    }

    /// `Reset`.
    pub fn reset(&mut self) {
        for t in self.textures.iter_mut() {
            *t = 0.0;
        }
    }

    #[inline]
    fn texture(&self, t: i32) -> &[f32] {
        let s = self.size as usize;
        let base = t as usize * s;
        &self.textures[base..base + s]
    }

    /// `fast_p2r`.
    #[inline]
    fn fast_p2r(magnitude: f32, angle: u16) -> (f32, f32) {
        let angle = (angle >> 6) as usize;
        let re = magnitude * LUT_SIN[angle + 256];
        let im = magnitude * LUT_SIN[angle];
        (re, im)
    }

    /// `Process`.
    pub fn process(&mut self, parameters: &Parameters, fft_out: &mut [f32], ifft_in: &mut [f32]) {
        let half = (self.fft_size >> 1) as usize;
        fft_out[0] = 0.0;
        fft_out[half] = 0.0;

        let freeze = parameters.freeze;
        let glitch = parameters.gate;
        let pitch_ratio = semitones_to_ratio(parameters.pitch);

        if !freeze {
            self.rectangular_to_polar(fft_out);
            self.store_magnitudes(
                fft_out,
                parameters.position,
                parameters.spectral.refresh_rate,
            );
        }

        self.replay_magnitudes(ifft_in, parameters.position);
        // `temp` aliases `fft_out` in the C -- used as scratch from here on.
        self.warp_magnitudes(ifft_in, fft_out, parameters.spectral.warp);
        self.shift_magnitudes(fft_out, ifft_in, pitch_ratio);
        if glitch {
            self.add_glitch(ifft_in);
        }
        self.quantize_magnitudes(ifft_in, parameters.spectral.quantization);
        self.set_phases(ifft_in, parameters.spectral.phase_randomization, pitch_ratio);
        self.polar_to_rectangular(ifft_in);

        if !glitch {
            self.glitch_algorithm = (Random::get_sample() & 3) as i8;
        }

        ifft_in[0] = 0.0;
        ifft_in[half] = 0.0;
    }

    /// `RectangularToPolar`.
    fn rectangular_to_polar(&mut self, fft_data: &mut [f32]) {
        let half = (self.fft_size >> 1) as usize;
        let size = self.size as usize;
        for i in 1..size {
            let mut magnitude = 0.0f32;
            let angle = fast_atan2r(fft_data[half + i], fft_data[i], &mut magnitude);
            fft_data[i] = magnitude;
            self.phases[size + i] = angle.wrapping_sub(self.phases[i]);
            self.phases[i] = angle;
        }
    }

    /// `SetPhases`.
    fn set_phases(&mut self, destination: &mut [f32], phase_randomization: f32, pitch_ratio: f32) {
        let half = (self.fft_size >> 1) as usize;
        let size = self.size as usize;
        for i in 0..size {
            // synthesis_phase[i] = phases_[i];
            destination[half + i] = f32::from_bits(self.phases[i] as u32);
            let delta = self.phases[size + i];
            self.phases[i] = self.phases[i].wrapping_add((delta as f32 * pitch_ratio) as i32 as u16);
        }
        let mut r = phase_randomization;
        r = (r - 0.05) * 1.06;
        r = constrain(r, 0.0, 1.0);
        r *= r;
        let amount = (r * 32768.0) as i32;
        for i in 0..size {
            let sp = destination[half + i].to_bits();
            let d = ((Random::get_sample() as i32).wrapping_mul(amount) >> 14) as u32;
            destination[half + i] = f32::from_bits(sp.wrapping_add(d));
        }
    }

    /// `PolarToRectangular`.
    fn polar_to_rectangular(&mut self, fft_data: &mut [f32]) {
        let half = (self.fft_size >> 1) as usize;
        let size = self.size as usize;
        for i in 1..size {
            let magnitude = fft_data[i];
            let angle = fft_data[half + i].to_bits() as u16;
            let (re, im) = Self::fast_p2r(magnitude, angle);
            fft_data[i] = re;
            fft_data[half + i] = im;
        }
        for i in size..half {
            fft_data[i] = 0.0;
            fft_data[half + i] = 0.0;
        }
    }

    /// `AddGlitch`.
    fn add_glitch(&mut self, x: &mut [f32]) {
        let size = self.size as usize;
        match self.glitch_algorithm {
            0 => {
                // Spectral hold and blow.
                let mut held = 0.0f32;
                for xi in x.iter_mut().take(size) {
                    if Random::get_sample() & 15 == 0 {
                        held = *xi;
                    }
                    *xi = held;
                    held *= 1.01;
                }
            }
            1 => {
                // Spectral shift up with aliasing.
                let factor = 1.0 + (Random::get_sample() & 7) as f32 / 4.0;
                let mut source = 0.0f32;
                for i in 0..size {
                    source += factor;
                    if source >= size as f32 {
                        source = 0.0;
                    }
                    x[i] = x[source as usize];
                }
            }
            2 => {
                // Kill largest harmonic, boost second largest.
                let m0 = max_index(&x[..size]);
                x[m0] = 0.0;
                let m1 = max_index(&x[..size]);
                x[m1] *= 8.0;
            }
            3 => {
                // Nasty high-pass.
                for i in 0..size {
                    if Random::get_sample() as u32 & 15 == 0 {
                        x[i] *= i as f32 / 16.0;
                    }
                }
            }
            _ => {}
        }
    }

    /// `QuantizeMagnitudes`.
    fn quantize_magnitudes(&mut self, xf_polar: &mut [f32], amount: f32) {
        let size = self.size as usize;
        if amount <= 0.48 {
            let amount = amount * 2.0;
            let scale_down = 0.5 * semitones_to_ratio(-108.0 * (1.0 - amount * amount))
                / self.fft_size as f32;
            let scale_up = 1.0 / scale_down;
            for xi in xf_polar.iter_mut().take(size) {
                *xi = scale_up * ((scale_down * *xi) as i32 as f32);
            }
        } else if amount >= 0.52 {
            let amount = (amount - 0.52) * 2.0;
            let norm = max_value(&xf_polar[..size]);
            let inv_norm = 1.0 / (norm + 0.0001);
            for xi in xf_polar.iter_mut().take(size).skip(1) {
                let x = *xi * inv_norm;
                let warped = 4.0 * x * (1.0 - x) * (1.0 - x) * (1.0 - x);
                *xi = (x + (warped - x) * amount) * norm;
            }
        }
    }

    /// `WarpMagnitudes`.
    fn warp_magnitudes(&mut self, source: &[f32], xf_polar: &mut [f32], amount: f32) {
        let size = self.size as usize;
        let bin_width = 1.0 / size as f32;
        let mut f = 0.0f32;

        let amount = amount * 4.0;
        let amount_integral = amount as i32;
        let amount_fractional = amount - amount_integral as f32;
        let ai = amount_integral.clamp(0, 4) as usize;
        let mut coefficients = [0.0f32; 4];
        for i in 0..4 {
            coefficients[i] = crossfade(
                WARP_POLYNOMIALS[ai][i],
                WARP_POLYNOMIALS[ai + 1][i],
                amount_fractional,
            );
        }
        let (a, b, c, d) = (
            coefficients[0],
            coefficients[1],
            coefficients[2],
            coefficients[3],
        );

        for i in 1..size {
            f += bin_width;
            let wf = (d + f * (c + f * (b + a * f))) * size as f32;
            xf_polar[i] = interpolate_raw(source, wf, 1.0);
        }
    }

    /// `ShiftMagnitudes`.
    fn shift_magnitudes(&mut self, source: &[f32], xf_polar: &mut [f32], pitch_ratio: f32) {
        let size = self.size as usize;
        // destination = &xf_polar[0], temp = &xf_polar[size_]
        if pitch_ratio == 1.0 {
            let (lo, hi) = xf_polar.split_at_mut(size);
            hi[..size].copy_from_slice(&source[..size]);
            lo.copy_from_slice(&hi[..size]);
        } else if pitch_ratio > 1.0 {
            let mut index = 1.0f32;
            let increment = 1.0 / pitch_ratio;
            for i in 1..size {
                xf_polar[size + i] = interpolate_raw(source, index, 1.0);
                index += increment;
            }
            let (lo, hi) = xf_polar.split_at_mut(size);
            lo.copy_from_slice(&hi[..size]);
        } else {
            for xi in xf_polar[size..2 * size].iter_mut() {
                *xi = 0.0;
            }
            let mut index = 1.0f32;
            let increment = pitch_ratio;
            for i in 1..size {
                let index_integral = index as i32 as usize;
                let index_fractional = index - (index as i32) as f32;
                xf_polar[size + index_integral] += (1.0 - index_fractional) * source[i];
                xf_polar[size + index_integral + 1] += index_fractional * source[i];
                index += increment;
            }
            let (lo, hi) = xf_polar.split_at_mut(size);
            lo.copy_from_slice(&hi[..size]);
        }
    }

    /// `StoreMagnitudes`.
    fn store_magnitudes(&mut self, xf_polar: &[f32], position: f32, feedback: f32) {
        let size = self.size as usize;
        let index_float = position * (self.num_textures - 1) as f32;
        let index_int = index_float as i32;
        let index_fractional = index_float - index_int as f32;
        let mut gain_a = 1.0 - index_fractional;
        let mut gain_b = index_fractional;

        let a_idx = index_int as usize;
        let b_idx = (index_int + if position == 1.0 { 0 } else { 1 }) as usize;
        let s = size;

        if feedback >= 0.5 {
            let feedback = 2.0 * (feedback - 0.5);
            if feedback < 0.5 {
                gain_a *= 1.0 - feedback;
                gain_b *= 1.0 - feedback;
                for i in 0..size {
                    let x = xf_polar[i];
                    self.textures[a_idx * s + i] = crossfade(self.textures[a_idx * s + i], x, gain_a);
                    self.textures[b_idx * s + i] = crossfade(self.textures[b_idx * s + i], x, gain_b);
                }
            } else {
                let t = (feedback - 0.5) * 0.7 + 0.5;
                let mut gain_new = t - 0.5;
                gain_new = gain_new * gain_new * 2.0 + 0.5;
                let gain_new_a = gain_a * gain_new;
                let gain_new_b = gain_b * gain_new;
                let gain_old_a = 1.0 - gain_a * (1.0 - t);
                let gain_old_b = 1.0 - gain_b * (1.0 - t);
                for i in 0..size {
                    let x = xf_polar[i];
                    self.textures[a_idx * s + i] = self.textures[a_idx * s + i] * gain_old_a + x * gain_new_a;
                    self.textures[b_idx * s + i] = self.textures[b_idx * s + i] * gain_old_b + x * gain_new_b;
                }
            }
        } else {
            let mut feedback = feedback * 2.0;
            feedback *= feedback;
            let threshold = (feedback * 65535.0) as u16;
            for i in 0..size {
                let x = xf_polar[i];
                let gain = if (Random::get_sample() as u16) <= threshold {
                    1.0
                } else {
                    0.0
                };
                self.textures[a_idx * s + i] = crossfade(self.textures[a_idx * s + i], x, gain_a * gain);
                self.textures[b_idx * s + i] = crossfade(self.textures[b_idx * s + i], x, gain_b * gain);
            }
        }
    }

    /// `ReplayMagnitudes`.
    fn replay_magnitudes(&self, xf_polar: &mut [f32], position: f32) {
        let size = self.size as usize;
        let index_float = position * (self.num_textures - 1) as f32;
        let index_int = index_float as i32;
        let index_fractional = index_float - index_int as f32;
        let a = self.texture(index_int);
        let b = self.texture(index_int + if position == 1.0 { 0 } else { 1 });
        for i in 0..size {
            xf_polar[i] = crossfade(a[i], b[i], index_fractional);
        }
    }
}

impl Default for FrameTransformation {
    fn default() -> Self {
        Self::new()
    }
}

/// `*std::max_element(&x[0], &x[n])` -- index of the first maximum.
fn max_index(x: &[f32]) -> usize {
    let mut best = 0usize;
    for i in 1..x.len() {
        if x[i] > x[best] {
            best = i;
        }
    }
    best
}

fn max_value(x: &[f32]) -> f32 {
    x.iter().copied().fold(f32::NEG_INFINITY, f32::max)
}
