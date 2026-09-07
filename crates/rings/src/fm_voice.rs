//! `rings/dsp/fm_voice.{h,cc}` -- the "bonus" 2-operator FM voice, morphing
//! between a plain oscillator and an FM-LPG "thing" as `damping` rises.

use stmlib::fdsp::{one_pole, slew, slope};
use stmlib::parameter_interpolator::ParameterInterpolator;
use stmlib::units::semitones_to_ratio;

use crate::dsp::SAMPLE_RATE;
use crate::follower::Follower;
use crate::resources::{LUT_FM_FREQUENCY_QUANTIZER, LUT_SINE};

/// `Interpolate` clamped in bounds (see `resonator.rs`).
#[inline]
fn interpolate(table: &[f32], index: f32, size: f32) -> f32 {
    let scaled = index.clamp(0.0, 1.0) * size;
    let last = table.len() - 1;
    let integral = (scaled as usize).min(last);
    let fractional = scaled - integral as f32;
    let a = table[integral];
    let b = table[(integral + 1).min(last)];
    a + (b - a) * fractional
}

#[inline]
fn f32_to_u32_wrap(x: f32) -> u32 {
    (x as i64) as u32
}

#[inline]
fn sine_fm(phase: u32, fm: f32) -> f32 {
    let phase = phase.wrapping_add(f32_to_u32_wrap((fm + 4.0) * 536_870_912.0).wrapping_shl(3));
    let integral = (phase >> 20) as usize;
    let fractional = (phase << 12) as f32 / 4_294_967_296.0;
    let a = LUT_SINE[integral];
    let b = LUT_SINE[integral + 1];
    a + (b - a) * fractional
}

/// `rings::FMVoice`.
///
/// `position` / `previous_damping` are kept for parity with the C (which also
/// leaves them unused in `Process`).
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct FmVoice {
    carrier_frequency: f32,
    ratio: f32,
    brightness: f32,
    damping: f32,
    position: f32,
    feedback_amount: f32,

    previous_carrier_frequency: f32,
    previous_modulator_frequency: f32,
    previous_brightness: f32,
    previous_damping: f32,
    previous_feedback_amount: f32,

    amplitude_envelope: f32,
    brightness_envelope: f32,
    gain: f32,
    fm_amount: f32,
    carrier_phase: u32,
    modulator_phase: u32,
    previous_sample: f32,

    follower: Follower,
}

impl Default for FmVoice {
    fn default() -> Self {
        Self::new()
    }
}

impl FmVoice {
    pub fn new() -> Self {
        let mut v = Self {
            carrier_frequency: 220.0 / SAMPLE_RATE,
            ratio: 0.5,
            brightness: 0.5,
            damping: 0.5,
            position: 0.5,
            feedback_amount: 0.0,
            previous_carrier_frequency: 0.0,
            previous_modulator_frequency: 0.0,
            previous_brightness: 0.0,
            previous_damping: 0.0,
            previous_feedback_amount: 0.0,
            amplitude_envelope: 0.0,
            brightness_envelope: 0.0,
            gain: 0.0,
            fm_amount: 0.0,
            carrier_phase: 0,
            modulator_phase: 0,
            previous_sample: 0.0,
            follower: Follower::new(),
        };
        v.init();
        v
    }

    /// `Init`.
    pub fn init(&mut self) {
        self.carrier_frequency = 220.0 / SAMPLE_RATE;
        self.ratio = 0.5;
        self.brightness = 0.5;
        self.damping = 0.5;
        self.position = 0.5;
        self.feedback_amount = 0.0;

        self.previous_carrier_frequency = self.carrier_frequency;
        self.previous_modulator_frequency = self.carrier_frequency;
        self.previous_brightness = self.brightness;
        self.previous_damping = self.damping;
        self.previous_feedback_amount = self.feedback_amount;

        self.amplitude_envelope = 0.0;
        self.brightness_envelope = 0.0;
        self.carrier_phase = 0;
        self.modulator_phase = 0;
        self.gain = 0.0;
        self.fm_amount = 0.0;
        self.previous_sample = 0.0;

        self.follower
            .init(8.0 / SAMPLE_RATE, 160.0 / SAMPLE_RATE, 1600.0 / SAMPLE_RATE);
    }

    #[inline]
    pub fn set_frequency(&mut self, frequency: f32) {
        self.carrier_frequency = frequency;
    }
    #[inline]
    pub fn set_ratio(&mut self, ratio: f32) {
        self.ratio = ratio;
    }
    #[inline]
    pub fn set_brightness(&mut self, brightness: f32) {
        self.brightness = brightness;
    }
    #[inline]
    pub fn set_damping(&mut self, damping: f32) {
        self.damping = damping;
    }
    #[inline]
    pub fn set_position(&mut self, position: f32) {
        self.position = position;
    }
    #[inline]
    pub fn set_feedback_amount(&mut self, feedback_amount: f32) {
        self.feedback_amount = feedback_amount;
    }

