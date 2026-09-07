//! `elements/dsp/voice.{h,cc}` -- one modal-synthesis voice: it configures and
//! sums the three exciters (bow / blow / strike) plus the tube and diffuser,
//! then drives either the [`Resonator`] (MODAL) or a bank of [`String`]s
//! (STRING / STRINGS).

use stmlib::constrain;
use stmlib::filter::DcBlocker;
use stmlib::units::semitones_to_ratio;

use crate::dsp::{MAX_BLOCK_SIZE, SAMPLE_RATE};
use crate::exciter::{Exciter, ExciterModel, FLAG_FALLING_EDGE, FLAG_GATE, FLAG_RISING_EDGE};
use crate::fx::Diffuser;
use crate::multistage_envelope::MultistageEnvelope;
use crate::part::Patch;
use crate::resonator::Resonator;
use crate::resources::{LUT_ACCENT_GAIN_COARSE, LUT_ACCENT_GAIN_FINE};
use crate::string::String;
use crate::tube::Tube;

pub const NUM_STRINGS: usize = 5;

/// `ResonatorModel` -- which physical model the voice's body uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResonatorModel {
    Modal,
    String,
    Strings,
}

#[rustfmt::skip]
const CHORDS: [[f32; 5]; 11] = [
    [0.0, -12.0, 0.0, 0.01, 12.0],
    [0.0, -12.0, 3.0, 7.0,  10.0],
    [0.0, -12.0, 3.0, 7.0,  12.0],
    [0.0, -12.0, 3.0, 7.0,  14.0],
    [0.0, -12.0, 3.0, 7.0,  17.0],
    [0.0, -12.0, 7.0, 12.0, 19.0],
    [0.0, -12.0, 4.0, 7.0,  17.0],
    [0.0, -12.0, 4.0, 7.0,  14.0],
    [0.0, -12.0, 4.0, 7.0,  12.0],
    [0.0, -12.0, 4.0, 7.0,  11.0],
    [0.0, -12.0, 5.0, 7.0,  12.0],
];

/// `elements::Voice`.
pub struct Voice {
    envelope: MultistageEnvelope,
    tube: Tube,
    bow: Exciter,
    blow: Exciter,
    strike: Exciter,
    diffuser: Diffuser,
    resonator: Resonator,
    string: [String; NUM_STRINGS],
    dc_blocker: DcBlocker,

    strength: f32,
    envelope_value: f32,
    exciter_level: f32,

    bow_buffer: [f32; MAX_BLOCK_SIZE],
    bow_strength_buffer: [f32; MAX_BLOCK_SIZE],
    blow_buffer: [f32; MAX_BLOCK_SIZE],
    strike_buffer: [f32; MAX_BLOCK_SIZE],

    previous_gate: bool,
    resonator_model: ResonatorModel,
    chord_index: f32,
}

impl Default for Voice {
    fn default() -> Self {
        Self::new()
    }
}

impl Voice {
    pub fn new() -> Self {
        let mut v = Self {
            envelope: MultistageEnvelope::new(),
            tube: Tube::new(),
            bow: Exciter::new(),
            blow: Exciter::new(),
            strike: Exciter::new(),
            diffuser: Diffuser::new(),
            resonator: Resonator::new(),
            string: [
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                String::new(),
            ],
            dc_blocker: DcBlocker::default(),
            strength: 0.0,
            envelope_value: 0.0,
            exciter_level: 0.0,
            bow_buffer: [0.0; MAX_BLOCK_SIZE],
            bow_strength_buffer: [0.0; MAX_BLOCK_SIZE],
            blow_buffer: [0.0; MAX_BLOCK_SIZE],
            strike_buffer: [0.0; MAX_BLOCK_SIZE],
            previous_gate: false,
            resonator_model: ResonatorModel::Modal,
            chord_index: 0.0,
        };
        v.init();
        v
    }

    /// `Init`.
    pub fn init(&mut self) {
        self.envelope.init();
        self.bow.init();
        self.blow.init();
        self.strike.init();
        self.diffuser.init();

        self.reset_resonator();

        self.bow.set_model(ExciterModel::Flow);
        self.bow.set_parameter(0.7);
        self.bow.set_timbre(0.5);

        self.blow.set_model(ExciterModel::GranularSamplePlayer);

        self.envelope.set_adsr(0.5, 0.5, 0.5, 0.5);

        self.previous_gate = false;
        self.strength = 0.0;
        self.exciter_level = 0.0;
        self.envelope_value = 0.0;
        self.chord_index = 0.0;

        self.resonator_model = ResonatorModel::Modal;
    }

