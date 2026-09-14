//! `yarns/voice.{h,cc}` -- `Voice`: one analog voice's pitch (with
//! portamento and vibrato), gate, trigger envelope, and calibrated CV/DAC
//! code generation.

use stmlib::fixed::{interpolate_824_i16, interpolate_824_u16};

use crate::oscillator::Oscillator;
use crate::resources::{LUT_LFO_INCREMENTS, LUT_PORTAMENTO_INCREMENTS, WAVEFORM_TABLE};

pub const NUM_OCTAVES: usize = 11;

const OCTAVE: i32 = 12 << 7;
const MAX_NOTE: i32 = 120 << 7;

const CC_MODULATION_WHEEL_MSB: u8 = 0x01;
const CC_BREATH_CONTROLLER: u8 = 0x02;
const CC_FOOT_PEDAL_MSB: u8 = 0x04;

/// `TriggerShape` -- these numbering match the C enum exactly (unlike
/// `oscillator::audio_mode`): `trigger_dac_code` uses them directly, with no
/// out-of-scope `settings.cc` translation in between.
pub mod trigger_shape {
    pub const SQUARE: u8 = 0;
    pub const LINEAR: u8 = 1;
    pub const EXPONENTIAL: u8 = 2;
    pub const RING: u8 = 3;
    pub const STEPS: u8 = 4;
    pub const NOISE_BURST: u8 = 5;
}

#[derive(Debug, Clone)]
pub struct Voice {
    note_source: i32,
    note_target: i32,
    note_portamento: i32,
    note: i32,
    tuning: i32,
    gate: bool,

    dirty: bool,
    note_dac_code: u16,
    calibrated_dac_code: [u16; NUM_OCTAVES],

    mod_pitch_bend: i16,
    mod_wheel: u8,
    mod_aux: [u16; 8],
    mod_velocity: u8,

    pitch_bend_range: u8,
    modulation_rate: u8,
    vibrato_range: u8,

    trigger_duration: u8,
    trigger_shape: u8,
    trigger_scale: bool,
    aux_cv_source: usize,
    aux_cv_source_2: usize,

    lfo_phase: u32,
    portamento_phase: u32,
    portamento_phase_increment: u32,
    portamento_exponential_shape: bool,

    // This counter is used to artificially create a 500us dip at LOW level
    // when the gate is currently HIGH and a new note arrives with a
    // retrigger command. This happens with note-stealing, or when sending a
    // MIDI sequence with overlapping notes.
    retrigger_delay: u16,

    trigger_pulse: u16,
    trigger_phase_increment: u32,
    trigger_phase: u32,

    // PLL for clock-synced LFO.
    lfo_pll_phase_increment: u32,
    lfo_pll_previous_target_phase: u32,
    lfo_pll_previous_phase: u32,

    audio_mode: u8,
    oscillator: Oscillator,
}

impl Default for Voice {
    fn default() -> Self {
        Self {
            note_source: 0,
            note_target: 0,
            note_portamento: 0,
            note: -1,
            tuning: 0,
            gate: false,
            dirty: false,
            note_dac_code: 0,
            calibrated_dac_code: [0; NUM_OCTAVES],
            mod_pitch_bend: 0,
            mod_wheel: 0,
            mod_aux: [0; 8],
            mod_velocity: 0,
            pitch_bend_range: 2,
            modulation_rate: 0,
            vibrato_range: 0,
            trigger_duration: 2,
            trigger_shape: 0,
            trigger_scale: false,
            aux_cv_source: 0,
            aux_cv_source_2: 0,
            lfo_phase: 0,
            portamento_phase: 0,
            portamento_phase_increment: 1u32 << 31,
            portamento_exponential_shape: false,
            retrigger_delay: 0,
            trigger_pulse: 0,
            trigger_phase_increment: 0,
            trigger_phase: 0,
            lfo_pll_phase_increment: 0,
            lfo_pll_previous_target_phase: 0,
            lfo_pll_previous_phase: 0,
            audio_mode: 0,
            oscillator: Oscillator::default(),
        }
    }
}

impl Voice {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn init(&mut self, reset_calibration: bool) {
        self.note = -1;
        self.note_source = 60 << 7;
        self.note_target = self.note_source;
        self.note_portamento = self.note_source;
        self.gate = false;

