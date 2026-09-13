//! `plaits/dsp/fm/operator.h` -- one FM operator (a phase accumulator reading
//! a sine table) and the renderer that chains `N` of them together, all
//! sharing one phase-modulation signal per sample.

use core::cell::RefCell;

use crate::oscillator::sine_pm;

/// One operator's persistent state -- carried across render calls so its
/// phase and amplitude ramp click-free from block to block.
#[derive(Debug, Default, Clone, Copy)]
pub struct Operator {
    pub phase: u32,
    pub amplitude: f32,
}

impl Operator {
    pub fn reset(&mut self) {
        self.phase = 0;
        self.amplitude = 0.0;
    }
}

/// Where a [`RenderCall`](super::algorithms::RenderCall)'s phase-modulation
/// signal comes from each sample. Non-negative values additionally mean
/// "operator `n` in *this* chain feeds back into operator 0" -- see
/// [`super::algorithms::Algorithms::compile`], the only place that produces
/// them.
pub const MODULATION_SOURCE_EXTERNAL: i32 = -2;
pub const MODULATION_SOURCE_NONE: i32 = -1;
pub const MODULATION_SOURCE_FEEDBACK: i32 = 0;

/// A buffer the render graph reads from or writes to. Two of a [`Voice`](
/// super::voice::Voice)'s four buffer slots can be the very same backing
/// array (an operator group modulating, then overwriting, the buffer its
/// predecessor just wrote) -- [`RefCell`] gives that safely, at the cost of
/// a runtime borrow check the C's raw pointers didn't need.
pub type Buffer<'a> = RefCell<&'a mut [f32]>;

pub type RenderFn = fn(
    ops: &mut [Operator],
    f: &[f32],
    a: &[f32],
    fb_state: &mut [f32; 2],
    fb_amount: u8,
    modulation: &Buffer,
    out: &Buffer,
);

/// `RenderOperators<n, modulation_source, additive>` -- renders a chain of
/// `N` operators, each phase-modulating the next, into (or, if `ADDITIVE`,
/// added onto) `out`.
pub fn render_operators<const N: usize, const MODULATION_SOURCE: i32, const ADDITIVE: bool>(
    ops: &mut [Operator],
    f: &[f32],
    a: &[f32],
    fb_state: &mut [f32; 2],
    fb_amount: u8,
    modulation: &Buffer,
    out: &Buffer,
) {
    let size = out.borrow().len();
    let scale = 1.0 / size as f32;

    let mut frequency = [0u32; N];
    let mut phase = [0u32; N];
    let mut amplitude = [0.0f32; N];
    let mut amplitude_increment = [0.0f32; N];
    for i in 0..N {
        frequency[i] = (f[i].min(0.5) * 4_294_967_296.0) as u32;
        phase[i] = ops[i].phase;
        amplitude[i] = ops[i].amplitude;
        amplitude_increment[i] = (a[i].min(4.0) - amplitude[i]) * scale;
    }

    let fb_scale = if fb_amount != 0 {
        (1u32 << fb_amount) as f32 / 512.0
    } else {
        0.0
    };
    let mut previous_0 = fb_state[0];
    let mut previous_1 = fb_state[1];

    for i in 0..size {
        let mut pm = if MODULATION_SOURCE >= MODULATION_SOURCE_FEEDBACK {
            (previous_0 + previous_1) * fb_scale
        } else if MODULATION_SOURCE == MODULATION_SOURCE_EXTERNAL {
            modulation.borrow()[i]
        } else {
            0.0
        };

        for (j, phase_j) in phase.iter_mut().enumerate() {
            *phase_j = phase_j.wrapping_add(frequency[j]);
            pm = sine_pm(*phase_j, pm) * amplitude[j];
            amplitude[j] += amplitude_increment[j];
            if MODULATION_SOURCE >= 0 && j == MODULATION_SOURCE as usize {
                previous_1 = previous_0;
                previous_0 = pm;
            }
        }

        if ADDITIVE {
            out.borrow_mut()[i] += pm;
        } else {
            out.borrow_mut()[i] = pm;
        }
    }

    for i in 0..N {
        ops[i].phase = phase[i];
        ops[i].amplitude = amplitude[i];
    }
    if MODULATION_SOURCE >= MODULATION_SOURCE_FEEDBACK {
        fb_state[0] = previous_0;
        fb_state[1] = previous_1;
    }
}
