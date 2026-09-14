//! Rust port of Mutable Instruments **Branches** -- a dual Bernoulli gate:
//! each channel, on a rising trigger edge, draws a random 16-bit number and
//! passes the trigger through (or blocks it) depending on whether the draw
//! clears a probability threshold, optionally XORed against the channel's
//! own last outcome (toggle mode) and optionally held until the next
//! trigger instead of following the input's falling edge (latch mode).
//!
//! # Scope
//!
//! `branches.cc` is almost entirely a hardware main loop (GPIO gate I/O,
//! ADC polling, an EEPROM-backed long-press UI for `toggle_mode`/
//! `latch_mode`, LED refresh) around a tiny probabilistic core -- this
//! ports just that core: [`Channel::step`] (the rising/falling-edge gate
//! decision), [`rng::Rng`] (the free-running LFSR that supplies the random
//! draw), and [`probability_to_threshold`] (the ADC-reading -> 16-bit
//! threshold lookup table). Out of scope, per this workspace's
//! DSP-library-only rule: all GPIO/ADC/EEPROM access, the switch
//! long-press state machine that toggles `toggle_mode`/`latch_mode` (a host
//! sets those fields directly instead), and LED state.
//!
//! # Status
//!
//! Fixed-point (AVR), `mi-edges`/`mi-grids`-style verbatim arithmetic (a
//! 32-bit LFSR, a transpiled `PROGMEM` lookup table). No C bit-compare
//! harness (no host-buildable C test for this module); `tests/smoke.rs`
//! exercises [`Branches`] across both channels, every toggle/latch
//! combination, and a long trigger/threshold sweep.
#![no_std]

pub mod resources;
pub mod rng;

use resources::LINEAR_TABLE;
use rng::Rng;

/// `linear_table[adc_value]` -- converts an 8-bit ADC reading (as the
/// firmware's `Adc::ReadOut8()` would produce it) into the 16-bit
/// probability threshold `Channel::step` compares its random draw against.
pub fn probability_to_threshold(adc_value: u8) -> u16 {
    LINEAR_TABLE[adc_value as usize]
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Channel {
    pub toggle_mode: bool,
    pub latch_mode: bool,
    input_state: bool,
    previous_state: bool,
    gate: bool,
}

impl Channel {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn gate(&self) -> bool {
        self.gate
    }

    /// One control-rate step. `input` is the (already debounced) trigger
    /// input level; `random`/`threshold` are the 16-bit values
    /// `branches.cc`'s main loop reads off `rng_state`/`p[i]` (`threshold`
    /// typically from [`probability_to_threshold`]). Returns the new gate
    /// output level.
    pub fn step(&mut self, input: bool, random: u16, threshold: u16) -> bool {
        if input && !self.input_state {
            // Rising edge.
            let mut outcome = random >= threshold && threshold != 65535;
            if self.toggle_mode {
                outcome ^= self.previous_state;
            }
            self.previous_state = outcome;
            self.gate = outcome;
        } else if !input && self.input_state && !self.latch_mode {
            self.gate = false;
        }
        self.input_state = input;
        self.gate
    }
}

/// Both channels plus the shared free-running RNG they draw from.
#[derive(Debug, Clone, Copy, Default)]
pub struct Branches {
    pub channel: [Channel; 2],
    rng: Rng,
}

impl Branches {
    pub fn new() -> Self {
        Self::default()
    }

    /// One control-rate step for both channels: draws a single 32-bit RNG
    /// snapshot and splits it into a 16-bit half per channel, exactly as
    /// `branches.cc`'s main loop does (`random_words & 0xffff`, then
    /// `random_words >>= 16`) before advancing the RNG for next time.
    pub fn step(&mut self, input: [bool; 2], threshold: [u16; 2]) -> [bool; 2] {
        let random_words = self.rng.next_words();
        let out_0 = self.channel[0].step(input[0], (random_words & 0xffff) as u16, threshold[0]);
        let out_1 = self.channel[1].step(input[1], (random_words >> 16) as u16, threshold[1]);
        [out_0, out_1]
    }
}

pub const PORTED: bool = true;
