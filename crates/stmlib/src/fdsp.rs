//! Floating-point DSP helpers from `stmlib/dsp/dsp.h`.
//!
//! The `TEST`-build (host) variants are used -- e.g. `Clip16` clamps rather than
//! issuing an ARM `ssat`, `Sqrt` calls `sqrtf`. Behaviour is identical.

/// `Interpolate(table, index, size)` -- `index` in `[0, 1)`, scaled by `size`.
#[inline]
pub fn interpolate(table: &[f32], index: f32, size: f32) -> f32 {
    let index = index * size;
    let integral = index as i32;
    let fractional = index - integral as f32;
    let a = table[integral as usize];
    let b = table[integral as usize + 1];
    a + (b - a) * fractional
}

/// `InterpolateHermite(table, index, size)` -- 4-point (cubic) interpolation.
#[inline]
pub fn interpolate_hermite(table: &[f32], index: f32, size: f32) -> f32 {
    let index = index * size;
    let integral = index as i32;
    let fractional = index - integral as f32;
    let i = integral as usize;
    let xm1 = table[i - 1];
    let x0 = table[i];
    let x1 = table[i + 1];
    let x2 = table[i + 2];
    let c = (x1 - xm1) * 0.5;
    let v = x0 - x1;
    let w = c + v;
    let a = w + v + (x2 - x0) * 0.5;
    let b_neg = w + a;
    let f = fractional;
    (((a * f) - b_neg) * f + c) * f + x0
}

/// `InterpolateWrap(table, index, size)` -- takes the fractional part of `index`.
#[inline]
pub fn interpolate_wrap(table: &[f32], index: f32, size: f32) -> f32 {
    let index = index - (index as i32) as f32;
    interpolate(table, index, size)
}

/// `SmoothStep(x)`.
#[inline]
pub fn smooth_step(value: f32) -> f32 {
    value * value * (3.0 - 2.0 * value)
}

/// `ONE_POLE(out, in, coefficient)` from the C macro.
#[inline]
pub fn one_pole(out: &mut f32, input: f32, coefficient: f32) {
    *out += coefficient * (input - *out);
}

/// `SLOPE(out, in, positive, negative)`.
#[inline]
pub fn slope(out: &mut f32, input: f32, positive: f32, negative: f32) {
    let error = input - *out;
    *out += (if error > 0.0 { positive } else { negative }) * error;
}

/// `SLEW(out, in, delta)`.
#[inline]
pub fn slew(out: &mut f32, input: f32, delta: f32) {
    let error = (input - *out).clamp(-delta, delta);
    *out += error;
}

/// `Crossfade(a, b, fade)`.
#[inline]
pub fn crossfade(a: f32, b: f32, fade: f32) -> f32 {
    a + (b - a) * fade
}

/// `SoftLimit(x)` -- Pade approximant of `tanh`-ish saturation.
#[inline]
pub fn soft_limit(x: f32) -> f32 {
    x * (27.0 + x * x) / (27.0 + 9.0 * x * x)
}

/// `SoftClip(x)`.
#[inline]
pub fn soft_clip(x: f32) -> f32 {
    if x < -3.0 {
        -1.0
    } else if x > 3.0 {
        1.0
    } else {
        soft_limit(x)
    }
}

/// `Clip16(x)` (host / `TEST` build: a clamp).
#[inline]
pub fn clip16(x: i32) -> i32 {
    x.clamp(-32768, 32767)
}

/// `ClipU16(x)` (host / `TEST` build: a clamp).
#[inline]
pub fn clip_u16(x: i32) -> u16 {
    x.clamp(0, 65535) as u16
}

/// `SoftConvert(x)` -- saturate a normalised float and convert to `i16`.
#[inline]
pub fn soft_convert(x: f32) -> i16 {
    clip16((soft_limit(x * 0.5) * 32768.0) as i32) as i16
}
