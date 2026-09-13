//! `plaits/dsp/fm/lfo.h` -- the DX7's single LFO, shared by every operator of
//! a voice, modulating pitch and/or amplitude after an optional fade-in delay.

use super::dx_units::{lfo_delay, lfo_frequency, pitch_mod_sensitivity};
use super::patch::ModulationParameters;
use crate::oscillator::sine;
use crate::utils::random;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Waveform {
    #[default]
    Triangle,
    RampDown,
    RampUp,
    Square,
    Sine,
    SampleAndHold,
}

impl From<u8> for Waveform {
    fn from(value: u8) -> Self {
        match value {
            1 => Waveform::RampDown,
            2 => Waveform::RampUp,
            3 => Waveform::Square,
            4 => Waveform::Sine,
            5 => Waveform::SampleAndHold,
            _ => Waveform::Triangle,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Lfo {
    phase: f32,
    frequency: f32,

    /// A fade-in ramp with two segments (see [`super::dx_units::lfo_delay`]):
    /// a "key-off silence" period, then a ramp up to full depth.
    delay_phase: f32,
    delay_increment: [f32; 2],

    value: f32,
    random_value: f32,
    /// Which delay increment segment [`Lfo::scrub`] last crossed into, so it
    /// only redraws `random_value` on an actual cycle boundary.
    phase_integral: i32,

    one_hz: f32,
    amp_mod_depth: f32,
    pitch_mod_depth: f32,

    waveform: Waveform,
    reset_phase: bool,
}

impl Lfo {
    pub fn new() -> Self {
        Self {
            phase: 0.0,
            frequency: 0.1,
            delay_phase: 0.0,
            delay_increment: [0.1, 0.1],
            value: 0.0,
            random_value: 0.0,
            phase_integral: 0,
            one_hz: 0.0,
            amp_mod_depth: 0.0,
            pitch_mod_depth: 0.0,
            waveform: Waveform::Triangle,
            reset_phase: false,
        }
    }

    pub fn init(&mut self, sample_rate: f32) {
        *self = Self {
            one_hz: 1.0 / sample_rate,
            ..Self::new()
        };
    }

    pub fn set(&mut self, modulations: &ModulationParameters) {
        self.frequency = lfo_frequency(modulations.rate) * self.one_hz;

        self.delay_increment = lfo_delay(modulations.delay);
        self.delay_increment[0] *= self.one_hz;
        self.delay_increment[1] *= self.one_hz;

        self.waveform = Waveform::from(modulations.waveform);
        self.reset_phase = modulations.reset_phase != 0;

        self.amp_mod_depth = modulations.amp_mod_depth as f32 * 0.01;
        self.pitch_mod_depth = modulations.pitch_mod_depth as f32
            * 0.01
            * pitch_mod_sensitivity(modulations.pitch_mod_sensitivity);
    }

    pub fn reset(&mut self) {
        if self.reset_phase {
            self.phase = 0.0;
        }
        self.delay_phase = 0.0;
    }

    /// Advances the LFO by `scale` samples (a whole render block at once).
    pub fn step(&mut self, scale: f32) {
        self.phase += scale * self.frequency;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
            self.random_value = random::get_float();
        }
        self.value = self.value();

        let segment = if self.delay_phase < 0.5 { 0 } else { 1 };
        self.delay_phase += scale * self.delay_increment[segment];
        self.delay_phase = self.delay_phase.min(1.0);
    }

    /// Evaluates the LFO `sample` samples after it was reset, for Plaits'
    /// envelope-scrubbing mode -- the free-running counterpart of
    /// [`Lfo::step`].
    pub fn scrub(&mut self, mut sample: f32) {
        let phase = sample * self.frequency;
        let phase_integral = phase as i32;
        self.phase = phase - phase_integral as f32;
        if phase_integral != self.phase_integral {
            self.phase_integral = phase_integral;
            self.random_value = random::get_float();
        }
        self.value = self.value();

        self.delay_phase = sample * self.delay_increment[0];
        if self.delay_phase > 0.5 {
            sample -= 0.5 / self.delay_increment[0];
            self.delay_phase = (0.5 + sample * self.delay_increment[1]).min(1.0);
        }
    }

    fn value(&self) -> f32 {
        match self.waveform {
            Waveform::Triangle => 2.0 * if self.phase < 0.5 { 0.5 - self.phase } else { self.phase - 0.5 },
            Waveform::RampDown => 1.0 - self.phase,
            Waveform::RampUp => self.phase,
            Waveform::Square => if self.phase < 0.5 { 0.0 } else { 1.0 },
            Waveform::Sine => 0.5 + 0.5 * sine(self.phase + 0.5),
            Waveform::SampleAndHold => self.random_value,
        }
    }

    /// `0` during the delay's silent segment, then ramps `0..1` over the
    /// fade-in segment.
    fn delay_ramp(&self) -> f32 {
        if self.delay_phase < 0.5 {
            0.0
        } else {
            (self.delay_phase - 0.5) * 2.0
        }
    }

    pub fn pitch_mod(&self) -> f32 {
        (self.value - 0.5) * self.delay_ramp() * self.pitch_mod_depth
    }

    pub fn amp_mod(&self) -> f32 {
        (1.0 - self.value) * self.delay_ramp() * self.amp_mod_depth
    }
}

impl Default for Lfo {
    fn default() -> Self {
        Self::new()
    }
}
