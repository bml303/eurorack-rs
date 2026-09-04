//! `braids/macro_oscillator.{h,cc}` -- the top-level oscillator that routes the
//! selected [`MacroOscillatorShape`] to the analog or digital engine and mixes
//! multiple analog voices for the compound shapes.

use stmlib::clip16_sym;
use stmlib::fixed::{interpolate_824_u16, interpolate_88_i16, mix_i16};

use crate::analog_oscillator::AnalogOscillator;
use crate::digital_oscillator::DigitalOscillator;
use crate::resources::{LUT_SVF_CUTOFF, WS_VIOLENT_OVERDRIVE};
use crate::shapes::{AnalogOscillatorShape, DigitalModel, MacroOscillatorShape};

/// Maximum audio block size (`kAudioBlockSize` on the hardware). `render` panics
/// if handed a larger block.
pub const MAX_BLOCK_SIZE: usize = 24;

const SEMI: i16 = 128;

#[rustfmt::skip]
static INTERVALS: [i16; 65] = [
    -24 * SEMI, -24 * SEMI, -24 * SEMI + 4,
    -23 * SEMI, -22 * SEMI, -21 * SEMI, -20 * SEMI, -19 * SEMI, -18 * SEMI,
    -17 * SEMI - 4, -17 * SEMI,
    -16 * SEMI, -15 * SEMI, -14 * SEMI, -13 * SEMI,
    -12 * SEMI - 4, -12 * SEMI,
    -11 * SEMI, -10 * SEMI, -9 * SEMI, -8 * SEMI,
    -7 * SEMI - 4, -7 * SEMI,
    -6 * SEMI, -5 * SEMI, -4 * SEMI, -3 * SEMI, -2 * SEMI, -1 * SEMI,
    -24, -8, -4, 0, 4, 8, 24,
    SEMI, 2 * SEMI, 3 * SEMI, 4 * SEMI, 5 * SEMI, 6 * SEMI,
    7 * SEMI, 7 * SEMI + 4,
    8 * SEMI, 9 * SEMI, 10 * SEMI, 11 * SEMI,
    12 * SEMI, 12 * SEMI + 4,
    13 * SEMI, 14 * SEMI, 15 * SEMI, 16 * SEMI,
    17 * SEMI, 17 * SEMI + 4,
    18 * SEMI, 19 * SEMI, 20 * SEMI, 21 * SEMI, 22 * SEMI, 23 * SEMI,
    24 * SEMI - 4, 24 * SEMI, 24 * SEMI,
];

pub struct MacroOscillator {
    parameter: [i16; 2],
    previous_parameter: [i16; 2],
    pitch: i16,
    sync_buffer: [u8; MAX_BLOCK_SIZE],
    temp_buffer: [i16; MAX_BLOCK_SIZE],
    lp_state: i32,
    analog: [AnalogOscillator; 3],
    digital: DigitalOscillator,
    shape: MacroOscillatorShape,
}

impl Default for MacroOscillator {
    fn default() -> Self {
        Self::new()
    }
}

impl MacroOscillator {
    pub fn new() -> Self {
        let mut o = MacroOscillator {
            parameter: [0; 2],
            previous_parameter: [0; 2],
            pitch: 0,
            sync_buffer: [0; MAX_BLOCK_SIZE],
            temp_buffer: [0; MAX_BLOCK_SIZE],
            lp_state: 0,
            analog: [
                AnalogOscillator::new(),
                AnalogOscillator::new(),
                AnalogOscillator::new(),
            ],
            digital: DigitalOscillator::new(),
            shape: MacroOscillatorShape::Csaw,
        };
        o.init();
        o
    }

    pub fn init(&mut self) {
        for a in &mut self.analog {
            a.init();
        }
        self.digital.init();
        self.lp_state = 0;
        self.previous_parameter[0] = 0;
        self.previous_parameter[1] = 0;
    }

    #[inline]
    pub fn set_shape(&mut self, shape: MacroOscillatorShape) {
        if shape != self.shape {
            self.strike();
        }
        self.shape = shape;
    }

    #[inline]
    pub fn shape(&self) -> MacroOscillatorShape {
        self.shape
    }

    #[inline]
    pub fn set_pitch(&mut self, pitch: i16) {
        self.pitch = pitch;
    }

    #[inline]
    pub fn pitch(&self) -> i16 {
        self.pitch
    }

    #[inline]
    pub fn set_parameters(&mut self, parameter_1: i16, parameter_2: i16) {
        self.parameter[0] = parameter_1;
        self.parameter[1] = parameter_2;
    }

    #[inline]
    pub fn strike(&mut self) {
        self.digital.strike();
    }

