//! Fixed-point interpolation and mixing primitives from `stmlib/utils/dsp.h`.
//!
//! Every function reproduces the C integer arithmetic exactly, including the
//! points where the original overflows a 32-bit accumulator and relies on
//! 2's-complement wrap-around. Those spots use `wrapping_*` explicitly; the
//! result is bit-identical to the firmware on any target.
//!
//! Naming: the C used an overloaded `Interpolate824`; Rust needs distinct names,
//! so the element type is a suffix (`_i16`, `_u16`, `_u8`).

/// `Interpolate824(const int16_t*, uint32_t)` -- 8.24 phase into an `i16` table.
#[inline]
pub fn interpolate_824_i16(table: &[i16], phase: u32) -> i16 {
    let idx = (phase >> 24) as usize;
    let a = table[idx] as i32;
    let b = table[idx + 1] as i32;
    let frac = ((phase >> 8) & 0xffff) as i32;
    a.wrapping_add((b - a).wrapping_mul(frac) >> 16) as i16
}

/// `Interpolate824(const uint16_t*, uint32_t)` -- 8.24 phase into a `u16` table.
#[inline]
pub fn interpolate_824_u16(table: &[u16], phase: u32) -> u16 {
    let idx = (phase >> 24) as usize;
    let a = table[idx] as u32;
    let b = table[idx + 1] as u32;
    let frac = (phase >> 8) & 0xffff;
    a.wrapping_add(b.wrapping_sub(a).wrapping_mul(frac) >> 16) as u16
}

/// `Interpolate824(const uint8_t*, uint32_t)` -- note this overload uses the full
/// 24-bit fraction and rescales the 8-bit samples to signed 16-bit.
#[inline]
pub fn interpolate_824_u8(table: &[u8], phase: u32) -> i16 {
    let idx = (phase >> 24) as usize;
    let a = table[idx] as i32;
    let b = table[idx + 1] as i32;
    let frac = (phase & 0x00ff_ffff) as i32;
    (((a << 8) + ((b - a).wrapping_mul(frac) >> 16)) - 32768) as i16
}

/// `Interpolate88(const uint16_t*, uint16_t)`.
#[inline]
pub fn interpolate_88_u16(table: &[u16], index: u16) -> u16 {
    let idx = (index >> 8) as usize;
    let a = table[idx] as i32;
    let b = table[idx + 1] as i32;
    a.wrapping_add((b - a).wrapping_mul((index & 0xff) as i32) >> 8) as u16
}

/// `Interpolate88(const int16_t*, uint16_t)`.
#[inline]
pub fn interpolate_88_i16(table: &[i16], index: u16) -> i16 {
    let idx = (index >> 8) as usize;
    let a = table[idx] as i32;
    let b = table[idx + 1] as i32;
    a.wrapping_add((b - a).wrapping_mul((index & 0xff) as i32) >> 8) as i16
}

/// `Interpolate1022(const int16_t*, uint32_t)`.
#[inline]
pub fn interpolate_1022(table: &[i16], phase: u32) -> i16 {
    let idx = (phase >> 22) as usize;
    let a = table[idx] as i32;
    let b = table[idx + 1] as i32;
    let frac = ((phase >> 6) & 0xffff) as i32;
    a.wrapping_add((b - a).wrapping_mul(frac) >> 16) as i16
}

/// `Interpolate115(const int16_t*, uint16_t)`.
#[inline]
pub fn interpolate_115(table: &[i16], phase: u16) -> i16 {
    let idx = (phase >> 5) as usize;
    let a = table[idx] as i32;
    let b = table[idx + 1] as i32;
    a.wrapping_add((b - a).wrapping_mul((phase & 0x1f) as i32) >> 5) as i16
}

/// `Mix(int16_t a, int16_t b, uint16_t balance)` -- linear blend, `balance`
/// is 0.16 (`0` = all `a`, `0xffff` = all `b`).
#[inline]
pub fn mix_i16(a: i16, b: i16, balance: u16) -> i16 {
    let bal = balance as i32;
    ((a as i32)
        .wrapping_mul(65535 - bal)
        .wrapping_add((b as i32).wrapping_mul(bal))
        >> 16) as i16
}

/// `Mix(uint16_t a, uint16_t b, uint16_t balance)`.
///
/// The C performs this in `int` (signed 32-bit) and overflows for large inputs;
/// the low 16 bits that survive the final truncation are unaffected by the
/// signedness of the `>> 16`, so an `i32` wrapping accumulator matches exactly.
#[inline]
pub fn mix_u16(a: u16, b: u16, balance: u16) -> u16 {
    let bal = balance as i32;
    ((a as i32)
        .wrapping_mul(65535 - bal)
        .wrapping_add((b as i32).wrapping_mul(bal))
        >> 16) as u16
}

/// `Crossfade(const int16_t* a, const int16_t* b, uint32_t phase, uint16_t balance)`.
#[inline]
pub fn crossfade(table_a: &[i16], table_b: &[i16], phase: u32, balance: u16) -> i16 {
    let a = interpolate_824_i16(table_a, phase) as i32;
    let b = interpolate_824_i16(table_b, phase) as i32;
    a.wrapping_add((b - a).wrapping_mul(balance as i32) >> 16) as i16
}

/// `Crossfade(const uint8_t* a, const uint8_t* b, uint32_t phase, uint16_t balance)`.
#[inline]
pub fn crossfade_u8(table_a: &[u8], table_b: &[u8], phase: u32, balance: u16) -> i16 {
    let a = interpolate_824_u8(table_a, phase) as i32;
    let b = interpolate_824_u8(table_b, phase) as i32;
    a.wrapping_add((b - a).wrapping_mul(balance as i32) >> 16) as i16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpolate_endpoints() {
        let t = [0i16, 100, 200, 300];
        assert_eq!(interpolate_824_i16(&t, 0), 0);
        assert_eq!(interpolate_824_i16(&t, 1 << 24), 100);
        // Halfway between table[0] and table[1].
        assert_eq!(interpolate_824_i16(&t, 1 << 23), 50);
    }

    #[test]
    fn mix_extremes() {
        assert_eq!(mix_i16(1000, -1000, 0), 999); // truncation toward -inf of >>16
        assert_eq!(mix_i16(1000, -1000, 0xffff), -1000);
    }
}
