//! `stmlib/dsp/filter.h` -- zero-delay-feedback one-pole and state-variable
//! filters (topology-preserving transform).
//!
//! The C picks the frequency-warping approximation and the output mode via
//! template parameters (resolved at compile time, in the hot loop). Here they
//! are runtime enums matched per call -- same result, the C's reason for
//! templating was code size / speed on a Cortex-M, which doesn't apply to a
//! portable library.

/// How `tan(pi * f)` is approximated when setting a filter's frequency.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrequencyApproximation {
    Exact,
    Accurate,
    Fast,
    Dirty,
}

/// Which combination of the SVF's outputs `process` returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterMode {
    LowPass,
    BandPass,
    BandPassNormalized,
    HighPass,
}

const PI_F: f32 = core::f32::consts::PI;
const PI_POW_2: f32 = PI_F * PI_F;
const PI_POW_3: f32 = PI_POW_2 * PI_F;
const PI_POW_5: f32 = PI_POW_3 * PI_POW_2;
const PI_POW_7: f32 = PI_POW_5 * PI_POW_2;
const PI_POW_9: f32 = PI_POW_7 * PI_POW_2;
const PI_POW_11: f32 = PI_POW_9 * PI_POW_2;

#[inline]
pub fn one_pole_tan(f: f32, approximation: FrequencyApproximation) -> f32 {
    match approximation {
        FrequencyApproximation::Exact => {
            let f = if f < 0.497 { f } else { 0.497 };
            libm::tanf(PI_F * f)
        }
        FrequencyApproximation::Dirty => {
            let a = 3.736e-1 * PI_POW_3;
            f * (PI_F + a * f * f)
        }
        FrequencyApproximation::Fast => {
            let a = 3.260e-1 * PI_POW_3;
            let b = 1.823e-1 * PI_POW_5;
            let f2 = f * f;
            f * (PI_F + f2 * (a + b * f2))
        }
        FrequencyApproximation::Accurate => {
            let a = 3.333314036e-1 * PI_POW_3;
            let b = 1.333923995e-1 * PI_POW_5;
            let c = 5.33740603e-2 * PI_POW_7;
            let d = 2.900525e-3 * PI_POW_9;
            let e = 9.5168091e-3 * PI_POW_11;
            let f2 = f * f;
            f * (PI_F + f2 * (a + f2 * (b + f2 * (c + f2 * (d + f2 * e)))))
        }
    }
}

/// `stmlib::DCBlocker`.
#[derive(Debug, Clone, Copy, Default)]
pub struct DcBlocker {
    pole: f32,
    x: f32,
    y: f32,
}

impl DcBlocker {
    pub fn init(&mut self, pole: f32) {
        self.x = 0.0;
        self.y = 0.0;
        self.pole = pole;
    }

    pub fn process(&mut self, in_out: &mut [f32]) {
        let (mut x, mut y) = (self.x, self.y);
        for s in in_out.iter_mut() {
            let old_x = x;
            x = *s;
            y = y * self.pole + x - old_x;
            *s = y;
        }
        self.x = x;
        self.y = y;
    }
}

/// `stmlib::OnePole` -- a one-pole TPT filter.
#[derive(Debug, Clone, Copy, Default)]
pub struct OnePole {
    g: f32,
    gi: f32,
    state: f32,
}

impl OnePole {
    pub fn init(&mut self) {
        self.set_f(0.01, FrequencyApproximation::Dirty);
        self.reset();
    }

    pub fn reset(&mut self) {
        self.state = 0.0;
    }

    pub fn set_f(&mut self, f: f32, approximation: FrequencyApproximation) {
        self.g = one_pole_tan(f, approximation);
        self.gi = 1.0 / (1.0 + self.g);
    }

    #[inline]
    pub fn process(&mut self, mode: FilterMode, input: f32) -> f32 {
        let lp = (self.g * input + self.state) * self.gi;
        self.state = self.g * (input - lp) + lp;
        match mode {
            FilterMode::LowPass => lp,
            FilterMode::HighPass => input - lp,
            _ => 0.0,
        }
    }

    pub fn process_block(&mut self, mode: FilterMode, in_out: &mut [f32]) {
        for s in in_out.iter_mut() {
            *s = self.process(mode, *s);
        }
    }

