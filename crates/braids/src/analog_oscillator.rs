//! `braids/analog_oscillator.{h,cc}` -- the BLEP-based "analog style" waveforms.
//!
//! Faithful fixed-point port. Where the C divides by `phase_increment >> n` and
//! would hit a divide-by-zero (undefined behaviour) for sub-audio increments,
//! [`safe_div`] substitutes `0`; at real note pitches the divisor is never zero
//! so output is unaffected.

use stmlib::fixed::{crossfade, interpolate_824_i16, interpolate_88_i16};

use crate::dsp::{
    compute_phase_increment_analog, next_blep_sample, this_blep_sample, ParamRamp,
    PhaseIncrementRamp, HIGHEST_NOTE_ANALOG,
};
use crate::resources::{WAVEFORM_TABLE, WAV_SINE, WS_SINE_FOLD, WS_TRI_FOLD};
use crate::shapes::AnalogOscillatorShape;

const NUM_ZONES: usize = 15;
/// Index of `wav_bandlimited_comb_0` in [`WAVEFORM_TABLE`] (`WAV_BANDLIMITED_COMB_0`).
const WAV_BANDLIMITED_COMB_0: usize = 3;

#[inline]
fn safe_div(a: u32, b: u32) -> u32 {
    if b == 0 {
        0
    } else {
        a / b
    }
}

/// The C triangle bit-trick: `int16_t triangle = ((phase>>16)<<1) ^ mask;
/// triangle += 32768;` -- every step done in the exact C integer type.
#[inline]
fn triangle_from_phase(phase: u32) -> i16 {
    let phase_16 = (phase >> 16) as u16;
    let mask: u32 = if phase_16 & 0x8000 != 0 {
        0xffff
    } else {
        0x0000
    };
    let t = (((phase_16 as u32) << 1) ^ mask) as u16;
    // C: `int16_t triangle; triangle += 32768;` -- wraps in 16 bits.
    t.wrapping_add(0x8000) as i16
}

#[derive(Debug, Clone)]
pub struct AnalogOscillator {
    phase: u32,
    phase_increment: u32,
    previous_phase_increment: u32,
    high: bool,
    parameter: i16,
    previous_parameter: i16,
    aux_parameter: i16,
    discontinuity_depth: i16,
    pitch: i16,
    next_sample: i32,
    shape: AnalogOscillatorShape,
    previous_shape: AnalogOscillatorShape,
}

impl Default for AnalogOscillator {
    fn default() -> Self {
        Self::new()
    }
}

impl AnalogOscillator {
    pub fn new() -> Self {
        let mut o = Self {
            phase: 0,
            phase_increment: 1,
            previous_phase_increment: 0,
            high: false,
            parameter: 0,
            previous_parameter: 0,
            aux_parameter: 0,
            discontinuity_depth: -16383,
            pitch: 60 << 7,
            next_sample: 0,
            shape: AnalogOscillatorShape::Saw,
            previous_shape: AnalogOscillatorShape::Saw,
        };
        o.init();
        o
    }

    pub fn init(&mut self) {
        self.phase = 0;
        self.phase_increment = 1;
        self.high = false;
        self.parameter = 0;
        self.previous_parameter = 0;
        self.aux_parameter = 0;
        self.discontinuity_depth = -16383;
        self.pitch = 60 << 7;
        self.next_sample = 0;
    }

    #[inline]
    pub fn set_shape(&mut self, shape: AnalogOscillatorShape) {
        self.shape = shape;
    }
    #[inline]
    pub fn set_pitch(&mut self, pitch: i16) {
        self.pitch = pitch;
    }
    #[inline]
    pub fn set_parameter(&mut self, parameter: i16) {
        self.parameter = parameter;
    }
    #[inline]
    pub fn set_aux_parameter(&mut self, parameter: i16) {
        self.aux_parameter = parameter;
    }
    #[inline]
    pub fn phase_increment(&self) -> u32 {
        self.phase_increment
    }
    #[inline]
    pub fn reset(&mut self) {
        self.phase = self.phase_increment.wrapping_neg();
    }