    /// `Render(sync_buffer, buffer, size)`.
    pub fn render(&mut self, sync: &[u8], buffer: &mut [i16], size: usize) {
        assert!(size <= MAX_BLOCK_SIZE);
        use MacroOscillatorShape::*;
        match self.shape {
            Csaw => self.render_csaw(sync, buffer, size),
            Morph => self.render_morph(sync, buffer, size),
            SawSquare => self.render_saw_square(sync, buffer, size),
            SineTriangle => self.render_sine_triangle(sync, buffer, size),
            Buzz => self.render_buzz(sync, buffer, size),
            SquareSub | SawSub => self.render_sub(sync, buffer, size),
            SquareSync | SawSync => self.render_dual_sync(sync, buffer, size),
            TripleSaw | TripleSquare | TripleTriangle | TripleSine => {
                self.render_triple(sync, buffer, size)
            }
            SawComb => self.render_saw_comb(sync, buffer, size),
            other => self.render_digital(other, sync, buffer, size),
        }
    }

    fn analog_pitch(&self, offset: i32) -> i16 {
        (self.pitch as i32 + offset) as i16
    }

    fn render_csaw(&mut self, sync: &[u8], buffer: &mut [i16], size: usize) {
        self.analog[0].set_pitch(self.pitch);
        self.analog[0].set_shape(AnalogOscillatorShape::Csaw);
        self.analog[0].set_parameter(self.parameter[0]);
        self.analog[0].set_aux_parameter(self.parameter[1]);
        self.analog[0].render(sync, buffer, None, size);
        let shift = ((-(self.parameter[1] as i32 - 32767)) >> 4) as i16;
        for b in buffer.iter_mut().take(size) {
            let s = *b as i32 + shift as i32;
            *b = ((s * 13) >> 3) as i16;
        }
    }

    fn render_morph(&mut self, sync: &[u8], buffer: &mut [i16], size: usize) {
        self.analog[0].set_pitch(self.pitch);
        self.analog[1].set_pitch(self.pitch);

        let balance: u16;
        if self.parameter[0] <= 10922 {
            self.analog[0].set_parameter(0);
            self.analog[1].set_parameter(0);
            self.analog[0].set_shape(AnalogOscillatorShape::Triangle);
            self.analog[1].set_shape(AnalogOscillatorShape::Saw);
            balance = (self.parameter[0] as i32 * 6) as u16;
        } else if self.parameter[0] <= 21845 {
            self.analog[0].set_parameter(0);
            self.analog[1].set_parameter(0);
            self.analog[0].set_shape(AnalogOscillatorShape::Square);
            self.analog[1].set_shape(AnalogOscillatorShape::Saw);
            balance = (65535 - (self.parameter[0] as i32 - 10923) * 6) as u16;
        } else {
            self.analog[0].set_parameter(((self.parameter[0] as i32 - 21846) * 3) as i16);
            self.analog[1].set_parameter(0);
            self.analog[0].set_shape(AnalogOscillatorShape::Square);
            self.analog[1].set_shape(AnalogOscillatorShape::Sine);
            balance = 0;
        }

        self.analog[0].render(sync, buffer, None, size);
        self.analog[1].render(sync, &mut self.temp_buffer[..size], None, size);

        let mut lp_cutoff = self.pitch as i32 - (self.parameter[1] as i32 >> 1) + 128 * 128;
        lp_cutoff = lp_cutoff.clamp(0, 32767);
        let f = interpolate_824_u16(&LUT_SVF_CUTOFF, (lp_cutoff << 17) as u32) as i32;
        let mut lp_state = self.lp_state;
        let mut fuzz_amount = (self.parameter[1] as i32) << 1;
        if self.pitch > (80 << 7) {
            fuzz_amount -= (self.pitch as i32 - (80 << 7)) << 4;
            if fuzz_amount < 0 {
                fuzz_amount = 0;
            }
        }
        for i in 0..size {
            let sample = mix_i16(buffer[i], self.temp_buffer[i], balance);
            let mut shifted_sample = sample as i32;
            lp_state += (shifted_sample - lp_state) * f >> 15;
            lp_state = clip16_sym(lp_state);
            shifted_sample = lp_state + 32768;
            let fuzzed = interpolate_88_i16(&WS_VIOLENT_OVERDRIVE, shifted_sample as u16);
            buffer[i] = mix_i16(sample, fuzzed, fuzz_amount as u16);
        }
        self.lp_state = lp_state;
    }