    /// Alias of [`process_block`](Self::process_block) -- the C's in-place
    /// `Process<mode>(float* in_out, size)` overload.
    pub fn process_in_place(&mut self, mode: FilterMode, in_out: &mut [f32]) {
        self.process_block(mode, in_out);
    }
}

/// `stmlib::Svf` -- a state-variable TPT filter (Chamberlin/Zavalishin form).
#[derive(Debug, Clone, Copy, Default)]
pub struct Svf {
    g: f32,
    r: f32,
    h: f32,
    state_1: f32,
    state_2: f32,
}

impl Svf {
    pub fn init(&mut self) {
        self.set_f_q(0.01, 100.0, FrequencyApproximation::Dirty);
        self.reset();
    }

    pub fn reset(&mut self) {
        self.state_1 = 0.0;
        self.state_2 = 0.0;
    }

    #[inline]
    pub fn set(&mut self, other: &Svf) {
        self.g = other.g;
        self.r = other.r;
        self.h = other.h;
    }

    #[inline]
    pub fn set_g_r_h(&mut self, g: f32, r: f32, h: f32) {
        self.g = g;
        self.r = r;
        self.h = h;
    }

    #[inline]
    pub fn set_g_r(&mut self, g: f32, r: f32) {
        self.g = g;
        self.r = r;
        self.h = 1.0 / (1.0 + self.r * self.g + self.g * self.g);
    }

    #[inline]
    pub fn set_g_q(&mut self, g: f32, resonance: f32) {
        self.g = g;
        self.r = 1.0 / resonance;
        self.h = 1.0 / (1.0 + self.r * self.g + self.g * self.g);
    }

    pub fn set_f_q(&mut self, f: f32, resonance: f32, approximation: FrequencyApproximation) {
        self.g = one_pole_tan(f, approximation);
        self.r = 1.0 / resonance;
        self.h = 1.0 / (1.0 + self.r * self.g + self.g * self.g);
    }

    #[inline]
    fn step(&mut self, input: f32) -> (f32, f32, f32) {
        let hp = (input - self.r * self.state_1 - self.g * self.state_1 - self.state_2) * self.h;
        let bp = self.g * hp + self.state_1;
        self.state_1 = self.g * hp + bp;
        let lp = self.g * bp + self.state_2;
        self.state_2 = self.g * bp + lp;
        (hp, bp, lp)
    }

    #[inline]
    fn select(mode: FilterMode, hp: f32, bp: f32, lp: f32, r: f32) -> f32 {
        match mode {
            FilterMode::LowPass => lp,
            FilterMode::BandPass => bp,
            FilterMode::BandPassNormalized => bp * r,
            FilterMode::HighPass => hp,
        }
    }

    #[inline]
    pub fn process(&mut self, mode: FilterMode, input: f32) -> f32 {
        let (hp, bp, lp) = self.step(input);
        Self::select(mode, hp, bp, lp, self.r)
    }

    /// `Process<mode_1, mode_2>(in, out_1, out_2)` -- two outputs from one step.
    #[inline]
    pub fn process_dual(&mut self, mode_1: FilterMode, mode_2: FilterMode, input: f32) -> (f32, f32) {
        let (hp, bp, lp) = self.step(input);
        (
            Self::select(mode_1, hp, bp, lp, self.r),
            Self::select(mode_2, hp, bp, lp, self.r),
        )
    }

    pub fn process_block(&mut self, mode: FilterMode, input: &[f32], out: &mut [f32]) {
        for (i, o) in input.iter().zip(out.iter_mut()) {
            *o = self.process(mode, *i);
        }
    }

    /// `Process<mode>(buf, buf, size)` -- the C's common in-place call, where
    /// `in` and `out` alias; a separate method because Rust can't alias
    /// `&[f32]`/`&mut [f32]` the way C's raw pointers do.
    pub fn process_in_place(&mut self, mode: FilterMode, buf: &mut [f32]) {
        for s in buf.iter_mut() {
            *s = self.process(mode, *s);
        }
    }