    /// `TriggerInternalEnvelope`.
    #[inline]
    pub fn trigger_internal_envelope(&mut self) {
        self.amplitude_envelope = 1.0;
        self.brightness_envelope = 1.0;
    }

    /// `Process(in, out, aux, size)`.
    pub fn process(&mut self, input: &[f32], out: &mut [f32], aux: &mut [f32], size: usize) {
        let envelope_amount = if self.damping < 0.9 {
            1.0
        } else {
            (1.0 - self.damping) * 10.0
        };
        let amplitude_rt60 = 0.1 * semitones_to_ratio(self.damping * 96.0) * SAMPLE_RATE;
        let amplitude_decay = 1.0 - libm::powf(0.001, 1.0 / amplitude_rt60);

        let brightness_rt60 = 0.1 * semitones_to_ratio(self.damping * 84.0) * SAMPLE_RATE;
        let brightness_decay = 1.0 - libm::powf(0.001, 1.0 / brightness_rt60);

        let ratio = interpolate(&LUT_FM_FREQUENCY_QUANTIZER, self.ratio, 128.0);
        let mut modulator_frequency = self.carrier_frequency * semitones_to_ratio(ratio);
        if modulator_frequency > 0.5 {
            modulator_frequency = 0.5;
        }

        let feedback = (self.feedback_amount - 0.5) * 2.0;

        let mut carrier_increment = ParameterInterpolator::new(
            &mut self.previous_carrier_frequency,
            self.carrier_frequency,
            size,
        );
        let mut modulator_increment = ParameterInterpolator::new(
            &mut self.previous_modulator_frequency,
            modulator_frequency,
            size,
        );
        let mut brightness =
            ParameterInterpolator::new(&mut self.previous_brightness, self.brightness, size);
        let mut feedback_amount =
            ParameterInterpolator::new(&mut self.previous_feedback_amount, feedback, size);

        let mut carrier_phase = self.carrier_phase;
        let mut modulator_phase = self.modulator_phase;
        let mut previous_sample = self.previous_sample;

        for i in 0..size {
            let (amplitude_envelope, mut brightness_envelope) = self.follower.process(input[i]);
            brightness_envelope *= 2.0 * amplitude_envelope * (2.0 - amplitude_envelope);

            slope(
                &mut self.amplitude_envelope,
                amplitude_envelope,
                0.05,
                amplitude_decay,
            );
            slope(
                &mut self.brightness_envelope,
                brightness_envelope,
                0.01,
                brightness_decay,
            );

            let mut brightness_value = brightness.next();
            brightness_value *= brightness_value;
            let fm_amount_min = if brightness_value < 0.5 {
                0.0
            } else {
                brightness_value * 2.0 - 1.0
            };
            let fm_amount_max = if brightness_value < 0.5 {
                2.0 * brightness_value
            } else {
                1.0
            };
            let fm_envelope = 0.5 + envelope_amount * (self.brightness_envelope - 0.5);
            let fm_amount = (fm_amount_min + fm_amount_max * fm_envelope) * 2.0;
            slew(
                &mut self.fm_amount,
                fm_amount,
                0.005 + fm_amount_max * 0.015,
            );

            let phase_feedback = if feedback < 0.0 {
                0.5 * feedback * feedback
            } else {
                0.0
            };
            modulator_phase = modulator_phase.wrapping_add(f32_to_u32_wrap(
                4_294_967_296.0
                    * modulator_increment.next()
                    * (1.0 + previous_sample * phase_feedback),
            ));
            carrier_phase = carrier_phase
                .wrapping_add(f32_to_u32_wrap(4_294_967_296.0 * carrier_increment.next()));

            let feedback = feedback_amount.next();
            let modulator_fb = if feedback > 0.0 {
                0.25 * feedback * feedback
            } else {
                0.0
            };
            let modulator = sine_fm(modulator_phase, modulator_fb * previous_sample);
            let carrier = sine_fm(carrier_phase, self.fm_amount * modulator);
            one_pole(&mut previous_sample, carrier, 0.1);

            let gain = 1.0 + envelope_amount * (self.amplitude_envelope - 1.0);
            one_pole(&mut self.gain, gain, 0.005 + 0.045 * self.fm_amount);

            out[i] = (carrier + 0.5 * modulator) * self.gain;
            aux[i] = 0.5 * modulator * self.gain;
        }

        self.carrier_phase = carrier_phase;
        self.modulator_phase = modulator_phase;
        self.previous_sample = previous_sample;
    }
}