    /// `Render(sync_in, buffer, sync_out, size)`.
    pub fn render(
        &mut self,
        sync_in: &[u8],
        buffer: &mut [i16],
        mut sync_out: Option<&mut [u8]>,
        size: usize,
    ) {
        if self.shape != self.previous_shape {
            self.init();
            self.previous_shape = self.shape;
        }

        self.phase_increment = compute_phase_increment_analog(self.pitch);

        if self.pitch as i32 > HIGHEST_NOTE_ANALOG {
            self.pitch = HIGHEST_NOTE_ANALOG as i16;
        } else if self.pitch < 0 {
            self.pitch = 0;
        }

        let sout = sync_out.as_deref_mut();
        match self.shape {
            AnalogOscillatorShape::Saw => self.render_saw(sync_in, buffer, sout, size),
            AnalogOscillatorShape::VariableSaw => {
                self.render_variable_saw(sync_in, buffer, sout, size)
            }
            AnalogOscillatorShape::Csaw => self.render_csaw(sync_in, buffer, sout, size),
            AnalogOscillatorShape::Square => self.render_square(sync_in, buffer, sout, size),
            AnalogOscillatorShape::Triangle => self.render_triangle(sync_in, buffer, sout, size),
            AnalogOscillatorShape::Sine => self.render_sine(sync_in, buffer, sout, size),
            AnalogOscillatorShape::TriangleFold => {
                self.render_triangle_fold(sync_in, buffer, sout, size)
            }
            AnalogOscillatorShape::SineFold => self.render_sine_fold(sync_in, buffer, sout, size),
            AnalogOscillatorShape::Buzz => self.render_buzz(sync_in, buffer, sout, size),
        }
    }

    #[inline]
    fn write_sync_out(
        sync_out: &mut Option<&mut [u8]>,
        i: usize,
        phase: u32,
        phase_increment: u32,
    ) {
        if let Some(s) = sync_out.as_deref_mut() {
            s[i] = if phase < phase_increment {
                (safe_div(phase, phase_increment >> 7) + 1) as u8
            } else {
                0
            };
        }
    }

    fn render_csaw(
        &mut self,
        sync_in: &[u8],
        buffer: &mut [i16],
        mut sync_out: Option<&mut [u8]>,
        size: usize,
    ) {
        let mut ramp =
            PhaseIncrementRamp::new(self.previous_phase_increment, self.phase_increment, size);
        let mut next_sample = self.next_sample;
        for i in 0..size {
            let mut sync_reset = false;
            let mut self_reset = false;
            let mut transition_during_reset = false;
            let mut reset_time = 0u32;
            let phase_increment = ramp.next();

            let mut pw = (self.parameter as u32).wrapping_mul(49152);
            if pw < 8u32.wrapping_mul(phase_increment) {
                pw = 8u32.wrapping_mul(phase_increment);
            }

            let mut this_sample = next_sample;
            next_sample = 0;

            if sync_in[i] != 0 {
                reset_time = ((sync_in[i] - 1) as u32) << 9;
                let phase_at_reset = self
                    .phase
                    .wrapping_add((65535 - reset_time).wrapping_mul(phase_increment >> 16));
                sync_reset = true;
                transition_during_reset = false;
                if phase_at_reset < self.phase || (!self.high && phase_at_reset >= pw) {
                    transition_during_reset = true;
                }
                if self.phase >= pw {
                    self.discontinuity_depth = (-2048 + (self.aux_parameter >> 2)) as i16;
                    let before = (phase_at_reset >> 18) as i32;
                    let after = self.discontinuity_depth as i32;
                    let discontinuity = after - before;
                    this_sample += discontinuity.wrapping_mul(this_blep_sample(reset_time)) >> 15;
                    next_sample += discontinuity.wrapping_mul(next_blep_sample(reset_time)) >> 15;
                }
            }

            self.phase = self.phase.wrapping_add(phase_increment);
            if self.phase < phase_increment {
                self_reset = true;
            }
            Self::write_sync_out(&mut sync_out, i, self.phase, phase_increment);

            while transition_during_reset || !sync_reset {
                if !self.high {
                    if self.phase < pw {
                        break;
                    }
                    let t = safe_div(self.phase - pw, phase_increment >> 16);
                    let before = self.discontinuity_depth as i32;
                    let after = (self.phase >> 18) as i16 as i32;
                    let discontinuity = after - before;
                    this_sample += discontinuity.wrapping_mul(this_blep_sample(t)) >> 15;
                    next_sample += discontinuity.wrapping_mul(next_blep_sample(t)) >> 15;
                    self.high = true;
                }
                if self.high {
                    if !self_reset {
                        break;
                    }
                    self_reset = false;
                    self.discontinuity_depth = (-2048 + (self.aux_parameter >> 2)) as i16;
                    let t = safe_div(self.phase, phase_increment >> 16);
                    let before = 16383i32;
                    let after = self.discontinuity_depth as i32;
                    let discontinuity = after - before;
                    this_sample += discontinuity.wrapping_mul(this_blep_sample(t)) >> 15;
                    next_sample += discontinuity.wrapping_mul(next_blep_sample(t)) >> 15;
                    self.high = false;
                }
            }

            if sync_reset {
                self.phase = reset_time.wrapping_mul(phase_increment >> 16);
                self.high = false;
            }

            next_sample += if self.phase < pw {
                self.discontinuity_depth as i32
            } else {
                (self.phase >> 18) as i32
            };
            buffer[i] = (this_sample - 8192).wrapping_mul(2) as i16;
        }
        self.next_sample = next_sample;
        self.previous_phase_increment = ramp.value();
    }

