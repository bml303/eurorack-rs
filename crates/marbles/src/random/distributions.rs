//! `marbles/random/distributions.h` -- generates samples from various kinds
//! of random distributions.

use crate::resources::{DIST_ICDF_4_3, DISTRIBUTIONS_TABLE};

pub const NUM_BIAS_VALUES: usize = 5;
pub const NUM_RANGE_VALUES: usize = 9;
pub const ICDF_TABLE_SIZE: f32 = 128.0;

/// `stmlib::Interpolate` with the C's "read one past a `size + 1` table,
/// multiplied by a zero fraction" clamped in bounds (as in `mi-rings` /
/// `mi-elements`) -- defensive here (every caller's `uniform` is derived from
/// a `u32/2^32` PRNG draw, always `< 1.0` in practice), kept for consistency
/// with `lag_processor.rs`, where the equivalent call genuinely needs it.
#[inline]
fn interpolate(table: &[f32], index: f32, size: f32) -> f32 {
    let scaled = index.clamp(0.0, 1.0) * size;
    let last = table.len() - 1;
    let integral = (scaled as usize).min(last);
    let fractional = scaled - integral as f32;
    let a = table[integral];
    let b = table[(integral + 1).min(last)];
    a + (b - a) * fractional
}

/// Generates samples from a beta distribution, from uniformly distributed
/// samples. For higher throughput, uses pre-computed tables of inverse cdfs.
pub fn beta_distribution_sample(uniform: f32, spread: f32, bias: f32) -> f32 {
    // Tables are pre-computed only for bias <= 0.5. For values above 0.5,
    // symmetry is used.
    let flip_result = bias > 0.5;
    let (mut uniform, mut bias) = if flip_result {
        (1.0 - uniform, 1.0 - bias)
    } else {
        (uniform, bias)
    };

    bias *= (NUM_BIAS_VALUES as f32 - 1.0) * 2.0;
    let spread = spread * (NUM_RANGE_VALUES as f32 - 1.0);

    let bias_integral = bias as usize;
    let bias_fractional = bias - bias_integral as f32;
    let spread_integral = spread as usize;
    let spread_fractional = spread - spread_integral as f32;

    let cell = bias_integral * (NUM_RANGE_VALUES + 1) + spread_integral;

    // Lower 5% and 95% percentiles use a different table with higher
    // resolution.
    let mut offset = 0usize;
    if uniform <= 0.05 {
        offset = ICDF_TABLE_SIZE as usize + 1;
        uniform *= 20.0;
    } else if uniform >= 0.95 {
        offset = 2 * (ICDF_TABLE_SIZE as usize + 1);
        uniform = (uniform - 0.95) * 20.0;
    }

    let x1y1 = interpolate(&DISTRIBUTIONS_TABLE[cell][offset..], uniform, ICDF_TABLE_SIZE);
    let x2y1 = interpolate(
        &DISTRIBUTIONS_TABLE[cell + 1][offset..],
        uniform,
        ICDF_TABLE_SIZE,
    );
    let x1y2 = interpolate(
        &DISTRIBUTIONS_TABLE[cell + NUM_RANGE_VALUES + 1][offset..],
        uniform,
        ICDF_TABLE_SIZE,
    );
    let x2y2 = interpolate(
        &DISTRIBUTIONS_TABLE[cell + NUM_RANGE_VALUES + 2][offset..],
        uniform,
        ICDF_TABLE_SIZE,
    );

    let y1 = x1y1 + (x2y1 - x1y1) * spread_fractional;
    let y2 = x1y2 + (x2y2 - x1y2) * spread_fractional;
    let mut y = y1 + (y2 - y1) * bias_fractional;

    if flip_result {
        y = 1.0 - y;
    }
    y
}

/// Pre-computed beta(3, 3) with a fatter tail.
pub fn fast_beta_distribution_sample(uniform: f32) -> f32 {
    interpolate(&DIST_ICDF_4_3, uniform, ICDF_TABLE_SIZE)
}

/// Draws samples from a discrete distribution. Used for the quantizer.
///
/// The C template is `DiscreteDistribution<size>`, backed by `size + 2`
/// entry arrays (`cdf_`/`token_ids_`). Rust const generics can't express that
/// arithmetic on stable, so `CELLS` here is that `size + 2` already applied
/// (the sole instantiation, `DiscreteDistribution<kMaxDegrees == 16>`,
/// becomes `DiscreteDistribution<18>`) -- the same trick used by
/// `stmlib::PatternPredictor`.
///
/// ```ignore
/// let mut d: DiscreteDistribution<18> = DiscreteDistribution::new();
/// d.init();
/// d.add_token(1, 0.2);
/// d.add_token(20, 0.7);
/// d.add_token(666, 0.1);
/// d.no_more_tokens();
/// let r = d.sample(u);
/// ```
///
/// Weights do not have to add to 1.0 -- the class handles normalization.
#[derive(Debug, Clone)]
pub struct DiscreteDistribution<const CELLS: usize> {
    sum: f32,
    cdf: [f32; CELLS],
    token_ids: [i32; CELLS],
    num_tokens: usize,
}

pub struct DiscreteDistributionResult {
    pub token_id: i32,
    pub fraction: f32,
    pub start: f32,
    pub width: f32,
}

impl<const CELLS: usize> Default for DiscreteDistribution<CELLS> {
    fn default() -> Self {
        Self {
            sum: 0.0,
            cdf: [0.0; CELLS],
            token_ids: [0; CELLS],
            num_tokens: 1,
        }
    }
}

impl<const CELLS: usize> DiscreteDistribution<CELLS> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn init(&mut self) {
        self.sum = 0.0;
        self.num_tokens = 1;

        self.cdf[0] = 0.0;
        self.token_ids[0] = 0;
    }

    pub fn add_token(&mut self, token_id: i32, weight: f32) {
        if weight <= 0.0 {
            return;
        }
        self.sum += weight;
        self.token_ids[self.num_tokens] = token_id;
        self.cdf[self.num_tokens] = self.sum;
        self.num_tokens += 1;
    }

    pub fn no_more_tokens(&mut self) {
        self.token_ids[self.num_tokens] = self.token_ids[self.num_tokens - 1];
        self.cdf[self.num_tokens] = self.sum + 1.0;
    }

    pub fn sample(&self, u: f32) -> DiscreteDistributionResult {
        let u = u * self.sum;
        // `std::upper_bound(&cdf_[1], &cdf_[num_tokens_ + 1], u)`: index of the
        // first element in `cdf[1..=num_tokens]` that is strictly greater
        // than `u`.
        let mut n = self.num_tokens;
        for (i, &c) in self.cdf[1..=self.num_tokens].iter().enumerate() {
            if c > u {
                n = i + 1;
                break;
            }
        }
        let norm = 1.0 / self.sum;
        DiscreteDistributionResult {
            token_id: self.token_ids[n],
            width: (self.cdf[n] - self.cdf[n - 1]) * norm,
            start: self.cdf[n - 1] * norm,
            fraction: (u - self.cdf[n - 1]) / (self.cdf[n] - self.cdf[n - 1]),
        }
    }
}
