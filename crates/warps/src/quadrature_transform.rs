//! `warps/dsp/quadrature_transform.h` -- extracts, from an audio signal, two
//! phase-shifted signals that are the Hilbert transform of each other, via a
//! cascade of first-order all-pass filters alternating between the I and Q
//! output chains.

pub const K_MAX_NUM_FILTERS: usize = 24;

#[derive(Debug, Clone, Copy, Default)]
struct AllPassFilter {
    x: f32,
    y: f32,
    coefficient: f32,
}

impl AllPassFilter {
    fn init(&mut self, coefficient: f32) {
        self.x = 0.0;
        self.y = 0.0;
        self.coefficient = coefficient;
    }

    #[inline]
    fn process_sample(&mut self, input: f32) -> f32 {
        let y = self.coefficient * (input - self.y) + self.x;
        self.x = input;
        self.y = y;
        y
    }

    fn process_block(&mut self, input: &[f32], out: &mut [f32]) {
        for (i, o) in input.iter().zip(out.iter_mut()) {
            *o = self.process_sample(*i);
        }
    }

    fn process_in_place(&mut self, buf: &mut [f32]) {
        for s in buf.iter_mut() {
            *s = self.process_sample(*s);
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct QuadratureTransform {
    filters: [AllPassFilter; K_MAX_NUM_FILTERS],
    num_filters: usize,
}

impl Default for QuadratureTransform {
    fn default() -> Self {
        Self { filters: [AllPassFilter::default(); K_MAX_NUM_FILTERS], num_filters: 0 }
    }
}

impl QuadratureTransform {
    pub fn init(&mut self, poles: &[f32]) {
        self.num_filters = poles.len();
        for (f, &p) in self.filters.iter_mut().zip(poles.iter()) {
            f.init(-p);
        }
    }

    /// `Process(float in, float* i_out, float* q_out)` -- one sample at a time.
    pub fn process_sample(&mut self, input: f32) -> (f32, f32) {
        let mut i_out = 0.0f32;
        let mut q_out = 0.0f32;
        for i in 0..self.num_filters {
            let source = if i <= 1 {
                input
            } else if i & 1 == 1 {
                q_out
            } else {
                i_out
            };
            let y = self.filters[i].process_sample(source);
            if i & 1 == 1 {
                q_out = y;
            } else {
                i_out = y;
            }
        }
        (i_out, q_out)
    }

    /// `Process(const float* in, float* i_out, float* q_out, size_t size)` --
    /// the first two filters read from `input`; every later filter in the
    /// cascade runs in place on whichever of `i_out`/`q_out` it feeds.
    pub fn process_block(&mut self, input: &[f32], i_out: &mut [f32], q_out: &mut [f32]) {
        for i in 0..self.num_filters {
            if i == 0 {
                self.filters[i].process_block(input, i_out);
            } else if i == 1 {
                self.filters[i].process_block(input, q_out);
            } else if i & 1 == 0 {
                self.filters[i].process_in_place(i_out);
            } else {
                self.filters[i].process_in_place(q_out);
            }
        }
    }
}
