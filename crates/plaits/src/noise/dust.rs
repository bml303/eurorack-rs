//! `plaits/dsp/noise/dust.h` -- randomly clocked unipolar impulses.

use stmlib::Random;

#[inline]
pub fn dust(frequency: f32) -> f32 {
    let inv_frequency = 1.0 / frequency;
    let u = Random::get_float();
    if u < frequency {
        u * inv_frequency
    } else {
        0.0
    }
}