    fn render_saw_square(&mut self, sync: &[u8], buffer: &mut [i16], size: usize) {
        self.analog[0].set_parameter(self.parameter[0]);
        self.analog[1].set_parameter(self.parameter[0]);
        self.analog[0].set_pitch(self.pitch);
        self.analog[1].set_pitch(self.pitch);
        self.analog[0].set_shape(AnalogOscillatorShape::VariableSaw);
        self.analog[1].set_shape(AnalogOscillatorShape::Square);

        self.analog[0].render(sync, buffer, None, size);
        self.analog[1].render(sync, &mut self.temp_buffer[..size], None, size);

        let start = self.previous_parameter[1] as i32;
        let delta = self.parameter[1] as i32 - start;
        let increment = 32767 / size as i32;
        let mut xfade = 0i32;
        for i in 0..size {
            xfade += increment;
            let parameter_1 = start + (delta.wrapping_mul(xfade) >> 15);
            let balance = (parameter_1 << 1) as u16;
            let attenuated_square = ((self.temp_buffer[i] as i32) * 148 >> 8) as i16;
            buffer[i] = mix_i16(buffer[i], attenuated_square, balance);
        }
        self.previous_parameter[1] = self.parameter[1];
    }

    fn render_triple(&mut self, sync: &[u8], buffer: &mut [i16], size: usize) {
        let base_shape = match self.shape {
            MacroOscillatorShape::TripleSaw => AnalogOscillatorShape::Saw,
            MacroOscillatorShape::TripleTriangle => AnalogOscillatorShape::Triangle,
            MacroOscillatorShape::TripleSquare => AnalogOscillatorShape::Square,
            _ => AnalogOscillatorShape::Sine,
        };

        self.analog[0].set_parameter(0);
        self.analog[1].set_parameter(0);
        self.analog[2].set_parameter(0);

        self.analog[0].set_pitch(self.pitch);
        for i in 0..2 {
            let detune_1 = INTERVALS[(self.parameter[i] >> 9) as usize];
            let detune_2 = INTERVALS[(((self.parameter[i] >> 8) + 1) >> 1) as usize];
            let xfade = ((self.parameter[i] as u32) << 8) as u16;
            let detune =
                detune_1 as i32 + ((detune_2 as i32 - detune_1 as i32) * xfade as i32 >> 16);
            self.analog[i + 1].set_pitch(self.analog_pitch(detune));
        }

        self.analog[0].set_shape(base_shape);
        self.analog[1].set_shape(base_shape);
        self.analog[2].set_shape(base_shape);

        buffer[..size].fill(0);
        for i in 0..3 {
            self.analog[i].render(sync, &mut self.temp_buffer[..size], None, size);
            for j in 0..size {
                buffer[j] = buffer[j].wrapping_add(((self.temp_buffer[j] as i32 * 21) >> 6) as i16);
            }
        }
    }

    fn render_sub(&mut self, sync: &[u8], buffer: &mut [i16], size: usize) {
        let base_shape = if self.shape == MacroOscillatorShape::SquareSub {
            AnalogOscillatorShape::Square
        } else {
            AnalogOscillatorShape::VariableSaw
        };
        self.analog[0].set_parameter(self.parameter[0]);
        self.analog[0].set_shape(base_shape);
        self.analog[0].set_pitch(self.pitch);

        self.analog[1].set_parameter(0);
        self.analog[1].set_shape(AnalogOscillatorShape::Square);
        let octave = if self.parameter[1] < 16384 {
            24 << 7
        } else {
            12 << 7
        };
        self.analog[1].set_pitch(self.analog_pitch(-octave));

        self.analog[0].render(sync, buffer, None, size);
        self.analog[1].render(sync, &mut self.temp_buffer[..size], None, size);

        let start = self.previous_parameter[1] as i32;
        let delta = self.parameter[1] as i32 - start;
        let increment = 32767 / size as i32;
        let mut xfade = 0i32;
        for i in 0..size {
            xfade += increment;
            let parameter_1 = start + (delta.wrapping_mul(xfade) >> 15);
            let sub_gain = ((if parameter_1 < 16384 {
                16383 - parameter_1
            } else {
                parameter_1 - 16384
            }) << 1) as u16;
            buffer[i] = mix_i16(buffer[i], self.temp_buffer[i], sub_gain);
        }
        self.previous_parameter[1] = self.parameter[1];
    }

