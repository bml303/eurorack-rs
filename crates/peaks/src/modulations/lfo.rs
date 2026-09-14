//! `peaks/modulations/lfo.{h,cc}` -- a 5-shape LFO (sine/wavefold,
//! variable-slope triangle, variable-width square, quantized "steps",
//! interpolated noise), with an optional tap-tempo sync mode that predicts
//! the incoming clock's period via `stmlib::PatternPredictor`.

use stmlib::fixed::{interpolate_1022, interpolate_824_u16};
use stmlib::pattern_predictor::PatternPredictor;
use stmlib::random::Random;

use crate::gate_processor::{ControlMode, GateFlags};
use crate::resources::{LUT_LFO_INCREMENTS, LUT_RAISED_COSINE, WAV_FOLD_POWER, WAV_FOLD_SINE, WAV_SINE};

const K_SLOPE_BITS: u32 = 12;
const K_SYNC_COUNTER_MAX_TIME: u32 = 8 * 48000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u16)]
pub enum LfoShape {
    #[default]
    Sine,
    Triangle,
    Square,
    Steps,
    Noise,
}

impl LfoShape {
    fn from_u16(value: u16) -> Self {
        match value {
            0 => LfoShape::Sine,
            1 => LfoShape::Triangle,
            2 => LfoShape::Square,
            3 => LfoShape::Steps,
            _ => LfoShape::Noise,
        }
    }
}

const PRESETS: [(LfoShape, i16); 7] = [
    (LfoShape::Sine, 0),
    (LfoShape::Triangle, 0),
    (LfoShape::Triangle, 32767),
    (LfoShape::Square, 0),
    (LfoShape::Steps, 0),
    (LfoShape::Noise, -32767),
    (LfoShape::Noise, 32767),
];

#[derive(Debug, Clone, Copy)]
pub struct Lfo {
    rate: u16,
    shape: LfoShape,
    parameter: i16,
    reset_phase: i32,
    level: i32,

    sync: bool,
    sync_counter: u32,
    pattern_predictor: PatternPredictor<32, 9>,

    phase: u32,
    phase_increment: u32,

    period: u32,
    end_of_attack: u32,
    attack_factor: u32,
    decay_factor: u32,
    previous_parameter: i16,

    value: i32,
    next_value: i32,
}

impl Default for Lfo {
    fn default() -> Self {
        Self {
            rate: 0,
            shape: LfoShape::Square,
            parameter: 0,
            reset_phase: 0,
            level: 32767,
            sync: false,
            sync_counter: K_SYNC_COUNTER_MAX_TIME,
            pattern_predictor: PatternPredictor::default(),
            phase: 0,
            phase_increment: 0,
            period: 0,
            end_of_attack: 0,
            attack_factor: 0,
            decay_factor: 0,
            previous_parameter: 32767,
            value: 0,
            next_value: 0,
        }
    }
}

impl Lfo {
    pub fn init(&mut self) {
        self.rate = 0;
        self.shape = LfoShape::Square;
        self.parameter = 0;
        self.reset_phase = 0;
        self.sync = false;
        self.previous_parameter = 32767;
        self.sync_counter = K_SYNC_COUNTER_MAX_TIME;
        self.level = 32767;
        self.pattern_predictor.init();
    }

    pub fn configure(&mut self, parameter: &[u16; 4], control_mode: ControlMode) {
        if control_mode == ControlMode::Half {
            if self.sync {
                self.set_shape_integer(parameter[0]);
                self.set_parameter((parameter[1] as i32 - 32768) as i16);
            } else {
                self.set_rate(parameter[0]);
                self.set_shape_parameter_preset(parameter[1]);
            }
            self.set_reset_phase(0);
            self.set_level(40960);
        } else if self.sync {
            self.set_level(parameter[0]);
            self.set_shape_integer(parameter[1]);
            self.set_parameter((parameter[2] as i32 - 32768) as i16);
            self.set_reset_phase((parameter[3] as i32 - 32768) as i16);
        } else {
            self.set_level(40960);
            self.set_rate(parameter[0]);
            self.set_shape_integer(parameter[1]);
            self.set_parameter((parameter[2] as i32 - 32768) as i16);
            self.set_reset_phase((parameter[3] as i32 - 32768) as i16);
        }
    }

