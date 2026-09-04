//! `plaits/dsp/engine/waveshaping_engine.h` -- a band-limited slope oscillator
//! run through a cross-faded pair of waveshaper LUTs, then a wavefolder.

use stmlib::parameter_interpolator::ParameterInterpolator;

use crate::engine::{note_to_frequency, Engine, EngineParameters, PostProcessingSettings};
use crate::oscillator::{Oscillator, OscillatorShape};
use crate::oscillator::sine;
use crate::resources::{LOOKUP_TABLE_I16_TABLE, LUT_FOLD, LUT_FOLD_2};

/// `InterpolateHermite(table + 1, index, size)` -- the C shifts the table
/// pointer by 1 so that `table[index_integral - 1]` stays in bounds down to
/// `index_integral == 0`; a Rust slice can't be indexed at -1, so this bakes
/// the shift into the offsets instead (`table[i]` here == the C's
/// `(table_ptr + 1)[i - 1]` == `table_ptr[i]`, so it's called with the
/// *unshifted* `LUT_FOLD`/`LUT_FOLD_2`).
fn interpolate_hermite_fold(table: &[f32], index: f32, size: f32) -> f32 {
    let index = index * size;
    let integral = index as i32;
    let fractional = index - integral as f32;
    let i = integral as usize;
    let xm1 = table[i];
    let x0 = table[i + 1];
    let x1 = table[i + 2];
    let x2 = table[i + 3];
    let c = (x1 - xm1) * 0.5;
    let v = x0 - x1;
    let w = c + v;
    let a = w + v + (x2 - x0) * 0.5;
    let b_neg = w + a;
    let f = fractional;
    (((a * f) - b_neg) * f + c) * f + x0
}

fn tame(f0: f32, harmonics: f32, order: f32) -> f32 {
    let f0 = f0 * harmonics;
    let max_f = 0.5 / order;
    let max_amount = (1.0 - (f0 - max_f) / (0.5 - max_f)).clamp(0.0, 1.0);
    max_amount * max_amount * max_amount
}

#[derive(Default)]
pub struct WaveshapingEngine {
    slope: Oscillator,
    triangle: Oscillator,
    previous_shape: f32,
    previous_wavefolder_gain: f32,
    previous_overtone_gain: f32,
}

impl Engine for WaveshapingEngine {
    fn init(&mut self) {
        self.slope.init();
        self.triangle.init();
        self.previous_shape = 0.0;
        self.previous_wavefolder_gain = 0.0;
        self.previous_overtone_gain = 0.0;
    }

    fn reset(&mut self) {}

    fn load_user_data(&mut self, _user_data: Option<&'static [u8]>) {}

    fn render(
        &mut self,
        parameters: &EngineParameters,
        out: &mut [f32],
        aux: &mut [f32],
        already_enveloped: bool,
    ) -> bool {
        let size = out.len();
        let root = parameters.note;
        let f0 = note_to_frequency(root);
        let pw = parameters.morph * 0.45 + 0.5;

        // Start from a band-limited slope signal.
        self.slope.render(OscillatorShape::Slope, f0, pw, out);
        self.triangle.render(OscillatorShape::Slope, f0, 0.5, aux);

        // Estimate how rich the spectrum is, and reduce the waveshaping
        // control's range accordingly.
        let slope = 3.0 + (parameters.morph - 0.5).abs() * 5.0;
        let shape_amount = (parameters.harmonics - 0.5).abs() * 2.0;
        let shape_amount_attenuation = tame(f0, slope, 16.0);
        let wavefolder_gain = parameters.timbre;
        let wavefolder_gain_attenuation = tame(
            f0,
            slope * (3.0 + shape_amount * shape_amount_attenuation * 5.0),
            12.0,
        );

        let mut shape_modulation = ParameterInterpolator::new(
            &mut self.previous_shape,
            0.5 + (parameters.harmonics - 0.5) * shape_amount_attenuation,
            size,
        );
        let mut wf_gain_modulation = ParameterInterpolator::new(
            &mut self.previous_wavefolder_gain,
            0.03 + 0.46 * wavefolder_gain * wavefolder_gain_attenuation,
            size,
        );
        let overtone_gain = parameters.timbre * (2.0 - parameters.timbre);
        let mut overtone_gain_modulation = ParameterInterpolator::new(
            &mut self.previous_overtone_gain,
            overtone_gain * (2.0 - overtone_gain),
            size,
        );

        for i in 0..size {
            let shape = shape_modulation.next() * 3.9999;
            let shape_integral = shape as usize;
            let shape_fractional = shape - shape_integral as f32;

            let shape_1 = LOOKUP_TABLE_I16_TABLE[shape_integral];
            let shape_2 = LOOKUP_TABLE_I16_TABLE[shape_integral + 1];

            let ws_index = 127.0 * out[i] + 128.0;
            let ws_index_integral = (ws_index as i32 & 255) as usize;
            let ws_index_fractional = ws_index - (ws_index as i32) as f32;

            let x0 = shape_1[ws_index_integral] as f32 / 32768.0;
            let x1 = shape_1[ws_index_integral + 1] as f32 / 32768.0;
            let x = x0 + (x1 - x0) * ws_index_fractional;

            let y0 = shape_2[ws_index_integral] as f32 / 32768.0;
            let y1 = shape_2[ws_index_integral + 1] as f32 / 32768.0;
            let y = y0 + (y1 - y0) * ws_index_fractional;

            let mix = x + (y - x) * shape_fractional;
            let index = mix * wf_gain_modulation.next() + 0.5;
            let fold = interpolate_hermite_fold(&LUT_FOLD, index, 512.0);
            let fold_2 = -interpolate_hermite_fold(&LUT_FOLD_2, index, 512.0);

            let sine = sine(aux[i] * 0.25 + 0.5);
            out[i] = fold;
            aux[i] = sine + (fold_2 - sine) * overtone_gain_modulation.next();
        }
        already_enveloped
    }

    fn post_processing_settings(&self) -> PostProcessingSettings {
        PostProcessingSettings {
            out_gain: 0.7,
            aux_gain: 0.6,
            already_enveloped: false,
        }
    }
}