    fn render_square(
        &mut self,
        sync_in: &[u8],
        buffer: &mut [i16],
        mut sync_out: Option<&mut [u8]>,
        size: usize,
    ) {
        let mut ramp =
            PhaseIncrementRamp::new(self.previous_phase_increment, self.phase_increment, size);
        if self.parameter > 32000 {
            self.parameter = 32000;
        }
        let mut next_sample = self.next_sample;
        for i in 0..size {
            let mut sync_reset = false;
            let mut self_reset = false;
            let mut transition_during_reset = false;
            let mut reset_time = 0u32;
            let phase_increment = ramp.next();
            let pw = ((32768 - self.parameter as i32) as u32) << 16;

            let mut this_sample = next_sample;
            next_sample = 0;

            if sync_in[i] != 0 {
                reset_time = ((sync_in[i] - 1) as u32) << 9;
                let phase_at_reset = self
                    .phase
                    .wrapping_add((65535 - reset_time).wrapping_mul(phase_increment >> 16));
                sync_reset = true;
                if phase_at_reset < self.phase || (!self.high && phase_at_reset >= pw) {
                    transition_during_reset = true;
                }
                if phase_at_reset >= pw {
                    this_sample -= this_blep_sample(reset_time);
                    next_sample -= next_blep_sample(reset_time);
                }
            }

            self.phase = self.phase.wrapping_add(phase_increment);
            if self.phase < phase_increment {
                self_reset = true;
            }
            Self::write_sync_out(&mut sync_out, i, self.phase, phase_increment);

            while transition_during_reset || !sync_reset {
                if !self.high {
                    if self.phase < pw {
                        break;
                    }
                    let t = safe_div(self.phase - pw, phase_increment >> 16);
                    this_sample += this_blep_sample(t);
                    next_sample += next_blep_sample(t);
                    self.high = true;
                }
                if self.high {
                    if !self_reset {
                        break;
                    }
                    self_reset = false;
                    let t = safe_div(self.phase, phase_increment >> 16);
                    this_sample -= this_blep_sample(t);
                    next_sample -= next_blep_sample(t);
                    self.high = false;
                }
            }

            if sync_reset {
                self.phase = reset_time.wrapping_mul(phase_increment >> 16);
                self.high = false;
            }

            next_sample += if self.phase < pw { 0 } else { 32767 };
            buffer[i] = (this_sample - 16384).wrapping_mul(2) as i16;
        }
        self.next_sample = next_sample;
        self.previous_phase_increment = ramp.value();
    }

    fn render_saw(
        &mut self,
        sync_in: &[u8],
        buffer: &mut [i16],
        mut sync_out: Option<&mut [u8]>,
        size: usize,
    ) {
        let mut ramp =
            PhaseIncrementRamp::new(self.previous_phase_increment, self.phase_increment, size);
        let mut next_sample = self.next_sample;
        for i in 0..size {
            let mut sync_reset = false;
            let mut self_reset = false;
            let mut transition_during_reset = false;
            let mut reset_time = 0u32;
            let phase_increment = ramp.next();
            let mut this_sample = next_sample;
            next_sample = 0;

            if sync_in[i] != 0 {
                reset_time = ((sync_in[i] - 1) as u32) << 9;
                let phase_at_reset = self
                    .phase
                    .wrapping_add((65535 - reset_time).wrapping_mul(phase_increment >> 16));
                sync_reset = true;
                if phase_at_reset < self.phase {
                    transition_during_reset = true;
                }
                let discontinuity = (phase_at_reset >> 17) as i32;
                this_sample -= discontinuity.wrapping_mul(this_blep_sample(reset_time)) >> 15;
                next_sample -= discontinuity.wrapping_mul(next_blep_sample(reset_time)) >> 15;
            }

            self.phase = self.phase.wrapping_add(phase_increment);
            if self.phase < phase_increment {
                self_reset = true;
            }
            Self::write_sync_out(&mut sync_out, i, self.phase, phase_increment);

            if (transition_during_reset || !sync_reset) && self_reset {
                let t = safe_div(self.phase, phase_increment >> 16);
                this_sample -= this_blep_sample(t);
                next_sample -= next_blep_sample(t);
            }

            if sync_reset {
                self.phase = reset_time.wrapping_mul(phase_increment >> 16);
                self.high = false;
            }

            next_sample += (self.phase >> 17) as i32;
            buffer[i] = (this_sample - 16384).wrapping_mul(2) as i16;
        }
        self.next_sample = next_sample;
        self.previous_phase_increment = ramp.value();
    }

