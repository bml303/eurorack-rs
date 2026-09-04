//! `plaits/dsp/engine2/wave_terrain_engine.h` -- a 2D function ("terrain")
//! evaluated along an elliptical path, computed on the fly rather than
//! looked up from a stored table.
//!
//! `terrain_index == 8` (a user-supplied terrain loaded from flash) is kept
//! for fidelity but is unreachable in this port since nothing wires flash
//! storage into [`Engine::load_user_data`] yet -- `user_terrain` is always
//! `None`, so [`WaveTerrainEngine::terrain`] never selects it and
//! `num_terrains` stays 8.

use stmlib::parameter_interpolator::ParameterInterpolator;

use crate::dsp::MAX_BLOCK_SIZE;
use crate::engine::{note_to_frequency, Engine, EngineParameters, PostProcessingSettings};
use crate::oscillator::{sine, FastSineOscillator};
use crate::resources::WAV_INTEGRATED_WAVES;

const OVERSAMPLING: usize = 2;

fn interpolate_wave_i8(table: &[i8], index_integral: usize, index_fractional: f32) -> f32 {
    let a = table[index_integral] as f32;
    let b = table[index_integral + 1] as f32;
    a + (b - a) * index_fractional
}

fn interpolate_integrated_wave(table: &[i16], index_integral: usize, index_fractional: f32) -> f32 {
    let a = table[index_integral] as f32;
    let b = table[index_integral + 1] as f32;
    let c = table[index_integral + 2] as f32;
    let t = index_fractional;
    (b - a) + (c - b - b + a) * t
}

/// `terrain` is a 64x64 grid of signed 8-bit samples (`user_terrain`, from
/// flash -- unreachable in this port, see the module doc).
fn terrain_lookup(x: f32, y: f32, terrain: &[i8]) -> f32 {
    const TERRAIN_SIZE: usize = 64;
    const VALUE_SCALE: f32 = 1.0 / 128.0;
    const COORD_SCALE: f32 = (TERRAIN_SIZE - 2) as f32 * 0.5;

    let x = (x + 1.0) * COORD_SCALE;
    let y = (y + 1.0) * COORD_SCALE;

    let x_integral = x as usize;
    let x_fractional = x - x_integral as f32;
    let y_integral = y as usize;
    let y_fractional = y - y_integral as f32;

    let row0 = &terrain[y_integral * TERRAIN_SIZE..];
    let row1 = &terrain[(y_integral + 1) * TERRAIN_SIZE..];
    let xy0 = interpolate_wave_i8(row0, x_integral, x_fractional);
    let xy1 = interpolate_wave_i8(row1, x_integral, x_fractional);
    (xy0 + (xy1 - xy0) * y_fractional) * VALUE_SCALE
}

fn terrain_lookup_wt(x: f32, y: f32, bank: usize) -> f32 {
    const TABLE_SIZE: usize = 128;
    const TABLE_SIZE_FULL: usize = TABLE_SIZE + 4;
    const NUM_WAVES: usize = 64;

    let sample = (y + 1.0) * 0.5 * TABLE_SIZE as f32;
    let wt = (x + 1.0) * 0.5 * (NUM_WAVES - 1) as f32;

    let waves = &WAV_INTEGRATED_WAVES[bank * NUM_WAVES * TABLE_SIZE_FULL..];

    let sample_integral = sample as usize;
    let sample_fractional = sample - sample_integral as f32;
    let wt_integral = wt as usize;
    let wt_fractional = wt - wt_integral as f32;

    const VALUE_SCALE: f32 = 1.0 / 1024.0;
    let row0 = &waves[wt_integral * TABLE_SIZE_FULL..];
    let row1 = &waves[(wt_integral + 1) * TABLE_SIZE_FULL..];
    let xy0 = interpolate_integrated_wave(row0, sample_integral, sample_fractional);
    let xy1 = interpolate_integrated_wave(row1, sample_integral, sample_fractional);
    (xy0 + (xy1 - xy0) * wt_fractional) * VALUE_SCALE
}

fn squash(x: f32, a: f32) -> f32 {
    let x = x * a;
    x / (1.0 + x.abs())
}

pub struct WaveTerrainEngine {
    path: FastSineOscillator,
    offset: f32,
    terrain: f32,
    temp_buffer: [f32; MAX_BLOCK_SIZE * 4],
    user_terrain: Option<&'static [u8]>,
}

impl Default for WaveTerrainEngine {
    fn default() -> Self {
        Self {
            path: FastSineOscillator::default(),
            offset: 0.0,
            terrain: 0.0,
            temp_buffer: [0.0; MAX_BLOCK_SIZE * 4],
            user_terrain: None,
        }
    }
}

