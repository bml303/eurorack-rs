//! `tides2/ramp/ratio.h` -- a clock division/multiplication ratio.

#[derive(Debug, Clone, Copy, Default)]
pub struct Ratio {
    pub ratio: f32,
    pub q: i32,
}