    fn render_variable_saw(
        &mut self,
        sync_in: &[u8],
        buffer: &mut [i16],
        mut sync_out: Option<&mut [u8]>,
        size: usize,
    ) {
        let mut ramp =
            PhaseIncrementRamp::new(self.previous_phase_increment, self.phase_increment, size);
        let mut next_sample = self.next_sample;
        if self.parameter < 1024 {
            self.parameter = 1024;
        }
        for i in 0..size {
            let mut sync_reset = false;
            let mut self_reset = false;
            let mut transition_during_reset = false;
            let mut reset_time = 0u32;
            let phase_increment = ramp.next();
            let pw = (self.parameter as u32) << 16;

            let mut this_sample = next_sample;
            next_sample = 0;

            if sync_in[i] != 0 {
                reset_time = ((sync_in[i] - 1) as u32) << 9;
                let phase_at_reset = self
                    .phase
                    .wrapping_add((65535 - reset_time).wrapping_mul(phase_increment >> 16));
                sync_reset = true;
                if phase_at_reset < self.phase || (!self.high && phase_at_reset >= pw) {
                    transition_during_reset = true;
                }
                let before = ((phase_at_reset >> 18) as i32)
                    + (((phase_at_reset.wrapping_sub(pw)) >> 18) as i32);
                let after = (0i32 >> 18) + (((0u32.wrapping_sub(pw)) >> 18) as i32);
                let discontinuity = after - before;
                this_sample += discontinuity.wrapping_mul(this_blep_sample(reset_time)) >> 15;
                next_sample += discontinuity.wrapping_mul(next_blep_sample(reset_time)) >> 15;
            }

            self.phase = self.phase.wrapping_add(phase_increment);
            if self.phase < phase_increment {
                self_reset = true;
            }
            Self::write_sync_out(&mut sync_out, i, self.phase, phase_increment);

            while transition_during_reset || !sync_reset {
                if !self.high {
                    if self.phase < pw {
                        break;
                    }
                    let t = safe_div(self.phase - pw, phase_increment >> 16);
                    this_sample -= this_blep_sample(t) >> 1;
                    next_sample -= next_blep_sample(t) >> 1;
                    self.high = true;
                }
                if self.high {
                    if !self_reset {
                        break;
                    }
                    self_reset = false;
                    let t = safe_div(self.phase, phase_increment >> 16);
                    this_sample -= this_blep_sample(t) >> 1;
                    next_sample -= next_blep_sample(t) >> 1;
                    self.high = false;
                }
            }

            if sync_reset {
                self.phase = reset_time.wrapping_mul(phase_increment >> 16);
                self.high = false;
            }

            next_sample += (self.phase >> 18) as i32;
            next_sample += (self.phase.wrapping_sub(pw) >> 18) as i32;
            buffer[i] = (this_sample - 16384).wrapping_mul(2) as i16;
        }
        self.next_sample = next_sample;
        self.previous_phase_increment = ramp.value();
    }

    fn render_triangle(
        &mut self,
        sync_in: &[u8],
        buffer: &mut [i16],
        _sync_out: Option<&mut [u8]>,
        size: usize,
    ) {
        let mut ramp =
            PhaseIncrementRamp::new(self.previous_phase_increment, self.phase_increment, size);
        let mut phase = self.phase;
        for i in 0..size {
            let phase_increment = ramp.next();
            if sync_in[i] != 0 {
                phase = 0;
            }

            phase = phase.wrapping_add(phase_increment >> 1);
            buffer[i] = triangle_from_phase(phase) >> 1;

            phase = phase.wrapping_add(phase_increment >> 1);
            buffer[i] = buffer[i].wrapping_add(triangle_from_phase(phase) >> 1);
        }
        self.phase = phase;
        self.previous_phase_increment = ramp.value();
    }

    fn render_sine(
        &mut self,
        sync_in: &[u8],
        buffer: &mut [i16],
        _sync_out: Option<&mut [u8]>,
        size: usize,
    ) {
        let mut phase = self.phase;
        let mut ramp =
            PhaseIncrementRamp::new(self.previous_phase_increment, self.phase_increment, size);
        for i in 0..size {
            let phase_increment = ramp.next();
            phase = phase.wrapping_add(phase_increment);
            if sync_in[i] != 0 {
                phase = 0;
            }
            buffer[i] = interpolate_824_i16(&WAV_SINE, phase);
        }
        self.previous_phase_increment = ramp.value();
        self.phase = phase;
    }

