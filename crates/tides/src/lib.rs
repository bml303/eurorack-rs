//! Rust port of Mutable Instruments **Tides** -- Tidal modulator (2014).
//!
//! This crate ports the DSP core, [`Generator`] (`tides/generator.{h,cc}`):
//! a variable-slope oscillator/envelope, driven either at audio rate (with
//! polyBLEP band-limiting and an optional PLL sync to an external clock) or at
//! control rate, followed by a shared two-pole lowpass + wavefolder.
//!
//! Out of scope, matching the `mi-braids` precedent (see the workspace
//! `PORTING.md`): `cv_scaler.{h,cc}` (ADC calibration), `plotter.{h,cc}` and
//! `easter_egg/` (the OLED display "easter egg"), `ui.{h,cc}` and `tides.cc`
//! (hardware wiring), and the peripheral `drivers/`.
#![no_std]

pub mod generator;
pub mod resources;

pub use generator::{
    FrequencyRatio, Generator, GeneratorMode, GeneratorRange, GeneratorSample, CONTROL_CLOCK,
    CONTROL_CLOCK_RISING, CONTROL_FREEZE, CONTROL_GATE, CONTROL_GATE_FALLING, CONTROL_GATE_RISING,
    FLAG_END_OF_ATTACK, FLAG_END_OF_RELEASE,
};
