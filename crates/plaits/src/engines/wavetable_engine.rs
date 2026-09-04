//! `plaits/dsp/engine/wavetable_engine.h` -- an 8x8x3 wave terrain built from
//! 192 built-in single-cycle waves (+ up to 15 user-uploaded ones).
//!
//! `LoadUserData` takes `Option<&'static [u8]>` rather than owning a copy: the
//! only realistic source for real (non-`None`) user data is a flash-resident
//! asset, which is `'static` for the process lifetime; this port doesn't wire
//! up flash storage, so every caller in this workspace passes `None` (the
//! fallback wave map the C also uses when its `user_data` pointer is null).

use stmlib::fdsp::one_pole;
use stmlib::parameter_interpolator::ParameterInterpolator;

use crate::engine::{note_to_frequency, Engine, EngineParameters, PostProcessingSettings};
use crate::oscillator::{interpolate_wave_hermite, Differentiator};
use crate::resources::WAV_INTEGRATED_WAVES;

const NUM_BANKS: usize = 4;
const NUM_WAVES_PER_BANK: usize = 64;
const NUM_WAVES: usize = 192;
const NUM_CUSTOM_WAVES: usize = 15;
const TABLE_SIZE: usize = 128;
const TABLE_SIZE_F: f32 = TABLE_SIZE as f32;
/// Samples per wave row in `wav_integrated_waves` / a custom wave (128 + 4 guard
/// samples for Hermite interpolation overrun).
const WAVE_STRIDE: usize = TABLE_SIZE + 4;

#[derive(Debug, Clone, Copy)]
enum WaveSource {
    Builtin(usize),
    Custom(usize),
}

pub struct WavetableEngine {
    phase: f32,
    x_pre_lp: f32,
    y_pre_lp: f32,
    z_pre_lp: f32,
    x_lp: f32,
    y_lp: f32,
    z_lp: f32,
    previous_x: f32,
    previous_y: f32,
    previous_z: f32,
    previous_f0: f32,
    wave_map: [WaveSource; NUM_BANKS * NUM_WAVES_PER_BANK],
    custom_waves: Option<&'static [u8]>,
    diff_out: Differentiator,
}

impl Default for WavetableEngine {
    fn default() -> Self {
        Self {
            phase: 0.0,
            x_pre_lp: 0.0,
            y_pre_lp: 0.0,
            z_pre_lp: 0.0,
            x_lp: 0.0,
            y_lp: 0.0,
            z_lp: 0.0,
            previous_x: 0.0,
            previous_y: 0.0,
            previous_z: 0.0,
            previous_f0: crate::dsp::A0,
            wave_map: [WaveSource::Builtin(0); NUM_BANKS * NUM_WAVES_PER_BANK],
            custom_waves: None,
            diff_out: Differentiator::default(),
        }
    }
}

#[inline]
fn clamp_toward_center(x: f32, amount: f32) -> f32 {
    let mut x = x - 0.5;
    x *= amount;
    x = x.clamp(-0.5, 0.5);
    x + 0.5
}

/// Free function (not a `&self` method): called while `self.previous_{x,y,z,f0}`
/// are already borrowed by live `ParameterInterpolator`s, so it takes exactly
/// the two fields it needs, letting Rust see the borrows as disjoint.
fn read_wave(
    wave_map: &[WaveSource],
    custom_waves: Option<&'static [u8]>,
    x: usize,
    y: usize,
    z: usize,
    phase_integral: usize,
    phase_fractional: f32,
) -> f32 {
    let index = x + y * 8 + z * NUM_WAVES_PER_BANK;
    match wave_map[index] {
        WaveSource::Builtin(w) => interpolate_wave_hermite(
            &WAV_INTEGRATED_WAVES[w * WAVE_STRIDE..],
            phase_integral,
            phase_fractional,
        ),
        WaveSource::Custom(w) => {
            let data = custom_waves.unwrap_or(&[]);
            let byte_offset = 64 + w * WAVE_STRIDE * 2;
            // Reinterpret consecutive LE i16 samples starting at byte_offset.
            let sample_at = |k: usize| -> i16 {
                let o = byte_offset + k * 2;
                i16::from_le_bytes([data[o], data[o + 1]])
            };
            let a = sample_at(phase_integral) as f32;
            let b = sample_at(phase_integral + 1) as f32;
            let c = sample_at(phase_integral + 2) as f32;
            let d = sample_at(phase_integral + 3) as f32;
            hermite4(a, b, c, d, phase_fractional)
        }
    }
}

#[inline]
fn hermite4(xm1: f32, x0: f32, x1: f32, x2: f32, f: f32) -> f32 {
    let c = (x1 - xm1) * 0.5;
    let v = x0 - x1;
    let w = c + v;
    let a = w + v + (x2 - x0) * 0.5;
    let b_neg = w + a;
    (((a * f) - b_neg) * f + c) * f + x0
}

impl Engine for WavetableEngine {
    fn init(&mut self) {
        *self = Self::default();
        self.diff_out.init();
        self.load_user_data(None);
    }

    fn reset(&mut self) {}

    fn load_user_data(&mut self, user_data: Option<&'static [u8]>) {
        self.custom_waves = user_data;
        for bank in 0..NUM_BANKS {
            for wave in 0..NUM_WAVES_PER_BANK {
                let i = bank * NUM_WAVES_PER_BANK + wave;
                let mut w = i;
                if bank == NUM_BANKS - 1 {
                    w = user_data.map(|d| d[wave] as usize).unwrap_or((w * 101) % NUM_WAVES);
                }
                self.wave_map[i] = if w >= NUM_WAVES {
                    WaveSource::Custom((w - NUM_WAVES).min(NUM_CUSTOM_WAVES - 1))
                } else {
                    WaveSource::Builtin(w)
                };
            }
        }
    }