    fn render_dual_sync(&mut self, sync: &[u8], buffer: &mut [i16], size: usize) {
        let base_shape = if self.shape == MacroOscillatorShape::SquareSync {
            AnalogOscillatorShape::Square
        } else {
            AnalogOscillatorShape::Saw
        };
        self.analog[0].set_parameter(0);
        self.analog[0].set_shape(base_shape);
        self.analog[0].set_pitch(self.pitch);

        self.analog[1].set_parameter(0);
        self.analog[1].set_shape(base_shape);
        self.analog[1].set_pitch(self.analog_pitch((self.parameter[0] >> 2) as i32));

        self.analog[0].render(sync, buffer, Some(&mut self.sync_buffer[..size]), size);
        self.analog[1].render(
            &self.sync_buffer[..size],
            &mut self.temp_buffer[..size],
            None,
            size,
        );

        let start = self.previous_parameter[1] as i32;
        let delta = self.parameter[1] as i32 - start;
        let increment = 32767 / size as i32;
        let mut xfade = 0i32;
        for i in 0..size {
            xfade += increment;
            let parameter_1 = start + (delta.wrapping_mul(xfade) >> 15);
            let balance = (parameter_1 << 1) as u16;
            buffer[i] = ((mix_i16(buffer[i], self.temp_buffer[i], balance) as i32 >> 2) * 3) as i16;
        }
        self.previous_parameter[1] = self.parameter[1];
    }

    fn render_sine_triangle(&mut self, sync: &[u8], buffer: &mut [i16], size: usize) {
        let mut attenuation_sine = 32767 - 6 * (self.pitch as i32 - (92 << 7));
        let mut attenuation_tri = 32767 - 7 * (self.pitch as i32 - (80 << 7));
        attenuation_tri = attenuation_tri.clamp(0, 32767);
        attenuation_sine = attenuation_sine.clamp(0, 32767);

        let timbre = self.parameter[0] as i32;
        self.analog[0].set_parameter((timbre * attenuation_sine >> 15) as i16);
        self.analog[1].set_parameter((timbre * attenuation_tri >> 15) as i16);
        self.analog[0].set_pitch(self.pitch);
        self.analog[1].set_pitch(self.pitch);
        self.analog[0].set_shape(AnalogOscillatorShape::SineFold);
        self.analog[1].set_shape(AnalogOscillatorShape::TriangleFold);

        self.analog[0].render(sync, buffer, None, size);
        self.analog[1].render(sync, &mut self.temp_buffer[..size], None, size);

        let start = self.previous_parameter[1] as i32;
        let delta = self.parameter[1] as i32 - start;
        let increment = 32767 / size as i32;
        let mut xfade = 0i32;
        for i in 0..size {
            xfade += increment;
            let parameter_1 = start + (delta.wrapping_mul(xfade) >> 15);
            let balance = (parameter_1 << 1) as u16;
            buffer[i] = mix_i16(buffer[i], self.temp_buffer[i], balance);
        }
        self.previous_parameter[1] = self.parameter[1];
    }

    fn render_buzz(&mut self, sync: &[u8], buffer: &mut [i16], size: usize) {
        self.analog[0].set_parameter(self.parameter[0]);
        self.analog[0].set_shape(AnalogOscillatorShape::Buzz);
        self.analog[0].set_pitch(self.pitch);

        self.analog[1].set_parameter(self.parameter[0]);
        self.analog[1].set_shape(AnalogOscillatorShape::Buzz);
        self.analog[1].set_pitch(self.analog_pitch((self.parameter[1] >> 8) as i32));

        self.analog[0].render(sync, buffer, None, size);
        self.analog[1].render(sync, &mut self.temp_buffer[..size], None, size);
        for i in 0..size {
            buffer[i] = (buffer[i] >> 1).wrapping_add(self.temp_buffer[i] >> 1);
        }
    }

    fn render_digital(
        &mut self,
        shape: MacroOscillatorShape,
        sync: &[u8],
        buffer: &mut [i16],
        size: usize,
    ) {
        let model = DigitalModel::from_u8(shape as u8 - MacroOscillatorShape::TripleRingMod as u8)
            .expect("digital model index in range");
        self.digital
            .set_parameters(self.parameter[0], self.parameter[1]);
        self.digital.set_pitch(self.pitch);
        self.digital.set_shape(model);
        self.digital.render(sync, buffer, size);
    }

    fn render_saw_comb(&mut self, sync: &[u8], buffer: &mut [i16], size: usize) {
        self.analog[0].set_parameter(0);
        self.analog[0].set_pitch(self.pitch);
        self.analog[0].set_shape(AnalogOscillatorShape::Saw);
        self.analog[0].render(sync, buffer, None, size);

        self.digital
            .set_parameters(self.parameter[0], self.parameter[1]);
        self.digital.set_pitch(self.pitch);
        self.digital.set_shape(DigitalModel::Comb);
        self.digital.render(sync, buffer, size);
    }
}
