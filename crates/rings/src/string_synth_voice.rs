//! `rings/dsp/string_synth_voice.h` -- one string-synth note: a stack of
//! `NUM_HARMONICS` octave-spaced [`StringSynthOscillator`]s, harmonic 0 a dark
//! square (pitch-interpolated), the rest bright squares.

use crate::string_synth_oscillator::{OscillatorShape, StringSynthOscillator};

/// `rings::StringSynthVoice<num_harmonics>`.
#[derive(Debug, Clone)]
pub struct StringSynthVoice<const NUM_HARMONICS: usize> {
    oscillator: [StringSynthOscillator; NUM_HARMONICS],
}

impl<const NUM_HARMONICS: usize> Default for StringSynthVoice<NUM_HARMONICS> {
    fn default() -> Self {
        Self {
            oscillator: core::array::from_fn(|_| StringSynthOscillator::new()),
        }
    }
}

impl<const NUM_HARMONICS: usize> StringSynthVoice<NUM_HARMONICS> {
    pub fn new() -> Self {
        Self::default()
    }

    /// `Init`.
    pub fn init(&mut self) {
        for o in self.oscillator.iter_mut() {
            o.init();
        }
    }

    /// `Render(frequency, amplitudes, summed_harmonics, out, size)` -- `out` is
    /// accumulated into. `amplitudes` is `[gain0, gain_saw0, gain1, gain_saw1, ...]`.
    pub fn render(
        &mut self,
        mut frequency: f32,
        amplitudes: &[f32],
        summed_harmonics: usize,
        out: &mut [f32],
        size: usize,
    ) {
        self.oscillator[0].render(
            OscillatorShape::DarkSquare,
            true,
            frequency,
            amplitudes[0],
            amplitudes[1],
            out,
            size,
        );

        for i in 1..summed_harmonics {
            frequency *= 2.0;
            self.oscillator[i].render(
                OscillatorShape::BrightSquare,
                false,
                frequency,
                amplitudes[2 * i],
                amplitudes[2 * i + 1],
                out,
                size,
            );
        }
    }
}
