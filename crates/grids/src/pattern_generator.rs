//! `grids/pattern_generator.{cc,h}` -- `PatternGenerator`: the topographic
//! drum-map sequencer and Euclidean-rhythm generator.
//!
//! ```text
//! OUTPUT MODE  OUTPUT CLOCK  BIT7  BIT6  BIT5  BIT4  BIT3  BIT2  BIT1  BIT0
//! DRUMS        FALSE          RND   CLK  HHAC  SDAC  BDAC    HH    SD    BD
//! DRUMS        TRUE           RND   CLK   CLK   BAR   ACC    HH    SD    BD
//! EUCLIDEAN    FALSE          RND   CLK  RST3  RST2  RST1  EUC3  EUC2  EUC1
//! EUCLIDEAN    TRUE           RND   CLK   CLK  STEP   RST  EUC3  EUC2  EUC1
//! ```
//!
//! Deviations from the C++ (beyond "modernise structure only"):
//! - Every member of `PatternGenerator`/`Options`/etc. is `static` in the
//!   C++ (one hardwired global instance, the usual AVR-firmware idiom).
//!   Ported as a normal instance -- `PatternGenerator::new()` +ordinary
//!   `&mut self` methods -- matching every other crate in this workspace.
//! - `LoadSettings`/`SaveSettings` (raw AVR EEPROM reads/writes) and the
//!   `factory_testing_` counter they maintain are dropped entirely --
//!   out of scope, like every other crate's persistence layer. `init()`
//!   just resets state; a host configures `Options`/`PatternGeneratorSettings`
//!   however it obtains them (its own storage, `Options::unpack` on a byte
//!   from wherever, or setting fields directly).
//! - `Options::Options` is a C `union` of `DrumsSettings` and
//!   `[u8; kNumParts]` (euclidean lengths), aliased in memory to save the
//!   3-4 bytes that matters on an AVR but not here. Ported as a plain
//!   struct with both fields present (`drums: DrumsSettings`,
//!   `euclidean_length: [u8; NUM_PARTS]`) rather than an actual union,
//!   avoiding `unsafe`/C-union aliasing rules for a memory saving that
//!   doesn't matter on a host.

use crate::random::Lfsr;
use crate::resources::{
    LUT_RES_EUCLIDEAN, NODE_0, NODE_1, NODE_2, NODE_3, NODE_4, NODE_5, NODE_6, NODE_7, NODE_8,
    NODE_9, NODE_10, NODE_11, NODE_12, NODE_13, NODE_14, NODE_15, NODE_16, NODE_17, NODE_18,
    NODE_19, NODE_20, NODE_21, NODE_22, NODE_23, NODE_24,
};

pub const NUM_PARTS: usize = 3;
pub const PULSES_PER_STEP: u8 = 3; // 24 ppqn; 8 steps per quarter note.
pub const STEPS_PER_PATTERN: u8 = 32;
pub const PULSE_DURATION: u8 = 8; // 8 ticks of the main clock.