        self.mod_velocity = 0;
        self.reset_all_controllers();

        self.modulation_rate = 0;
        self.pitch_bend_range = 2;
        self.vibrato_range = 0;

        self.lfo_phase = 0;
        self.portamento_phase = 0;
        self.portamento_phase_increment = 1u32 << 31;
        self.portamento_exponential_shape = false;

        self.trigger_duration = 2;

        if reset_calibration {
            for (i, slot) in self.calibrated_dac_code.iter_mut().enumerate() {
                *slot = (54586 - 5133 * i as i32) as u16;
            }
        }
        self.dirty = false;
        self.oscillator.init(
            self.calibrated_dac_code[3] as i32 - self.calibrated_dac_code[8] as i32,
            self.calibrated_dac_code[3] as i32,
        );
    }

    pub fn calibrate(&mut self, calibrated_dac_code: &[u16; NUM_OCTAVES]) {
        self.calibrated_dac_code = *calibrated_dac_code;
    }

    fn note_to_dac_code(&self, note: i32) -> u16 {
        let mut note = note.max(0);
        if note >= MAX_NOTE {
            note = MAX_NOTE - 1;
        }
        let mut octave = 0usize;
        while note >= OCTAVE {
            note -= OCTAVE;
            octave += 1;
        }

        // `note` is now between 0 and `OCTAVE`; `octave` indicates the
        // octave. Look up in the DAC code table.
        let a = self.calibrated_dac_code[octave] as i32;
        let b = self.calibrated_dac_code[octave + 1] as i32;
        (a + ((b - a) * note / OCTAVE)) as u16
    }

    pub fn reset_all_controllers(&mut self) {
        self.mod_pitch_bend = 8192;
        self.mod_wheel = 0;
        self.mod_aux = [0; 8];
    }

    pub fn refresh(&mut self) {
        // Compute base pitch with portamento.
        self.portamento_phase = self.portamento_phase.wrapping_add(self.portamento_phase_increment);
        if self.portamento_phase < self.portamento_phase_increment {
            self.portamento_phase = 0;
            self.portamento_phase_increment = 0;
            self.note_source = self.note_target;
        }
        let portamento_level = if self.portamento_exponential_shape {
            interpolate_824_u16(&crate::resources::LUT_ENV_EXPO, self.portamento_phase)
        } else {
            (self.portamento_phase >> 16) as u16
        };
        let note = self.note_source
            + (((self.note_target - self.note_source) * portamento_level as i32) >> 16);

        self.note_portamento = note;
        let mut note = note;

        // Add pitch-bend.
        note += ((self.mod_pitch_bend as i32 - 8192) * self.pitch_bend_range as i32) >> 6;

        // Add transposition/fine tuning.
        note += self.tuning;

        // Add vibrato.
        if self.modulation_rate < 100 {
            self.lfo_phase = self
                .lfo_phase
                .wrapping_add(LUT_LFO_INCREMENTS[self.modulation_rate as usize]);
        } else {
            self.lfo_phase = self.lfo_phase.wrapping_add(self.lfo_pll_phase_increment);
        }
        let lfo: i32 = if self.lfo_phase < 1u32 << 31 {
            -32768 + (self.lfo_phase >> 15) as i32
        } else {
            0x17fff - (self.lfo_phase >> 15) as i32
        };
        note += (lfo * self.mod_wheel as i32 * self.vibrato_range as i32) >> 15;
        self.mod_aux[0] = (self.mod_velocity as u16) << 9;
        self.mod_aux[1] = (self.mod_wheel as u16) << 9;
        self.mod_aux[5] = (self.mod_pitch_bend as u16) << 2;
        self.mod_aux[6] = (((lfo * self.mod_wheel as i32) >> 7) + 32768) as u16;
        self.mod_aux[7] = (lfo + 32768) as u16;

        if self.retrigger_delay != 0 {
            self.retrigger_delay -= 1;
        }

        if self.trigger_pulse != 0 {
            self.trigger_pulse -= 1;
        }

        if self.trigger_phase_increment != 0 {
            self.trigger_phase = self.trigger_phase.wrapping_add(self.trigger_phase_increment);
            if self.trigger_phase < self.trigger_phase_increment {
                self.trigger_phase = 0;
                self.trigger_phase_increment = 0;
            }
        }
        if note != self.note || self.dirty {
            self.note_dac_code = self.note_to_dac_code(note);
            self.note = note;
            self.dirty = false;
        }
    }

