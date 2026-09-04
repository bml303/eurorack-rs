//! `stmlib/dsp/rsqrt.h` -- fast approximate reciprocal square root (the
//! "Quake" bit-hack), used to keep oscillators built on a rotating
//! `(x, y)` vector normalised without a real `1/sqrt`.

#[inline]
pub fn fast_rsqrt_carmack(x: f32) -> f32 {
    let i = 0x5f3759df_u32.wrapping_sub(x.to_bits() >> 1);
    let y = f32::from_bits(i);
    let x2 = x * 0.5;
    y * (1.5 - (x2 * y * y))
}

#[inline]
pub fn fast_rsqrt_accurate(fp0: f32) -> f32 {
    const MIN: f32 = 1.0e-38;
    const ONE_P5: f32 = 1.5;
    let q = fp0.to_bits();
    let mut fp2 = f32::from_bits(0x5F3997BB_u32.wrapping_sub((q >> 1) & 0x3FFF_FFFF));
    let fp1 = ONE_P5 * fp0 - fp0;
    let fp3 = fp2 * fp2;
    if fp0 < MIN {
        return if fp0 > 0.0 { fp2 } else { 1000.0 };
    }
    let mut fp3 = ONE_P5 - fp1 * fp3;
    fp2 *= fp3;
    fp3 = fp2 * fp2;
    let fp3 = ONE_P5 - fp1 * fp3;
    fp2 *= fp3;
    let fp3 = fp2 * fp2;
    let fp3 = ONE_P5 - fp1 * fp3;
    fp2 * fp3
}