/// `LedBits` -- only the 3 instrument bits `led_pattern` uses; `LED_CLOCK`/
/// `LED_ALL`/the AVR GPIO typedefs in `hardware_config.h` are hardware
/// wiring, out of scope.
pub mod led_bits {
    pub const BD: u8 = 8;
    pub const SD: u8 = 4;
    pub const HH: u8 = 2;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum OutputBits {
    Common = 0x08,
    Clock = 0x10,
    Reset = 0x20,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputMode {
    #[default]
    Euclidean,
    Drums,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum ClockResolution {
    #[default]
    Ppqn4,
    Ppqn8,
    Ppqn24,
}

fn clock_resolution_from_u8(value: u8) -> ClockResolution {
    match value.min(2) {
        0 => ClockResolution::Ppqn4,
        1 => ClockResolution::Ppqn8,
        _ => ClockResolution::Ppqn24,
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct DrumsSettings {
    pub x: u8,
    pub y: u8,
    pub randomness: u8,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct PatternGeneratorOptions {
    pub drums: DrumsSettings,
    pub euclidean_length: [u8; NUM_PARTS],
}

#[derive(Debug, Clone, Copy, Default)]
pub struct PatternGeneratorSettings {
    pub options: PatternGeneratorOptions,
    pub density: [u8; NUM_PARTS],
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Options {
    pub clock_resolution: ClockResolution,
    pub output_mode: OutputMode,
    pub output_clock: bool,
    pub tap_tempo: bool,
    pub gate_mode: bool,
    pub swing: bool,
}

impl Options {
    pub fn pack(&self) -> u8 {
        let mut byte = self.clock_resolution as u8;
        if !self.swing {
            byte |= 0x08;
        }
        if self.tap_tempo {
            byte |= 0x10;
        }
        if self.output_clock {
            byte |= 0x20;
        }
        if self.output_mode == OutputMode::Drums {
            byte |= 0x40;
        }
        if !self.gate_mode {
            byte |= 0x80;
        }
        byte
    }

    pub fn unpack(&mut self, byte: u8) {
        self.tap_tempo = byte & 0x10 != 0;
        self.output_clock = byte & 0x20 != 0;
        self.output_mode = if byte & 0x40 != 0 { OutputMode::Drums } else { OutputMode::Euclidean };
        self.gate_mode = byte & 0x80 == 0;
        self.swing = byte & 0x08 == 0;
        self.clock_resolution = clock_resolution_from_u8(byte & 0x7);
    }
}

/// `U8Mix(a, b, balance)` from `avrlib/op.h`'s portable (non-AVR-asm) path.
#[inline]
fn u8_mix(a: u8, b: u8, balance: u8) -> u8 {
    (((a as u16) * (255 - balance) as u16 + (b as u16) * balance as u16) >> 8) as u8
}

/// `U8U8MulShift8(a, b)`.
#[inline]
fn u8_u8_mul_shift8(a: u8, b: u8) -> u8 {
    (((a as u16) * (b as u16)) >> 8) as u8
}

/// `drum_map[5][5]` from `pattern_generator.cc` -- a hand-arranged, *not*
/// sequential, layout of the 25 `node_N` tables `resources.rs` transpiled.
const DRUM_MAP: [[&[u8]; 5]; 5] = [
    [&NODE_10, &NODE_8, &NODE_0, &NODE_9, &NODE_11],
    [&NODE_15, &NODE_7, &NODE_13, &NODE_12, &NODE_6],
    [&NODE_18, &NODE_14, &NODE_4, &NODE_5, &NODE_3],
    [&NODE_23, &NODE_16, &NODE_21, &NODE_1, &NODE_2],
    [&NODE_24, &NODE_19, &NODE_17, &NODE_20, &NODE_22],
];

#[derive(Debug, Clone)]
pub struct PatternGenerator {
    options: Options,

    pulse: u8,
    step: u8,
    euclidean_step: [u8; NUM_PARTS],
    first_beat: bool,
    beat: bool,

    state: u8,
    part_perturbation: [u8; NUM_PARTS],

    pulse_duration_counter: u8,

    settings: PatternGeneratorSettings,

    rng: Lfsr,
}

impl Default for PatternGenerator {
    fn default() -> Self {
        Self {
            options: Options::default(),
            pulse: 0,
            step: 0,
            euclidean_step: [0; NUM_PARTS],
            first_beat: false,
            beat: false,
            state: 0,
            part_perturbation: [0; NUM_PARTS],
            pulse_duration_counter: 0,
            settings: PatternGeneratorSettings::default(),
            rng: Lfsr::default(),
        }
    }
}

impl PatternGenerator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn init(&mut self) {
        self.reset();
    }

    pub fn reset(&mut self) {
        self.step = 0;
        self.pulse = 0;
        self.euclidean_step = [0; NUM_PARTS];
    }

    pub fn retrigger(&mut self) {
        self.evaluate();
    }

    pub fn tick_clock(&mut self, num_pulses: u8) {
        self.evaluate();
        self.beat = self.step & 0x7 == 0;
        self.first_beat = self.step == 0;

        self.pulse += num_pulses;

        // Wrap into ppqn steps.
        while self.pulse >= PULSES_PER_STEP {
            self.pulse -= PULSES_PER_STEP;
            if self.step & 1 == 0 {
                for s in self.euclidean_step.iter_mut() {
                    *s += 1;
                }
            }
            self.step += 1;
        }

        // Wrap into step sequence steps.
        if self.step >= STEPS_PER_PATTERN {
            self.step -= STEPS_PER_PATTERN;
        }
    }

    pub fn state(&self) -> u8 {
        self.state
    }
    pub fn step(&self) -> u8 {
        self.step
    }

    pub fn swing(&self) -> bool {
        self.options.swing
    }
    pub fn output_clock(&self) -> bool {
        self.options.output_clock
    }
    pub fn tap_tempo(&self) -> bool {
        self.options.tap_tempo
    }
    pub fn gate_mode(&self) -> bool {
        self.options.gate_mode
    }
    pub fn output_mode(&self) -> OutputMode {
        self.options.output_mode
    }
    pub fn clock_resolution(&self) -> ClockResolution {
        self.options.clock_resolution
    }

    pub fn options(&self) -> &Options {
        &self.options
    }
    pub fn set_options(&mut self, options: Options) {
        self.options = options;
    }

    pub fn set_swing(&mut self, value: bool) {
        self.options.swing = value;
    }
    pub fn set_output_clock(&mut self, value: bool) {
        self.options.output_clock = value;
    }
    pub fn set_tap_tempo(&mut self, value: bool) {
        self.options.tap_tempo = value;
    }
    pub fn set_output_mode(&mut self, value: u8) {
        self.options.output_mode = if value == 0 { OutputMode::Euclidean } else { OutputMode::Drums };
    }
    pub fn set_clock_resolution(&mut self, value: u8) {
        self.options.clock_resolution = clock_resolution_from_u8(value);
    }
    pub fn set_gate_mode(&mut self, gate_mode: bool) {
        self.options.gate_mode = gate_mode;
    }

    pub fn increment_pulse_counter(&mut self) {
        self.pulse_duration_counter += 1;
        // Zero all pulses after 1ms.
        if self.pulse_duration_counter >= PULSE_DURATION && !self.options.gate_mode {
            self.state = 0;
            // Possible mod: the extra random pulse is not reset, and its
            // behaviour is more similar to that of a S&H.
            // self.state &= 0x80;
        }
    }

    pub fn clock_falling_edge(&mut self) {
        if self.options.gate_mode {
            self.state = 0;
        }
    }

    pub fn settings(&self) -> &PatternGeneratorSettings {
        &self.settings
    }
    pub fn mutable_settings(&mut self) -> &mut PatternGeneratorSettings {
        &mut self.settings
    }

    pub fn on_first_beat(&self) -> bool {
        self.first_beat
    }
    pub fn on_beat(&self) -> bool {
        self.beat
    }

    pub fn led_pattern(&self) -> u8 {
        let mut result = 0;
        if self.state & 1 != 0 {
            result |= led_bits::BD;
        }
        if self.state & 2 != 0 {
            result |= led_bits::SD;
        }
        if self.state & 4 != 0 {
            result |= led_bits::HH;
        }
        result
    }

    fn read_drum_map(&self, step: u8, instrument: u8, x: u8, y: u8) -> u8 {
        let i = (x >> 6) as usize;
        let j = (y >> 6) as usize;
        let a_map = DRUM_MAP[i][j];
        let b_map = DRUM_MAP[i + 1][j];
        let c_map = DRUM_MAP[i][j + 1];
        let d_map = DRUM_MAP[i + 1][j + 1];
        let offset = (instrument as usize) * (STEPS_PER_PATTERN as usize) + step as usize;
        let a = a_map[offset];
        let b = b_map[offset];
        let c = c_map[offset];
        let d = d_map[offset];
        u8_mix(u8_mix(a, b, x << 2), u8_mix(c, d, x << 2), y << 2)
    }

    fn evaluate_drums(&mut self) {
        // At the beginning of a pattern, decide on perturbation levels.
        if self.step == 0 {
            for i in 0..NUM_PARTS {
                let randomness = if self.options.swing { 0 } else { self.settings.options.drums.randomness >> 2 };
                self.part_perturbation[i] = u8_u8_mul_shift8(self.rng.get_byte(), randomness);
            }
        }

        let mut instrument_mask = 1u8;
        let x = self.settings.options.drums.x;
        let y = self.settings.options.drums.y;
        let mut accent_bits = 0u8;
        for i in 0..NUM_PARTS {
            let mut level = self.read_drum_map(self.step, i as u8, x, y);
            if level < 255 - self.part_perturbation[i] {
                level += self.part_perturbation[i];
            } else {
                // The sequencer from Anushri uses a weird clipping rule
                // here. Comment this line to reproduce its behavior.
                level = 255;
            }
            let threshold = !self.settings.density[i];
            if level > threshold {
                if level > 192 {
                    accent_bits |= instrument_mask;
                }
                self.state |= instrument_mask;
            }
            instrument_mask <<= 1;
        }
        if self.output_clock() {
            self.state |= if accent_bits != 0 { OutputBits::Common as u8 } else { 0 };
            self.state |= if self.step == 0 { OutputBits::Reset as u8 } else { 0 };
        } else {
            self.state |= accent_bits << 3;
        }
    }

    fn evaluate_euclidean(&mut self) {
        // Refresh only on sixteenth notes.
        if self.step & 1 != 0 {
            return;
        }

        let mut instrument_mask = 1u8;
        let mut reset_bits = 0u8;
        for i in 0..NUM_PARTS {
            let length = (self.settings.options.euclidean_length[i] >> 3) + 1;
            let density = self.settings.density[i] >> 3;
            let address = (length - 1) as u16 * 32 + density as u16;
            while self.euclidean_step[i] >= length {
                self.euclidean_step[i] -= length;
            }
            let step_mask = 1u32 << (self.euclidean_step[i] as u32);
            let pattern_bits = LUT_RES_EUCLIDEAN[address as usize];
            if pattern_bits & step_mask != 0 {
                self.state |= instrument_mask;
            }
            if self.euclidean_step[i] == 0 {
                reset_bits |= instrument_mask;
            }
            instrument_mask <<= 1;
        }

        if self.output_clock() {
            self.state |= if reset_bits != 0 { OutputBits::Common as u8 } else { 0 };
            self.state |= if reset_bits == 0x07 { OutputBits::Reset as u8 } else { 0 };
        } else {
            self.state |= reset_bits << 3;
        }
    }

    fn evaluate(&mut self) {
        self.state = 0;
        self.pulse_duration_counter = 0;

        self.rng.update();
        // Highest bits: clock and random bit.
        self.state |= 0x40;
        self.state |= (self.rng.state() & 0x80) as u8;

        if self.output_clock() {
            self.state |= OutputBits::Clock as u8;
        }

        // Refresh only at step changes.
        if self.pulse != 0 {
            return;
        }

        if self.options.output_mode == OutputMode::Euclidean {
            self.evaluate_euclidean();
        } else {
            self.evaluate_drums();
        }
    }

    pub fn swing_amount(&self) -> i8 {
        if self.options.swing && self.output_mode() == OutputMode::Drums {
            let value = u8_u8_mul_shift8(self.settings.options.drums.randomness, 42 + 1) as i8;
            if self.step & 2 == 0 { value } else { -value }
        } else {
            0
        }
    }
}