/// Free function (rather than a `&self` method) so it can be called while
/// `path_x`/`path_y` -- disjoint fields of `WaveTerrainEngine` -- are still
/// mutably borrowed from `temp_buffer` in `render`.
fn terrain_fn(user_terrain: Option<&'static [u8]>, x: f32, y: f32, terrain_index: i32) -> f32 {
    const K: f32 = 4.0;
    match terrain_index {
        0 => (squash(sine(K + x * 1.273), 2.0) - sine(K + y * (x + 1.571) * 0.637)) * 0.57,
        1 => {
            let xy = x * y;
            sine(K + sine(K + (x + y) * 0.637) / (0.2 + xy * xy) * 0.159)
        }
        2 => {
            let xy = x * y;
            sine(K + sine(K + 2.387 * xy) / (0.350 + xy * xy) * 0.159)
        }
        3 => {
            let xy = x * y;
            let xys = (x - 0.25) * (y + 0.25);
            sine(K + xy / (2.0 + (5.0 * xys).abs()) * 6.366)
        }
        4 => sine(0.159 / (0.170 + (y - 0.25).abs()) + 0.477 / (0.350 + ((x + 0.5) * (y + 1.5)).abs()) + K),
        5 | 6 | 7 => terrain_lookup_wt(x, y, 2 - (terrain_index - 5) as usize),
        8 => {
            // Unreachable: `user_terrain` is always `None` in this port.
            if let Some(data) = user_terrain {
                // Reinterpret bytes as signed 8-bit terrain samples (same
                // size/align as u8, just a different read interpretation).
                let signed: &[i8] =
                    unsafe { core::slice::from_raw_parts(data.as_ptr() as *const i8, data.len()) };
                terrain_lookup(x, y, signed)
            } else {
                0.0
            }
        }
        _ => 0.0,
    }
}

impl Engine for WaveTerrainEngine {
    fn init(&mut self) {
        self.path.init();
        self.offset = 0.0;
        self.terrain = 0.0;
        self.user_terrain = None;
    }

    fn reset(&mut self) {}

    fn load_user_data(&mut self, user_data: Option<&'static [u8]>) {
        self.user_terrain = user_data;
    }

    fn render(
        &mut self,
        parameters: &EngineParameters,
        out: &mut [f32],
        aux: &mut [f32],
        already_enveloped: bool,
    ) -> bool {
        let size = out.len();
        const SCALE: f32 = 1.0 / OVERSAMPLING as f32;

        let (path_x, path_y) = self.temp_buffer.split_at_mut(OVERSAMPLING * size);
        let path_x = &mut path_x[..OVERSAMPLING * size];
        let path_y = &mut path_y[..OVERSAMPLING * size];

        let f0 = note_to_frequency(parameters.note);
        let attenuation = (1.0 - 8.0 * f0).max(0.0);
        let radius = 0.1 + 0.9 * parameters.timbre * attenuation * (2.0 - attenuation);

        self.path.render_quadrature(f0 * SCALE, radius, path_x, path_y);

        let mut offset = ParameterInterpolator::new(&mut self.offset, 1.9 * parameters.morph - 1.0, size);
        let num_terrains = if self.user_terrain.is_some() { 9 } else { 8 };
        let mut terrain = ParameterInterpolator::new(
            &mut self.terrain,
            (parameters.harmonics * 1.05).min(1.0) * (num_terrains as f32 - 1.0001),
            size,
        );

        let mut ij = 0usize;
        for i in 0..size {
            let x_offset = offset.next();

            let z = terrain.next();
            let z_integral = z as i32;
            let z_fractional = z - z_integral as f32;

            let mut out_s = 0.0f32;
            let mut aux_s = 0.0f32;

            for _ in 0..OVERSAMPLING {
                let x = path_x[ij] * (1.0 - x_offset.abs()) + x_offset;
                let y = path_y[ij];
                ij += 1;

                let z0 = terrain_fn(self.user_terrain, x, y, z_integral);
                let z1 = terrain_fn(self.user_terrain, x, y, z_integral + 1);
                let z = z0 + (z1 - z0) * z_fractional;
                out_s += z;
                aux_s += y + z;
            }
            out[i] = SCALE * out_s;
            aux[i] = sine(1.0 + 0.5 * SCALE * aux_s);
        }

        already_enveloped
    }

    fn post_processing_settings(&self) -> PostProcessingSettings {
        PostProcessingSettings {
            out_gain: 0.7,
            aux_gain: 0.7,
            already_enveloped: false,
        }
    }
}
