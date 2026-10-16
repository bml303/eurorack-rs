//! `plaits/dsp/engine/wavetable_engine.h` -- four 8x8 banks of waves from
//! 192 built-in single-cycle waves; HARMONICS goes through them A B C D D C
//! B A. The fourth bank (Z = 3) is the built-in waves shuffled, or a user
//! wavetable ([`UserWavetable`]): a map of its 64 waves among the built-in
//! ones and up to 15 custom waves, the C's `LoadUserData` block (what the
//! Plaits wavetable editor makes).
//!
//! # Deviations from the C
//!
//! - The user wavetable is a decoded [`UserWavetable`] given with
//!   [`WavetableEngine::set_user_wavetable`] (`Voice::set_user_wavetable`)
//!   instead of the raw bytes through `LoadUserData`, whose waves the C
//!   reads in place as `int16_t`.
//! - A map entry past the custom waves plays the last one; the C's
//!   `min(w - 192, kNumCustomWaves)` reads one wave past them.

use stmlib::fdsp::one_pole;

use crate::PostProcessingSettings;
use crate::dsp::SAMPLE_RATE;
use crate::engine::{Engine, EngineParameters, note_to_frequency};
use crate::oscillator::{Differentiator, interpolate_wave_hermite};
use crate::resources::WAV_INTEGRATED_WAVES;

use crate::utils::parameter_interpolator::SimpleParameterInterpolator;

const TABLE_SIZE: usize = 128;
const TABLE_SIZE_F: f32 = TABLE_SIZE as f32;
const NUM_WAVES: usize = 192;
const WAVES_PER_BANK: usize = 64;

/// Custom waves a user wavetable holds
pub const NUM_CUSTOM_WAVES: usize = 15;
/// A wave's samples: the integrated wave's 128, then its first 4 again
/// (for the interpolation)
pub const WAVE_SIZE: usize = TABLE_SIZE + 4;

/// A user wavetable: the fourth bank's 64 waves (`map[x + 8 * y]`: 0..192
/// a built-in wave, 192.. custom wave `n - 192`) and the custom waves,
/// integrated as the built-in ones are
#[derive(Debug, Clone, PartialEq)]
pub struct UserWavetable {
    pub map: [u8; WAVES_PER_BANK],
    pub waves: [i16; NUM_CUSTOM_WAVES * WAVE_SIZE],
}

impl UserWavetable {
    /// The C's user data block: the map, the waves (little-endian), and
    /// padding to 4 KB
    pub const BLOCK_SIZE: usize = 4096;
    /// The map and the waves, without the padding
    pub const DATA_SIZE: usize = WAVES_PER_BANK + NUM_CUSTOM_WAVES * WAVE_SIZE * 2;

    /// From the C's user data block (at least `DATA_SIZE` bytes; the rest
    /// is ignored)
    pub fn from_bytes(bytes: &[u8]) -> Option<UserWavetable> {
        if bytes.len() < Self::DATA_SIZE {
            return None;
        }
        let mut table = UserWavetable { map: [0; WAVES_PER_BANK], waves: [0; NUM_CUSTOM_WAVES * WAVE_SIZE] };
        table.map.copy_from_slice(&bytes[..WAVES_PER_BANK]);
        for (sample, b) in table.waves.iter_mut().zip(bytes[WAVES_PER_BANK..].chunks_exact(2)) {
            *sample = i16::from_le_bytes([b[0], b[1]]);
        }
        Some(table)
    }

    /// The samples map entry `wave` (0..64) plays: a built-in wave of
    /// `built_in` or a custom one
    fn wave<'a>(&'a self, wave: usize, built_in: &'a [i16]) -> &'a [i16] {
        match self.map[wave] as usize {
            w if w < NUM_WAVES => &built_in[w * WAVE_SIZE..],
            w => &self.waves[(w - NUM_WAVES).min(NUM_CUSTOM_WAVES - 1) * WAVE_SIZE..],
        }
    }
}

