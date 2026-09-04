//! `plaits/dsp/oscillator/*` -- the waveform generators shared by most
//! engines.

pub mod formant_oscillator;
pub mod grainlet_oscillator;
pub mod harmonic_oscillator;
pub mod nes_triangle_oscillator;
pub mod oscillator;
pub mod sine_oscillator;
pub mod string_synth_oscillator;
pub mod super_square_oscillator;
pub mod variable_saw_oscillator;
pub mod variable_shape_oscillator;
pub mod vosim_oscillator;
pub mod wavetable_oscillator;
pub mod z_oscillator;

pub use formant_oscillator::FormantOscillator;
pub use grainlet_oscillator::GrainletOscillator;
pub use harmonic_oscillator::HarmonicOscillator;
pub use nes_triangle_oscillator::NesTriangleOscillator;
pub use oscillator::{Oscillator, OscillatorShape, MAX_FREQUENCY, MIN_FREQUENCY};
pub use sine_oscillator::{sine, sine_no_wrap, sine_pm, sine_raw, FastSineOscillator, SineOscillator};
pub use string_synth_oscillator::StringSynthOscillator;
pub use super_square_oscillator::SuperSquareOscillator;
pub use variable_saw_oscillator::VariableSawOscillator;
pub use variable_shape_oscillator::VariableShapeOscillator;
pub use vosim_oscillator::VosimOscillator;
pub use wavetable_oscillator::{interpolate_wave, interpolate_wave_hermite, Differentiator, WavetableOscillator};
pub use z_oscillator::ZOscillator;
