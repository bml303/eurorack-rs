//! `clouds/dsp/sample_rate_converter.h` -- polyphase 2x up/down sampler used
//! only in low-fidelity mode. The C templates on `<ratio, filter_size,
//! coefficients>`; clouds instantiates it twice, both with the 45-tap
//! `src_filter_1x_2_45` kernel and ratio -2 (down) / +2 (up).

use alloc::boxed::Box;
use alloc::vec;

use crate::frame::FloatFrame;

/// `SampleRateConverter`.
pub struct SampleRateConverter {
    /// `-2` downsamples (consume 2, produce 1); `+2` upsamples.
    ratio: i32,
    coefficients: &'static [f32],
    history: Box<[FloatFrame]>,
    history_ptr: i32,
}

impl SampleRateConverter {
    /// Build for a given `ratio` (`-2` or `+2`) and FIR `coefficients`.
    pub fn new(ratio: i32, coefficients: &'static [f32]) -> Self {
        let filter_size = coefficients.len();
        let mut c = Self {
            ratio,
            coefficients,
            history: vec![FloatFrame::default(); filter_size * 2].into_boxed_slice(),
            history_ptr: 0,
        };
        c.init();
        c
    }

    /// `Init`.
    pub fn init(&mut self) {
        for h in self.history.iter_mut() {
            *h = FloatFrame::default();
        }
        self.history_ptr = self.coefficients.len() as i32 - 1;
    }

    /// `Process(in, out, input_size)`.
    pub fn process(&mut self, input: &[FloatFrame], output: &mut [FloatFrame], input_size: usize) {
        let filter_size = self.coefficients.len() as i32;
        let scale = if self.ratio < 0 { 1.0 } else { self.ratio as f32 };
        let consumed = if self.ratio < 0 { -self.ratio } else { 1 };
        let produced = if self.ratio > 0 { self.ratio } else { 1 };

        let mut history_ptr = self.history_ptr;
        let mut in_idx = 0usize;
        let mut out_idx = 0usize;
        let mut remaining = input_size as i32;

        while remaining > 0 {
            for _ in 0..consumed {
                let sample = input[in_idx];
                in_idx += 1;
                self.history[history_ptr as usize] = sample;
                self.history[(history_ptr + filter_size) as usize] = sample;
                remaining -= 1;
                history_ptr -= 1;
                if history_ptr < 0 {
                    history_ptr += filter_size;
                }
            }

            for i in 0..produced {
                let mut y_l = 0.0f32;
                let mut y_r = 0.0f32;
                let mut x = (history_ptr + 1) as usize;
                let mut j = i;
                while j < filter_size {
                    let h = self.coefficients[j as usize];
                    let frame = self.history[x];
                    y_l += frame.l * h;
                    y_r += frame.r * h;
                    x += 1;
                    j += produced;
                }
                output[out_idx] = FloatFrame {
                    l: y_l * scale,
                    r: y_r * scale,
                };
                out_idx += 1;
            }
        }
        self.history_ptr = history_ptr;
    }
}