    pub fn set_rate(&mut self, rate: u16) {
        self.rate = rate;
    }

    pub fn set_shape(&mut self, shape: LfoShape) {
        self.shape = shape;
    }

    pub fn set_shape_integer(&mut self, value: u16) {
        self.shape = LfoShape::from_u16(((value as u32).wrapping_mul(5) >> 16) as u16);
    }

    pub fn set_shape_parameter_preset(&mut self, value: u16) {
        let value = (((value >> 8) as u32).wrapping_mul(7) >> 8) as usize;
        let (shape, parameter) = PRESETS[value];
        self.set_shape(shape);
        self.set_parameter(parameter);
    }

    pub fn set_parameter(&mut self, parameter: i16) {
        self.parameter = parameter;
    }

    pub fn set_reset_phase(&mut self, reset_phase: i16) {
        self.reset_phase = (reset_phase as i32) << 16;
    }

    pub fn set_sync(&mut self, sync: bool) {
        if !self.sync && sync {
            self.pattern_predictor.init();
        }
        self.sync = sync;
    }

    pub fn set_level(&mut self, level: u16) {
        self.level = (level >> 1) as i32;
    }

    pub fn process(&mut self, gate_flags: &[GateFlags], out: &mut [i16]) {
        if !self.sync {
            let idx = (self.rate >> 8) as usize;
            let a = LUT_LFO_INCREMENTS[idx] as i32;
            let b = LUT_LFO_INCREMENTS[idx + 1] as i32;
            self.phase_increment = a.wrapping_add(((b - a) >> 1).wrapping_mul((self.rate & 0xff) as i32) >> 7) as u32;
        }
        for (&gate_flag, o) in gate_flags.iter().zip(out.iter_mut()) {
            self.sync_counter = self.sync_counter.wrapping_add(1);
            if gate_flag.contains(GateFlags::RISING) {
                let mut reset_phase = true;
                if self.sync {
                    if self.sync_counter < K_SYNC_COUNTER_MAX_TIME {
                        let period;
                        if gate_flag.contains(GateFlags::FROM_BUTTON) {
                            period = self.sync_counter;
                        } else if self.sync_counter < 1920 {
                            period = (3u32.wrapping_mul(self.period).wrapping_add(self.sync_counter)) >> 2;
                            reset_phase = false;
                        } else {
                            period = self.pattern_predictor.predict(self.sync_counter as i32);
                        }
                        if period != self.period {
                            self.period = period;
                            self.phase_increment = 0xffffffffu32 / self.period;
                        }
                    }
                    self.sync_counter = 0;
                }
                if reset_phase {
                    self.phase = self.reset_phase as u32;
                }
            }
            self.phase = self.phase.wrapping_add(self.phase_increment);
            let sample = self.compute_sample();
            *o = (sample.wrapping_mul(self.level) >> 15) as i16;
        }
    }

    fn compute_sample(&mut self) -> i32 {
        match self.shape {
            LfoShape::Sine => self.compute_sample_sine() as i32,
            LfoShape::Triangle => self.compute_sample_triangle() as i32,
            LfoShape::Square => self.compute_sample_square() as i32,
            LfoShape::Steps => self.compute_sample_steps() as i32,
            LfoShape::Noise => self.compute_sample_noise() as i32,
        }
    }