#[derive(Debug, Clone)]
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

    diff_out: Differentiator,

    wavetables: &'static [i16; 25344],
    user: Option<&'static UserWavetable>,
}

impl Default for WavetableEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl WavetableEngine {
    pub fn new() -> Self {
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
            previous_f0: 0.0,

            diff_out: Differentiator::new(),

            wavetables: &WAV_INTEGRATED_WAVES,
            user: None,
        }
    }

    pub fn set_wavetables(&mut self, wavetables: &'static [i16; 25344]) {
        self.wavetables = wavetables;
    }

    /// The fourth bank from `user` instead of the shuffled built-in waves,
    /// or those again with `None`
    pub fn set_user_wavetable(&mut self, user: Option<&'static UserWavetable>) {
        self.user = user;
    }

    #[inline]
    fn read_wave(
        &self,
        x: usize,
        y: usize,
        z: usize,
        randomize: usize,
        phase_integral: usize,
        phase_fractional: f32,
    ) -> f32 {
        if let (3, Some(user)) = (z, self.user) {
            let samples = user.wave(x + y * 8, &self.wavetables[..]);
            return interpolate_wave_hermite(samples, phase_integral, phase_fractional);
        }
        let wave = ((x + y * 8 + z * 64) * randomize) % 192;
        interpolate_wave_hermite(
            &self.wavetables[wave * (TABLE_SIZE + 4)..],
            phase_integral,
            phase_fractional,
        )
    }
}

impl Engine for WavetableEngine {
    fn init(&mut self) {
        self.phase = 0.0;

        self.x_lp = 0.0;
        self.y_lp = 0.0;
        self.z_lp = 0.0;

        self.x_pre_lp = 0.0;
        self.y_pre_lp = 0.0;
        self.z_pre_lp = 0.0;

        self.previous_x = 0.0;
        self.previous_y = 0.0;
        self.previous_z = 0.0;
        self.previous_f0 = 55.0 / SAMPLE_RATE; // A0 normalized

        self.diff_out.init();
    }

    fn reset(&mut self) {
        self.init();
    }

