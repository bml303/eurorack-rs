//! Shared low-level helpers for the Braids oscillators: the pitch tables and the
//! per-block linear ramps that the C implements as preprocessor macros
//! (`BEGIN_INTERPOLATE_*` / `INTERPOLATE_*` / `END_INTERPOLATE_*`).

use crate::resources::{LUT_OSCILLATOR_DELAYS, LUT_OSCILLATOR_INCREMENTS};

pub const HIGHEST_NOTE_ANALOG: i32 = 128 * 128;
pub const HIGHEST_NOTE_DIGITAL: i32 = 140 * 128;
pub const PITCH_TABLE_START: i32 = 128 * 128;
pub const OCTAVE: i32 = 12 * 128;

/// `x >> n` reproducing the C shift on the reference (g++/x86) build.
///
/// A few pitch-table paths compute a shift `>= 32` (or, in [`compute_delay`], a
/// negative one) for pathological inputs -- a note far outside the audio range
/// combined with an extreme timbre. That is undefined behaviour in C; on x86 the
/// hardware masks the count to its low 5 bits, and the firmware's own reference
/// build does the same, so we match that. The affected samples are garbage under
/// any interpretation.
#[inline]
fn c_shr_u32(value: u32, shift: i32) -> u32 {
    value >> ((shift as u32) & 31)
}

/// `AnalogOscillator::ComputePhaseIncrement` (clamps the input to just below the
/// top of the pitch table).
#[inline]
pub fn compute_phase_increment_analog(midi_pitch: i16) -> u32 {
    compute_phase_increment_inner(midi_pitch as i32, PITCH_TABLE_START)
}

/// `DigitalOscillator::ComputePhaseIncrement` -- identical body, same clamp.
#[inline]
pub fn compute_phase_increment_digital(midi_pitch: i32) -> u32 {
    compute_phase_increment_inner(midi_pitch, PITCH_TABLE_START)
}

#[inline]
fn compute_phase_increment_inner(mut midi_pitch: i32, clamp_at: i32) -> u32 {
    if midi_pitch >= clamp_at {
        midi_pitch = clamp_at - 1;
    }
    let mut ref_pitch = midi_pitch - PITCH_TABLE_START;
    let mut num_shifts = 0u32;
    while ref_pitch < 0 {
        ref_pitch += OCTAVE;
        num_shifts += 1;
    }
    let idx = (ref_pitch >> 4) as usize;
    let a = LUT_OSCILLATOR_INCREMENTS[idx];
    let b = LUT_OSCILLATOR_INCREMENTS[idx + 1];
    let phase_increment =
        a.wrapping_add(((b.wrapping_sub(a) as i32).wrapping_mul(ref_pitch & 0xf) >> 4) as u32);
    c_shr_u32(phase_increment, num_shifts as i32)
}

/// `DigitalOscillator::ComputeDelay`.
#[inline]
pub fn compute_delay(mut midi_pitch: i32) -> u32 {
    let limit = HIGHEST_NOTE_DIGITAL - OCTAVE;
    if midi_pitch >= limit {
        midi_pitch = limit;
    }
    let mut ref_pitch = midi_pitch - PITCH_TABLE_START;
    let mut num_shifts = 0i32;
    while ref_pitch < 0 {
        ref_pitch += OCTAVE;
        num_shifts += 1;
    }
    let idx = (ref_pitch >> 4) as usize;
    let a = LUT_OSCILLATOR_DELAYS[idx];
    let b = LUT_OSCILLATOR_DELAYS[idx + 1];
    let delay =
        a.wrapping_add(((b.wrapping_sub(a) as i32).wrapping_mul(ref_pitch & 0xf) >> 4) as u32);
    c_shr_u32(delay, 12 - num_shifts)
}

/// `ThisBlepSample(t)` from `analog_oscillator.h`.
#[inline]
pub fn this_blep_sample(t: u32) -> i32 {
    let t = t.min(65535);
    (t.wrapping_mul(t) >> 18) as i32
}

/// `NextBlepSample(t)` from `analog_oscillator.h`.
#[inline]
pub fn next_blep_sample(t: u32) -> i32 {
    let t = t.min(65535);
    let t = 65535 - t;
    -((t.wrapping_mul(t) >> 18) as i32)
}

/// The `BEGIN/INTERPOLATE/END_INTERPOLATE_PHASE_INCREMENT` macro triple.
///
/// Construct once per block from the previous and target phase increments, call
/// [`next`](Self::next) at the top of each sample, then write [`value`](Self::value)
/// back into `previous_phase_increment_`.
#[derive(Debug, Clone, Copy)]
pub struct PhaseIncrementRamp {
    value: u32,
    step: u32,
}

impl PhaseIncrementRamp {
    #[inline]
    pub fn new(previous: u32, target: u32, size: usize) -> Self {
        let size = size as u32;
        let step = if previous < target {
            (target - previous) / size
        } else {
            !((previous - target) / size)
        };
        Self {
            value: previous,
            step,
        }
    }

    #[inline]
    pub fn next(&mut self) -> u32 {
        self.value = self.value.wrapping_add(self.step);
        self.value
    }

    #[inline]
    pub fn value(&self) -> u32 {
        self.value
    }
}

/// The scalar `BEGIN/INTERPOLATE/END_INTERPOLATE_PARAMETER` macro triple (also
/// covers the `_0` / `_1` array variants -- pass the right element).
#[derive(Debug, Clone, Copy)]
pub struct ParamRamp {
    start: i32,
    delta: i32,
    increment: i32,
    xfade: i32,
}

impl ParamRamp {
    #[inline]
    pub fn new(previous: i32, target: i32, size: usize) -> Self {
        Self {
            start: previous,
            delta: target - previous,
            increment: 32767 / size as i32,
            xfade: 0,
        }
    }

    #[inline]
    pub fn next(&mut self) -> i32 {
        self.xfade += self.increment;
        self.start + (self.delta.wrapping_mul(self.xfade) >> 15)
    }
}
