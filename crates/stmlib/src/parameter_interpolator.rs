//! `stmlib/dsp/parameter_interpolator.h`.
//!
//! In C this is an RAII helper: the destructor writes the final value back
//! through a `float*`. Rust has no implicit borrow of external state across a
//! scope like that, so the idiomatic shape is: construct from a value, call
//! [`ParameterInterpolator::next`] `size` times, then read
//! [`ParameterInterpolator::value`] back into your state field.

/// Linear per-sample ramp between an old and a new parameter value.
#[derive(Debug, Clone, Copy)]
pub struct ParameterInterpolator {
    value: f32,
    increment: f32,
}

impl ParameterInterpolator {
    /// Ramp from `from` to `to` across `size` samples.
    #[inline]
    pub fn new(from: f32, to: f32, size: usize) -> Self {
        Self {
            value: from,
            increment: (to - from) / size as f32,
        }
    }

    /// Ramp using an explicit `1 / size` step (matches the C `step` ctor).
    #[inline]
    pub fn with_step(from: f32, to: f32, step: f32) -> Self {
        Self {
            value: from,
            increment: (to - from) * step,
        }
    }

    /// Advance one sample and return the new value.
    #[inline]
    pub fn next(&mut self) -> f32 {
        self.value += self.increment;
        self.value
    }

    /// Value at fractional sample offset `t` from the current position.
    #[inline]
    pub fn subsample(&self, t: f32) -> f32 {
        self.value + self.increment * t
    }

    /// Current value -- write this back into your persistent state after the loop.
    #[inline]
    pub fn value(&self) -> f32 {
        self.value
    }
}