    fn load_user_data(&mut self, _user_data: Option<&'static [u8]>) {}

    fn post_processing_settings(&self) -> PostProcessingSettings {
        PostProcessingSettings {
            out_gain: 0.6,
            aux_gain: 0.6,
            already_enveloped: false,
        }
    }

    #[inline]
    fn render(
        &mut self,
        parameters: &EngineParameters,
        out: &mut [f32],
        aux: &mut [f32],
        already_enveloped: bool,
    ) -> bool {
        let f0 = note_to_frequency(parameters.note);

        one_pole(&mut self.x_pre_lp, parameters.timbre * 6.9999, 0.2);
        one_pole(&mut self.y_pre_lp, parameters.morph * 6.9999, 0.2);
        one_pole(&mut self.z_pre_lp, parameters.harmonics * 6.9999, 0.05);

        let x = self.x_pre_lp;
        let y = self.y_pre_lp;
        let z = self.z_pre_lp;

        let quantization = (z - 3.0).clamp(0.0, 1.0);
        let lp_coefficient = (2.0 * f0 * (4.0 - 3.0 * quantization)).clamp(0.01, 0.1);

        let x_integral = x as usize;
        let mut x_fractional = x - (x_integral as f32);
        let y_integral = y as usize;
        let mut y_fractional = y - (y_integral as f32);
        let z_integral = z as usize;
        let mut z_fractional = z - (z_integral as f32);

        x_fractional += quantization * (clamp(x_fractional, 16.0) - x_fractional);
        y_fractional += quantization * (clamp(y_fractional, 16.0) - y_fractional);
        z_fractional += quantization * (clamp(z_fractional, 16.0) - z_fractional);

        let x_modulation = SimpleParameterInterpolator::new(
            self.previous_x,
            x_integral as f32 + x_fractional,
            out.len(),
        );
        let y_modulation = SimpleParameterInterpolator::new(
            self.previous_y,
            y_integral as f32 + y_fractional,
            out.len(),
        );
        let z_modulation = SimpleParameterInterpolator::new(
            self.previous_z,
            z_integral as f32 + z_fractional,
            out.len(),
        );

        let f0_modulation = SimpleParameterInterpolator::new(self.previous_f0, f0, out.len());

        for (out_sample, aux_sample) in out.iter_mut().zip(aux.iter_mut()) {
            let f0 = f0_modulation.update(&mut self.previous_f0);

            let gain = (1.0 / (f0 * 131072.0)) * (0.95 - f0);
            let cutoff = f32::min(TABLE_SIZE_F * f0, 1.0);

            one_pole(
                &mut self.x_lp,
                x_modulation.update(&mut self.previous_x),
                lp_coefficient,
            );
            one_pole(
                &mut self.y_lp,
                y_modulation.update(&mut self.previous_y),
                lp_coefficient,
            );
            one_pole(
                &mut self.z_lp,
                z_modulation.update(&mut self.previous_z),
                lp_coefficient,
            );

            let x = self.x_lp;
            let y = self.y_lp;
            let z = self.z_lp;

            let x_integral = x as usize;
            let x_fractional = x - (x_integral as f32);
            let y_integral = y as usize;
            let y_fractional = y - (y_integral as f32);
            let z_integral = z as usize;
            let z_fractional = z - (z_integral as f32);

            self.phase += f0;
            if self.phase >= 1.0 {
                self.phase -= 1.0;
            }

            let p = self.phase * TABLE_SIZE_F;
            let p_integral = p as usize;
            let p_fractional = p - (p_integral as f32);

            {
                let x0 = x_integral;
                let x1 = x_integral + 1;
                let y0 = y_integral;
                let y1 = y_integral + 1;
                let mut z0 = z_integral;
                let mut z1 = z_integral + 1;

                if z0 >= 4 {
                    z0 = 7 - z0;
                }
                if z1 >= 4 {
                    z1 = 7 - z1;
                }

                let r0 = if z0 == 3 { 101 } else { 1 };
                let r1 = if z1 == 3 { 101 } else { 1 };

                let x0y0z0 = self.read_wave(x0, y0, z0, r0, p_integral, p_fractional);
                let x1y0z0 = self.read_wave(x1, y0, z0, r0, p_integral, p_fractional);
                let xy0z0 = x0y0z0 + (x1y0z0 - x0y0z0) * x_fractional;

                let x0y1z0 = self.read_wave(x0, y1, z0, r0, p_integral, p_fractional);
                let x1y1z0 = self.read_wave(x1, y1, z0, r0, p_integral, p_fractional);
                let xy1z0 = x0y1z0 + (x1y1z0 - x0y1z0) * x_fractional;

                let xyz0 = xy0z0 + (xy1z0 - xy0z0) * y_fractional;

                let x0y0z1 = self.read_wave(x0, y0, z1, r1, p_integral, p_fractional);
                let x1y0z1 = self.read_wave(x1, y0, z1, r1, p_integral, p_fractional);
                let xy0z1 = x0y0z1 + (x1y0z1 - x0y0z1) * x_fractional;

                let x0y1z1 = self.read_wave(x0, y1, z1, r1, p_integral, p_fractional);
                let x1y1z1 = self.read_wave(x1, y1, z1, r1, p_integral, p_fractional);
                let xy1z1 = x0y1z1 + (x1y1z1 - x0y1z1) * x_fractional;

                let xyz1 = xy0z1 + (xy1z1 - xy0z1) * y_fractional;

                let mut mix = xyz0 + (xyz1 - xyz0) * z_fractional;
                mix = self.diff_out.process(cutoff, mix) * gain;
                *out_sample = mix;
                *aux_sample = (((mix * 32.0) as i32) as f32) / 32.0;
            }
        }
        already_enveloped
    }
}

#[inline]
fn clamp(mut x: f32, amount: f32) -> f32 {
    x -= 0.5;
    x *= amount;
    x = x.clamp(-0.5, 0.5);
    x += 0.5;

    x
}