    pub fn note_on(&mut self, note: i16, velocity: u8, portamento: u8, trigger: bool) {
        self.note_source = self.note_portamento;
        self.note_target = note as i32;
        if portamento == 0 {
            self.note_source = self.note_target;
        }
        self.portamento_phase = 0;
        if portamento <= 50 {
            self.portamento_phase_increment = LUT_PORTAMENTO_INCREMENTS[(portamento as usize) << 1];
            self.portamento_exponential_shape = true;
        } else {
            let base_increment = LUT_PORTAMENTO_INCREMENTS[(portamento as usize - 51) << 1];
            let delta = (self.note_target - self.note_source).unsigned_abs() + 1;
            // 32-bit unsigned arithmetic throughout, matching the C exactly
            // (including the wraparound if `1536 * (base_increment >> 11)`
            // overflows `u32` -- widening to `u64` here would silently
            // "fix" that instead of reproducing it).
            let phase_increment = (1536u32.wrapping_mul(base_increment >> 11) / delta) << 11;
            self.portamento_phase_increment = phase_increment.clamp(1, 2147483647);
            self.portamento_exponential_shape = false;
        }

        self.mod_velocity = velocity;

        if self.gate && trigger {
            self.retrigger_delay = 2;
        }
        if trigger {
            self.trigger_pulse = self.trigger_duration as u16 * 8;
            self.trigger_phase = 0;
            self.trigger_phase_increment = LUT_PORTAMENTO_INCREMENTS[self.trigger_duration as usize];
        }
        self.gate = true;
    }

    pub fn note_off(&mut self) {
        self.gate = false;
    }

    pub fn control_change(&mut self, controller: u8, value: u8) {
        match controller {
            CC_MODULATION_WHEEL_MSB => self.mod_wheel = value,
            CC_BREATH_CONTROLLER => self.mod_aux[3] = (value as u16) << 9,
            CC_FOOT_PEDAL_MSB => self.mod_aux[4] = (value as u16) << 9,
            _ => {}
        }
    }

    pub fn pitch_bend(&mut self, pitch_bend: u16) {
        self.mod_pitch_bend = pitch_bend as i16;
    }

    pub fn aftertouch(&mut self, velocity: u8) {
        self.mod_aux[2] = (velocity as u16) << 9;
    }

    pub fn set_modulation_rate(&mut self, modulation_rate: u8) {
        self.modulation_rate = modulation_rate;
    }

    pub fn set_pitch_bend_range(&mut self, pitch_bend_range: u8) {
        self.pitch_bend_range = pitch_bend_range;
    }

    pub fn set_vibrato_range(&mut self, vibrato_range: u8) {
        self.vibrato_range = vibrato_range;
    }

    pub fn set_trigger_duration(&mut self, trigger_duration: u8) {
        self.trigger_duration = trigger_duration;
    }

    pub fn set_trigger_scale(&mut self, trigger_scale: bool) {
        self.trigger_scale = trigger_scale;
    }

    pub fn set_trigger_shape(&mut self, trigger_shape: u8) {
        self.trigger_shape = trigger_shape;
    }

    pub fn set_aux_cv(&mut self, aux_cv_source: usize) {
        self.aux_cv_source = aux_cv_source;
    }

    pub fn set_aux_cv_2(&mut self, aux_cv_source_2: usize) {
        self.aux_cv_source_2 = aux_cv_source_2;
    }

    pub fn note(&self) -> i32 {
        self.note
    }

    pub fn velocity(&self) -> u8 {
        self.mod_velocity
    }

    pub fn modulation(&self) -> u8 {
        self.mod_wheel
    }

    pub fn aux_cv(&self) -> u8 {
        (self.mod_aux[self.aux_cv_source] >> 8) as u8
    }

    pub fn aux_cv_2(&self) -> u8 {
        (self.mod_aux[self.aux_cv_source_2] >> 8) as u8
    }