    fn compute_sample_sine(&mut self) -> i16 {
        let phase = self.phase;
        let sine = interpolate_1022(&WAV_SINE, phase);
        if self.parameter > 0 {
            let wf_balance = self.parameter as i32;
            let wf_gain = 2048 + ((self.parameter as i32).wrapping_mul(65535 - 2048) >> 15);
            let original = sine as i32;
            let folded = interpolate_1022(&WAV_FOLD_SINE, (original.wrapping_mul(wf_gain)).wrapping_add(1i32 << 31) as u32) as i32;
            (original.wrapping_add((folded.wrapping_sub(original)).wrapping_mul(wf_balance) >> 15)) as i16
        } else {
            let wf_balance = -(self.parameter as i32);
            let original = sine as i32;
            let phase = phase.wrapping_add(1u32 << 30);
            let tri: i32 = if phase < (1u32 << 31) { (phase << 1) as i32 } else { !((phase << 1) as i32) };
            let folded = interpolate_1022(&WAV_FOLD_POWER, tri as u32) as i32;
            (original.wrapping_add((folded.wrapping_sub(original)).wrapping_mul(wf_balance) >> 15)) as i16
        }
    }

    fn compute_sample_triangle(&mut self) -> i16 {
        if self.parameter != self.previous_parameter {
            let slope_offset = (self.parameter as i32 + 32768) as u16;
            if slope_offset <= 1 {
                self.decay_factor = 32768u32 << K_SLOPE_BITS;
                self.attack_factor = 1u32 << (K_SLOPE_BITS - 1);
            } else {
                self.decay_factor = (32768u32 << K_SLOPE_BITS) / slope_offset as u32;
                self.attack_factor = (32768u32 << K_SLOPE_BITS) / (65536 - slope_offset as u32);
            }
            self.end_of_attack = (slope_offset as u32) << 16;
            self.previous_parameter = self.parameter;
        }

        let phase = self.phase;
        let skewed_phase: u32 = if phase < self.end_of_attack {
            (phase >> K_SLOPE_BITS).wrapping_mul(self.decay_factor)
        } else {
            ((phase.wrapping_sub(self.end_of_attack)) >> K_SLOPE_BITS).wrapping_mul(self.attack_factor).wrapping_add(1u32 << 31)
        };
        if skewed_phase < (1u32 << 31) {
            (-32768i32 + (skewed_phase >> 15) as i32) as i16
        } else {
            (32767i32 - (skewed_phase >> 15) as i32) as i16
        }
    }

    fn compute_sample_square(&self) -> i16 {
        let mut threshold = ((self.parameter as i32 + 32768) as u32) << 16;
        if threshold < (self.phase_increment << 1) {
            threshold = self.phase_increment << 1;
        } else if !threshold < (self.phase_increment << 1) {
            threshold = !(self.phase_increment << 1);
        }
        if self.phase < threshold {
            32767
        } else {
            -32767
        }
    }

    fn compute_sample_steps(&self) -> i16 {
        let quantization_levels: u32 = 2 + (((self.parameter as i32 + 32768) as u32).wrapping_mul(15) >> 16);
        let scale: u32 = 65535 / (quantization_levels - 1);
        let phase = self.phase;
        let tri_phase = phase;
        let tri: u32 = if tri_phase < (1u32 << 31) { tri_phase << 1 } else { !(tri_phase << 1) };
        (((((tri >> 16).wrapping_mul(quantization_levels)) >> 16).wrapping_mul(scale)) as i32 - 32768) as i16
    }

    fn compute_sample_noise(&mut self) -> i16 {
        let phase = self.phase;
        if phase < self.phase_increment {
            self.value = self.next_value;
            self.next_value = Random::get_sample() as i32;
        }
        let linear_interpolation = self.value.wrapping_add((self.next_value.wrapping_sub(self.value)).wrapping_mul((phase >> 17) as i32) >> 15);
        if self.parameter < 0 {
            let balance = self.parameter as i32 + 32767;
            (self.value.wrapping_add((linear_interpolation.wrapping_sub(self.value)).wrapping_mul(balance) >> 15)) as i16
        } else {
            let raised_cosine = (interpolate_824_u16(&LUT_RAISED_COSINE, phase) >> 1) as i32;
            let smooth_interpolation = self.value.wrapping_add((self.next_value.wrapping_sub(self.value)).wrapping_mul(raised_cosine) >> 15);
            (linear_interpolation.wrapping_add((smooth_interpolation.wrapping_sub(linear_interpolation)).wrapping_mul(self.parameter as i32) >> 15)) as i16
        }
    }
}