    fn render_triangle_fold(
        &mut self,
        sync_in: &[u8],
        buffer: &mut [i16],
        _sync_out: Option<&mut [u8]>,
        size: usize,
    ) {
        let mut phase = self.phase;
        let mut pi_ramp =
            PhaseIncrementRamp::new(self.previous_phase_increment, self.phase_increment, size);
        let mut p_ramp =
            ParamRamp::new(self.previous_parameter as i32, self.parameter as i32, size);
        for i in 0..size {
            let parameter = p_ramp.next();
            let phase_increment = pi_ramp.next();

            let gain = (2048 + (parameter.wrapping_mul(30720) >> 15)) as i16;

            if sync_in[i] != 0 {
                phase = 0;
            }

            phase = phase.wrapping_add(phase_increment >> 1);
            let mut triangle = triangle_from_phase(phase);
            triangle = ((triangle as i32).wrapping_mul(gain as i32) >> 15) as i16;
            triangle = interpolate_88_i16(&WS_TRI_FOLD, (triangle as i32 + 32768) as u16);
            buffer[i] = triangle >> 1;

            phase = phase.wrapping_add(phase_increment >> 1);
            triangle = triangle_from_phase(phase);
            triangle = ((triangle as i32).wrapping_mul(gain as i32) >> 15) as i16;
            triangle = interpolate_88_i16(&WS_TRI_FOLD, (triangle as i32 + 32768) as u16);
            buffer[i] = buffer[i].wrapping_add(triangle >> 1);
        }
        self.previous_parameter = self.parameter;
        self.previous_phase_increment = pi_ramp.value();
        self.phase = phase;
    }

    fn render_sine_fold(
        &mut self,
        sync_in: &[u8],
        buffer: &mut [i16],
        _sync_out: Option<&mut [u8]>,
        size: usize,
    ) {
        let mut phase = self.phase;
        let mut pi_ramp =
            PhaseIncrementRamp::new(self.previous_phase_increment, self.phase_increment, size);
        let mut p_ramp =
            ParamRamp::new(self.previous_parameter as i32, self.parameter as i32, size);
        for i in 0..size {
            let parameter = p_ramp.next();
            let phase_increment = pi_ramp.next();
            let gain = (2048 + (parameter.wrapping_mul(30720) >> 15)) as i16;

            if sync_in[i] != 0 {
                phase = 0;
            }

            phase = phase.wrapping_add(phase_increment >> 1);
            let mut sine = interpolate_824_i16(&WAV_SINE, phase);
            sine = ((sine as i32).wrapping_mul(gain as i32) >> 15) as i16;
            sine = interpolate_88_i16(&WS_SINE_FOLD, (sine as i32 + 32768) as u16);
            buffer[i] = (sine >> 1) as i16;

            phase = phase.wrapping_add(phase_increment >> 1);
            sine = interpolate_824_i16(&WAV_SINE, phase);
            sine = ((sine as i32).wrapping_mul(gain as i32) >> 15) as i16;
            sine = interpolate_88_i16(&WS_SINE_FOLD, (sine as i32 + 32768) as u16);
            buffer[i] = buffer[i].wrapping_add(sine >> 1);
        }
        self.previous_parameter = self.parameter;
        self.previous_phase_increment = pi_ramp.value();
        self.phase = phase;
    }

    fn render_buzz(
        &mut self,
        sync_in: &[u8],
        buffer: &mut [i16],
        _sync_out: Option<&mut [u8]>,
        size: usize,
    ) {
        let shifted_pitch = self.pitch as i32 + ((32767 - self.parameter as i32) >> 1);
        let crossfade_amt = (shifted_pitch << 6) as u16;
        let mut index = (shifted_pitch >> 10) as usize;
        if index >= NUM_ZONES {
            index = NUM_ZONES - 1;
        }
        let wave_1 = WAVEFORM_TABLE[WAV_BANDLIMITED_COMB_0 + index];
        index += 1;
        if index >= NUM_ZONES {
            index = NUM_ZONES - 1;
        }
        let wave_2 = WAVEFORM_TABLE[WAV_BANDLIMITED_COMB_0 + index];
        for i in 0..size {
            self.phase = self.phase.wrapping_add(self.phase_increment);
            if sync_in[i] != 0 {
                self.phase = 0;
            }
            buffer[i] = crossfade(wave_1, wave_2, self.phase, crossfade_amt);
        }
    }
}