    fn reset_resonator(&mut self) {
        self.resonator.init();
        for s in self.string.iter_mut() {
            s.init(true);
        }
        self.dc_blocker.init(1.0 - 10.0 / SAMPLE_RATE);
        self.resonator.set_resolution(52); // Runs with 56 extremely tightly.
    }

    /// For metering.
    #[inline]
    pub fn exciter_level(&self) -> f32 {
        self.exciter_level
    }

    /// `Panic` -- zero the resonator state (called if the output blows up).
    pub fn panic(&mut self) {
        self.reset_resonator();
    }

    #[inline]
    pub fn set_resonator_model(&mut self, resonator_model: ResonatorModel) {
        self.resonator_model = resonator_model;
    }

    fn gate_flags(&mut self, gate_in: bool) -> u8 {
        let mut flags = 0;
        if gate_in {
            if !self.previous_gate {
                flags |= FLAG_RISING_EDGE;
            }
            flags |= FLAG_GATE;
        } else if self.previous_gate {
            flags = FLAG_FALLING_EDGE;
        }
        self.previous_gate = gate_in;
        flags
    }

    /// `Process`.
    pub fn process(
        &mut self,
        patch: &Patch,
        frequency: f32,
        mut strength: f32,
        gate_in: bool,
        blow_in: &[f32],
        strike_in: &[f32],
        raw: &mut [f32],
        center: &mut [f32],
        sides: &mut [f32],
        size: usize,
    ) {
        let flags = self.gate_flags(gate_in);

        // Compute the envelope.
        let mut envelope_gain = 1.0;
        if patch.exciter_envelope_shape < 0.4 {
            let a = patch.exciter_envelope_shape * 0.75 + 0.15;
            let dr = a * 1.8;
            self.envelope.set_adsr(a, dr, 0.0, dr);
            envelope_gain = 5.0 - patch.exciter_envelope_shape * 10.0;
        } else if patch.exciter_envelope_shape < 0.6 {
            let s = (patch.exciter_envelope_shape - 0.4) * 5.0;
            self.envelope.set_adsr(0.45, 0.81, s, 0.81);
        } else {
            let a = (1.0 - patch.exciter_envelope_shape) * 0.75 + 0.15;
            let dr = a * 1.8;
            self.envelope.set_adsr(a, dr, 1.0, dr);
        }
        let envelope_value = self.envelope.process(flags) * envelope_gain;
        let envelope_increment = (envelope_value - self.envelope_value) / size as f32;

        // Configure and evaluate exciters.
        let brightness_factor = 0.4 + 0.6 * patch.resonator_brightness;
        self.bow
            .set_timbre(patch.exciter_bow_timbre * brightness_factor);

        self.blow.set_parameter(patch.exciter_blow_meta);
        self.blow.set_timbre(patch.exciter_blow_timbre);
        self.blow.set_signature(patch.exciter_signature);

        let strike_meta = patch.exciter_strike_meta;
        self.strike.set_meta(
            if strike_meta <= 0.4 {
                strike_meta * 0.625
            } else {
                strike_meta * 1.25 - 0.25
            },
            ExciterModel::SamplePlayer,
            ExciterModel::Particles,
        );
        self.strike.set_timbre(patch.exciter_strike_timbre);
        self.strike.set_signature(patch.exciter_signature);

        self.bow.process(flags, &mut self.bow_buffer[..size]);

        let mut blow_level = patch.exciter_blow_level * 1.5;
        let tube_level = if blow_level > 1.0 {
            (blow_level - 1.0) * 2.0
        } else {
            0.0
        };
        blow_level = if blow_level < 1.0 {
            blow_level * 0.4
        } else {
            0.4
        };
        self.blow.process(flags, &mut self.blow_buffer[..size]);
        self.tube.process(
            frequency,
            envelope_value,
            patch.resonator_damping,
            tube_level,
            &mut self.blow_buffer[..size],
            tube_level * 0.5,
        );

        for i in 0..size {
            self.blow_buffer[i] = self.blow_buffer[i] * blow_level + blow_in[i];
        }
        self.diffuser.process(&mut self.blow_buffer[..size]);
        self.strike.process(flags, &mut self.strike_buffer[..size]);

        // Past a certain point, raising the Strike level stops increasing the
        // exciter amplitude and instead bleeds the raw signal into the output.
        let mut strike_level = patch.exciter_strike_level * 1.25;
        let strike_bleed = if strike_level > 1.0 {
            (strike_level - 1.0) * 2.0
        } else {
            0.0
        };
        strike_level = if strike_level < 1.0 {
            strike_level
        } else {
            1.0
        };
        strike_level *= 1.5;

        // The strength parameter is very sensitive to zipper noise.
        strength *= 256.0;
        let strength_increment = (strength - self.strength) / size as f32;

        // Sum all sources of excitation.
        for i in 0..size {
            self.strength += strength_increment;
            self.envelope_value += envelope_increment;
            let mut input_sample = 0.0;
            let mut e = self.envelope_value;
            let strength_lut = self.strength;
            let strength_lut_integral = strength_lut as i32;
            let strength_lut_fractional = strength_lut - strength_lut_integral as f32;
            let accent = LUT_ACCENT_GAIN_COARSE[strength_lut_integral as usize]
                * LUT_ACCENT_GAIN_FINE[(256.0 * strength_lut_fractional) as i32 as usize];
            self.bow_strength_buffer[i] = e * patch.exciter_bow_level;

            self.strike_buffer[i] *= accent;
            e *= accent;

            input_sample += self.bow_buffer[i] * self.bow_strength_buffer[i] * 0.125 * accent;
            input_sample += self.blow_buffer[i] * e;
            input_sample += self.strike_buffer[i] * strike_level;
            input_sample += strike_in[i];
            raw[i] = input_sample * 0.5;
        }

        // Update meter for exciter.
        for i in 0..size {
            let error = raw[i] * raw[i] - self.exciter_level;
            self.exciter_level += error * if error > 0.0 { 0.5 } else { 0.001 };
        }

        // Some exciters can cause palm mutes on release.
        let mut damping = patch.resonator_damping;
        damping -= self.strike.damping() * strike_level * 0.125;
        damping -= (1.0 - self.bow_strength_buffer[0]) * patch.exciter_bow_level * 0.0625;
        if damping <= 0.0 {
            damping = 0.0;
        }

        if self.resonator_model == ResonatorModel::Modal {
            self.resonator.set_frequency(frequency);
            self.resonator.set_geometry(patch.resonator_geometry);
            self.resonator.set_brightness(patch.resonator_brightness);
            self.resonator.set_position(patch.resonator_position);
            self.resonator.set_damping(damping);
            self.resonator
                .set_modulation_frequency(patch.resonator_modulation_frequency);
            self.resonator
                .set_modulation_offset(patch.resonator_modulation_offset);

            self.resonator
                .process(&self.bow_strength_buffer[..size], raw, center, sides, size);
        } else {
            let num_notes = if self.resonator_model == ResonatorModel::String {
                1
            } else {
                NUM_STRINGS
            };

            let normalization = 1.0 / num_notes as f32;
            self.dc_blocker.process(&mut raw[..size]);
            for r in raw[..size].iter_mut() {
                *r *= normalization;
            }

            let chord = patch.resonator_geometry * 10.0;
            let hysteresis = if chord > self.chord_index { -0.1 } else { 0.1 };
            let chord_index = constrain((chord + hysteresis + 0.5) as i32, 0, 10);
            self.chord_index = chord_index as f32;

            center[..size].fill(0.0);
            sides[..size].fill(0.0);
            for i in 0..num_notes {
                let transpose = CHORDS[chord_index as usize][i];
                self.string[i].set_frequency(frequency * semitones_to_ratio(transpose));
                self.string[i].set_brightness(patch.resonator_brightness);
                self.string[i].set_position(patch.resonator_position);
                self.string[i].set_damping(damping);
                if num_notes == 1 {
                    self.string[i].set_dispersion(patch.resonator_geometry);
                } else {
                    let b = patch.resonator_brightness;
                    self.string[i].set_dispersion(if b < 0.5 { 0.0 } else { (b - 0.5) * -0.4 });
                }
                self.string[i].process(&raw[..size], center, sides, size);
            }
            for i in 0..size {
                let left = center[i];
                let right = sides[i];
                center[i] = left - right;
                sides[i] = left + right;
            }
        }

        // This is where the raw mallet signal bleeds through the exciter output.
        for i in 0..size {
            center[i] += strike_bleed * self.strike_buffer[i];
        }
    }
}
