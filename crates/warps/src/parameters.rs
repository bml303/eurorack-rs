//! `warps/dsp/parameters.h` -- shape/algorithm selectors and the modulation
//! parameter block shared by every DSP stage.
//!
//! `ModulationAlgorithm` is a UI-only labelling enum in the C++ (used by
//! `settings.cc`'s string tables) -- `Modulator` itself only ever reads
//! `parameters_.modulation_algorithm` as a plain `f32` in `[0, 8)` (see
//! `modulator.rs`'s `xmod_table`/`vocoder_amount` derivation), so it isn't
//! ported here.

/// `warps::OscillatorShape` -- also used as the `Oscillator::fn_table_`
/// dispatch index (`Sine` = 0 .. `NoiseLp` = 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OscillatorShape {
    Sine,
    Triangle,
    Saw,
    Pulse,
    NoiseLp,
}

impl OscillatorShape {
    /// `static_cast<OscillatorShape>(index)` -- the C performs this cast on
    /// `parameters_.carrier_shape - 1` / `+ 1`, always in range for the
    /// `carrier_shape` values the UI can produce (`0..=3`); out-of-range
    /// input clamps into the table instead of being undefined behaviour.
    pub fn from_index(index: i32) -> Self {
        match index.clamp(0, 4) {
            0 => OscillatorShape::Sine,
            1 => OscillatorShape::Triangle,
            2 => OscillatorShape::Saw,
            3 => OscillatorShape::Pulse,
            _ => OscillatorShape::NoiseLp,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Parameters {
    pub channel_drive: [f32; 2],
    pub modulation_algorithm: f32,
    pub modulation_parameter: f32,

    // Easter egg parameters.
    pub frequency_shift_pot: f32,
    pub frequency_shift_cv: f32,
    pub phase_shift: f32,
    pub note: f32,

    /// 0 = external.
    pub carrier_shape: i32,
}

impl Parameters {
    /// Apply a non-linear response to the parameter of all algorithms
    /// between 1 and 4.
    pub fn skewed_modulation_parameter(&self) -> f32 {
        let mut skew = 0.0;
        if self.modulation_algorithm <= 1.0 {
            skew = self.modulation_algorithm;
        } else if self.modulation_algorithm >= 5.0 {
            skew = 1.0;
        } else if self.modulation_algorithm >= 4.0 {
            skew = 5.0 - self.modulation_algorithm;
        }
        self.modulation_parameter * (1.0 + skew * (self.modulation_parameter - 1.0))
    }
}