    fn render(
        &mut self,
        parameters: &EngineParameters,
        out: &mut [f32],
        aux: &mut [f32],
        already_enveloped: bool,
    ) -> bool {
        let size = out.len();
        let f0 = note_to_frequency(parameters.note);

        one_pole(&mut self.x_pre_lp, parameters.timbre * 6.9999, 0.2);
        one_pole(&mut self.y_pre_lp, parameters.morph * 6.9999, 0.2);
        one_pole(&mut self.z_pre_lp, parameters.harmonics * 6.9999, 0.05);

        let x = self.x_pre_lp;
        let y = self.y_pre_lp;
        let z = self.z_pre_lp;

        let quantization = (z - 3.0).max(0.0).min(1.0);
        let lp_coefficient = (2.0 * f0 * (4.0 - 3.0 * quantization)).max(0.01).min(0.1);

        let x_integral = x as i32;
        let mut x_fractional = x - x_integral as f32;
        let y_integral = y as i32;
        let mut y_fractional = y - y_integral as f32;
        let z_integral = z as i32;
        let mut z_fractional = z - z_integral as f32;

        x_fractional += quantization * (clamp_toward_center(x_fractional, 16.0) - x_fractional);
        y_fractional += quantization * (clamp_toward_center(y_fractional, 16.0) - y_fractional);
        z_fractional += quantization * (clamp_toward_center(z_fractional, 16.0) - z_fractional);

        let mut x_modulation =
            ParameterInterpolator::new(&mut self.previous_x, x_integral as f32 + x_fractional, size);
        let mut y_modulation =
            ParameterInterpolator::new(&mut self.previous_y, y_integral as f32 + y_fractional, size);
        let mut z_modulation =
            ParameterInterpolator::new(&mut self.previous_z, z_integral as f32 + z_fractional, size);
        let mut f0_modulation = ParameterInterpolator::new(&mut self.previous_f0, f0, size);

        for i in 0..size {
            let f0 = f0_modulation.next();

            let gain = (1.0 / (f0 * 131072.0)) * (0.95 - f0);
            let cutoff = (TABLE_SIZE_F * f0).min(1.0);

            one_pole(&mut self.x_lp, x_modulation.next(), lp_coefficient);
            one_pole(&mut self.y_lp, y_modulation.next(), lp_coefficient);
            one_pole(&mut self.z_lp, z_modulation.next(), lp_coefficient);

            let x = self.x_lp;
            let y = self.y_lp;
            let z = self.z_lp;

            let x_integral = x as i32;
            let x_fractional = x - x_integral as f32;
            let y_integral = y as i32;
            let y_fractional = y - y_integral as f32;
            let z_integral = z as i32;
            let z_fractional = z - z_integral as f32;

            self.phase += f0;
            if self.phase >= 1.0 {
                self.phase -= 1.0;
            }

            let p = self.phase * TABLE_SIZE_F;
            let p_integral = p as usize;
            let p_fractional = p - p_integral as f32;

            let x0 = x_integral.max(0) as usize;
            let x1 = x0 + 1;
            let y0 = y_integral.max(0) as usize;
            let y1 = y0 + 1;
            let mut z0 = z_integral.max(0) as usize;
            let mut z1 = z0 + 1;

            if z0 >= 4 {
                z0 = 7 - z0;
            }
            if z1 >= 4 {
                z1 = 7 - z1;
            }

            let x0y0z0 = read_wave(&self.wave_map, self.custom_waves, x0, y0, z0, p_integral, p_fractional);
            let x1y0z0 = read_wave(&self.wave_map, self.custom_waves, x1, y0, z0, p_integral, p_fractional);
            let xy0z0 = x0y0z0 + (x1y0z0 - x0y0z0) * x_fractional;

            let x0y1z0 = read_wave(&self.wave_map, self.custom_waves, x0, y1, z0, p_integral, p_fractional);
            let x1y1z0 = read_wave(&self.wave_map, self.custom_waves, x1, y1, z0, p_integral, p_fractional);
            let xy1z0 = x0y1z0 + (x1y1z0 - x0y1z0) * x_fractional;

            let xyz0 = xy0z0 + (xy1z0 - xy0z0) * y_fractional;

            let x0y0z1 = read_wave(&self.wave_map, self.custom_waves, x0, y0, z1, p_integral, p_fractional);
            let x1y0z1 = read_wave(&self.wave_map, self.custom_waves, x1, y0, z1, p_integral, p_fractional);
            let xy0z1 = x0y0z1 + (x1y0z1 - x0y0z1) * x_fractional;

            let x0y1z1 = read_wave(&self.wave_map, self.custom_waves, x0, y1, z1, p_integral, p_fractional);
            let x1y1z1 = read_wave(&self.wave_map, self.custom_waves, x1, y1, z1, p_integral, p_fractional);
            let xy1z1 = x0y1z1 + (x1y1z1 - x0y1z1) * x_fractional;

            let xyz1 = xy0z1 + (xy1z1 - xy0z1) * y_fractional;

            let mut mix = xyz0 + (xyz1 - xyz0) * z_fractional;
            mix = self.diff_out.process(cutoff, mix) * gain;
            out[i] = mix;
            aux[i] = (mix * 32.0) as i32 as f32 / 32.0;
        }
        already_enveloped
    }

    fn post_processing_settings(&self) -> PostProcessingSettings {
        PostProcessingSettings {
            out_gain: 0.6,
            aux_gain: 0.6,
            already_enveloped: false,
        }
    }
}
