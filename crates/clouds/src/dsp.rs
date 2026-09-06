//! Small DSP helpers shared across the crate.

/// `stmlib::Interpolate(table, index, size)` with a guarded upper tap.
///
/// Several clouds tables are sized `N + 1` and read with `size = N`, so at
/// `index == 1.0` the C reads `table[N + 1]` -- one past the end. The
/// generated `resources.cc` places the tables back to back, so that read
/// yields the neighbouring table's first entry, and it is *always* multiplied
/// by a zero fractional part. Clamping the tap to the last valid entry gives
/// the identical result without the out-of-bounds access.
#[inline]
pub fn interpolate(table: &[f32], index: f32, size: f32) -> f32 {
    let index = index.clamp(0.0, 1.0) * size;
    let integral = index as usize;
    let fractional = index - integral as f32;
    let a = table[integral];
    let b = *table.get(integral + 1).unwrap_or(&a);
    a + (b - a) * fractional
}
