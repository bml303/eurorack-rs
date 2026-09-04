//! `stmlib/dsp/parameter_interpolator.h`.
//!
//! In C this is an RAII helper: it borrows a `float*` and its destructor
//! writes the final ramped value back through it when the object goes out of
//! scope. That's exactly a Rust `Drop` impl holding a `&mut f32` -- so unlike
//! most of this workspace's C++-isms, this one ports as a *more* idiomatic
//! Rust type than a manual "call `.value()` and write it back yourself" API
//! would be, while remaining just as easy to get wrong if you let the
//! guard drop before you're done (its `next()` calls no longer count).

/// Linear per-sample ramp between an old and a new parameter value. Writes
/// the value it reached back into the borrowed `state` when dropped -- construct
/// it, call [`next`](Self::next) up to `size` times, then let it drop (end its
/// scope, or `drop(interpolator)`) before reading `state` again.
pub struct ParameterInterpolator<'a> {
    state: &'a mut f32,
    value: f32,
    increment: f32,
}

impl<'a> ParameterInterpolator<'a> {
    /// Ramp `*state` to `new_value` over `size` samples.
    #[inline]
    pub fn new(state: &'a mut f32, new_value: f32, size: usize) -> Self {
        let value = *state;
        let increment = (new_value - value) / size as f32;
        Self {
            state,
            value,
            increment,
        }
    }

    /// Ramp using an explicit `1 / size` step (matches the C `step` ctor).
    #[inline]
    pub fn with_step(state: &'a mut f32, new_value: f32, step: f32) -> Self {
        let value = *state;
        let increment = (new_value - value) * step;
        Self {
            state,
            value,
            increment,
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

    /// Current value (also written back to `state` on drop).
    #[inline]
    pub fn value(&self) -> f32 {
        self.value
    }
}

impl Drop for ParameterInterpolator<'_> {
    #[inline]
    fn drop(&mut self) {
        *self.state = self.value;
    }
}