    pub fn dac_code_from_16_bit_value(&self, value: u16) -> u16 {
        let v = value as u32;
        // The C computes `calibrated_dac_code_[3] - calibrated_dac_code_[8]`
        // as `int` (both operands promote from `uint16_t`), then assigns
        // that possibly-negative result to a `uint32_t scale` -- a 2's
        // complement reinterpretation, not a 16-bit wrapping subtraction.
        let scale = (self.calibrated_dac_code[3] as i32 - self.calibrated_dac_code[8] as i32) as u32;
        (self.calibrated_dac_code[3] as u32).wrapping_sub(scale.wrapping_mul(v) >> 16) as u16
    }

    pub fn note_dac_code(&self) -> u16 {
        self.note_dac_code
    }

    pub fn velocity_dac_code(&self) -> u16 {
        self.dac_code_from_16_bit_value((self.mod_velocity as u16) << 9)
    }

    pub fn modulation_dac_code(&self) -> u16 {
        self.dac_code_from_16_bit_value((self.mod_wheel as u16) << 9)
    }

    pub fn aux_cv_dac_code(&self) -> u16 {
        self.dac_code_from_16_bit_value(self.mod_aux[self.aux_cv_source])
    }

    pub fn aux_cv_dac_code_2(&self) -> u16 {
        self.dac_code_from_16_bit_value(self.mod_aux[self.aux_cv_source_2])
    }

    pub fn gate_on(&self) -> bool {
        self.gate
    }

    pub fn gate(&self) -> bool {
        self.gate && self.retrigger_delay == 0
    }

    pub fn trigger(&self) -> bool {
        self.gate && self.trigger_pulse != 0
    }

    pub fn trigger_dac_code(&self) -> u16 {
        if self.trigger_phase <= self.trigger_phase_increment {
            self.calibrated_dac_code[3] // 0V.
        } else {
            let velocity_coefficient = if self.trigger_scale {
                (self.mod_velocity as i32) << 8
            } else {
                32768
            };
            let mut value: i32 = match self.trigger_shape {
                trigger_shape::SQUARE => 32767,
                trigger_shape::LINEAR => 32767 - (self.trigger_phase >> 17) as i32,
                _ => {
                    let table = WAVEFORM_TABLE[(self.trigger_shape - trigger_shape::EXPONENTIAL) as usize];
                    interpolate_824_i16(table, self.trigger_phase) as i32
                }
            };
            value = (value * velocity_coefficient) >> 15;
            let max = self.calibrated_dac_code[8] as i32;
            let min = self.calibrated_dac_code[3] as i32;
            (min + (((max - min) * value) >> 15)) as u16
        }
    }

    pub fn calibration_dac_code(&self, note: usize) -> u16 {
        self.calibrated_dac_code[note]
    }

    pub fn set_calibration_dac_code(&mut self, note: usize, dac_code: u16) {
        self.calibrated_dac_code[note] = dac_code;
        self.dirty = true;
    }

    pub fn set_audio_mode(&mut self, audio_mode: u8) {
        self.audio_mode = audio_mode;
    }

    pub fn set_tuning(&mut self, coarse: i8, fine: i8) {
        self.tuning = ((coarse as i32) << 7) + fine as i32;
    }

    pub fn audio_mode(&self) -> u8 {
        self.audio_mode
    }

    pub fn render_audio(&mut self) {
        let audio_mode = self.audio_mode;
        let note = self.note as i16;
        let gate = self.gate;
        self.oscillator.render(audio_mode, note, gate);
    }

    pub fn read_sample(&mut self) -> u16 {
        self.oscillator.read_sample()
    }

    pub fn tap_lfo(&mut self, target_phase: u32) {
        let target_increment = target_phase.wrapping_sub(self.lfo_pll_previous_target_phase);

        let d_error = target_increment
            .wrapping_sub(self.lfo_phase.wrapping_sub(self.lfo_pll_previous_phase)) as i32;
        let p_error = target_phase.wrapping_sub(self.lfo_phase) as i32;
        let error = d_error.wrapping_add(p_error >> 1);

        self.lfo_pll_phase_increment = self.lfo_pll_phase_increment.wrapping_add((error >> 11) as u32);

        self.lfo_pll_previous_phase = self.lfo_phase;
        self.lfo_pll_previous_target_phase = target_phase;
    }
}