    pub fn process_add_block(&mut self, mode: FilterMode, input: &[f32], out: &mut [f32], gain: f32) {
        for (i, o) in input.iter().zip(out.iter_mut()) {
            *o += gain * self.process(mode, *i);
        }
    }

    /// `ProcessMultimode`: continuously morph LP -> BP -> HP as `mode` sweeps
    /// `[0, 1]`.
    pub fn process_multimode(&mut self, input: &[f32], out: &mut [f32], mode: f32) {
        let hp_gain = if mode < 0.5 { -mode * 2.0 } else { -2.0 + mode * 2.0 };
        let lp_gain = if mode < 0.5 { 1.0 - mode * 2.0 } else { 0.0 };
        let bp_gain = if mode < 0.5 { 0.0 } else { mode * 2.0 - 1.0 };
        for (i, o) in input.iter().zip(out.iter_mut()) {
            let (hp, bp, lp) = self.step(*i);
            *o = hp_gain * hp + bp_gain * bp + lp_gain * lp;
        }
    }

    /// `ProcessMultimodeLPtoHP`: LP -> BP -> HP with a different crossfade law.
    pub fn process_multimode_lp_to_hp(&mut self, input: &[f32], out: &mut [f32], mode: f32) {
        let hp_gain = (-mode * 2.0 + 1.0).min(0.0);
        let bp_gain = 1.0 - 2.0 * (mode - 0.5).abs();
        let lp_gain = (1.0 - mode * 2.0).max(0.0);
        for (i, o) in input.iter().zip(out.iter_mut()) {
            let (hp, bp, lp) = self.step(*i);
            *o = hp_gain * hp + bp_gain * bp + lp_gain * lp;
        }
    }

    /// `Process<mode>(in, out_1, out_2, size, gain_1, gain_2)` -- accumulate a
    /// gained copy of one output into each of two buffers.
    pub fn process_into_two(
        &mut self,
        mode: FilterMode,
        input: &[f32],
        out_1: &mut [f32],
        out_2: &mut [f32],
        gain_1: f32,
        gain_2: f32,
    ) {
        for i in 0..input.len() {
            let value = self.process(mode, input[i]);
            out_1[i] += value * gain_1;
            out_2[i] += value * gain_2;
        }
    }

    #[inline]
    pub fn g(&self) -> f32 {
        self.g
    }
    #[inline]
    pub fn r(&self) -> f32 {
        self.r
    }
    #[inline]
    pub fn h(&self) -> f32 {
        self.h
    }
}

/// A cheaper (and less stable at high resonance) SVF, using a naive/direct
/// discretization instead of `Svf`'s zero-delay-feedback topology. Only
/// used by `StringMachineEngine`.
#[derive(Debug, Clone, Copy, Default)]
pub struct NaiveSvf {
    f: f32,
    damp: f32,
    lp: f32,
    bp: f32,
}

impl NaiveSvf {
    pub fn init(&mut self) {
        self.set_f_q(0.01, 100.0, FrequencyApproximation::Dirty);
        self.reset();
    }

    pub fn reset(&mut self) {
        self.lp = 0.0;
        self.bp = 0.0;
    }

    pub fn set_f_q(&mut self, f: f32, resonance: f32, approximation: FrequencyApproximation) {
        let f = if approximation == FrequencyApproximation::Exact {
            let f = if f < 0.497 { f } else { 0.497 };
            2.0 * libm::sinf(PI_F * f)
        } else {
            let f = if f < 0.158 { f } else { 0.158 };
            2.0 * PI_F * f
        };
        self.f = f;
        self.damp = 1.0 / resonance;
    }

    pub fn process(&mut self, mode: FilterMode, input: f32) -> f32 {
        let bp_normalized = self.bp * self.damp;
        let notch = input - bp_normalized;
        self.lp += self.f * self.bp;
        let hp = notch - self.lp;
        self.bp += self.f * hp;

        match mode {
            FilterMode::LowPass => self.lp,
            FilterMode::BandPass => self.bp,
            FilterMode::BandPassNormalized => bp_normalized,
            FilterMode::HighPass => hp,
        }
    }

    #[inline]
    pub fn lp(&self) -> f32 {
        self.lp
    }
    #[inline]
    pub fn bp(&self) -> f32 {
        self.bp
    }
}
