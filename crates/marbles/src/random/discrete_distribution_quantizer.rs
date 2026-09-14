//! `marbles/random/discrete_distribution_quantizer.h` -- quantize voltages by
//! sampling from a discrete distribution.

use stmlib::constrain;

use super::distributions::DiscreteDistribution;
use super::quantizer::{MAX_DEGREES, Scale};

/// `DiscreteDistribution<kMaxDegrees>` -- see `DiscreteDistribution`'s doc
/// comment for why the const generic is `size + 2` (`18` for `MAX_DEGREES ==
/// 16`).
type Distribution = DiscreteDistribution<{ MAX_DEGREES + 2 }>;

#[derive(Debug, Clone, Copy, Default)]
struct Cell {
    center: f32,
    width: f32,
    weight: f32,
}

impl Cell {
    fn scaled_width(&self, amount: f32) -> f32 {
        let w = 8.0 * (self.weight - amount) + 0.5;
        let w = constrain(w, 0.0, 1.0);
        w * self.width
    }
}

#[derive(Debug, Clone)]
pub struct DiscreteDistributionQuantizer {
    base_interval: f32,
    base_interval_reciprocal: f32,

    num_cells: usize,
    cells: [Cell; MAX_DEGREES + 1],
    distribution: Distribution,
}

impl Default for DiscreteDistributionQuantizer {
    fn default() -> Self {
        Self {
            base_interval: 0.0,
            base_interval_reciprocal: 0.0,
            num_cells: 0,
            cells: [Cell::default(); MAX_DEGREES + 1],
            distribution: Distribution::default(),
        }
    }
}

impl DiscreteDistributionQuantizer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn init(&mut self, scale: &Scale) {
        let n = scale.num_degrees;

        // We don't want garbage scale data here...
        if n == 0 || n > MAX_DEGREES || scale.base_interval == 0.0 {
            return;
        }

        self.base_interval = scale.base_interval;
        self.base_interval_reciprocal = 1.0 / scale.base_interval;
        self.num_cells = n + 1;
        for i in 0..=n {
            let previous_voltage = scale.cell_voltage(if i == 0 { 0 } else { i - 1 });
            let next_voltage = scale.cell_voltage(if i == n { n } else { i + 1 });
            self.cells[i].center = scale.cell_voltage(i);
            self.cells[i].width = 0.5 * (next_voltage - previous_voltage);
            self.cells[i].weight = scale.degree[i % n].weight as f32 / 256.0;
        }
    }

    pub fn process(&mut self, value: f32, amount: f32) -> f32 {
        if amount < 0.0 {
            return value;
        }

        let raw_value = value;

        // Assuming 1V/Octave and a scale repeating every octave,
        // `note_integral` will store the octave number, and
        // `note_fractional` the fractional pitch class.
        let note = value * self.base_interval_reciprocal;
        let mut note_integral = note as i32;
        let mut note_fractional = note - note_integral as f32;
        if value < 0.0 {
            note_integral -= 1;
            note_fractional += 1.0;
        }

        // For amount ranging between 0 and 0.25, do not remove notes from the
        // scale, just crossfade from the unquantized output to the quantized
        // output.
        let scaled_amount = if amount < 0.25 { 0.0 } else { (amount - 0.25) * 1.333 };

        self.distribution.init();
        for i in 0..self.num_cells - 1 {
            self.distribution
                .add_token(i as i32, self.cells[i].scaled_width(scaled_amount));
        }
        self.distribution.no_more_tokens();
        let r = self.distribution.sample(note_fractional);

        let offset = note_integral as f32 * self.base_interval;
        let mut quantized_value = self.cells[r.token_id as usize].center + offset;
        // (`r.start`/`r.width` are computed by the C++ too, but never read
        // again after this point -- dead in the original.)

        if amount < 0.25 {
            let amount = amount * 4.0;

            let x = if r.token_id == 0 {
                r.fraction - 1.0
            } else if r.token_id as usize == self.num_cells - 1 {
                -r.fraction
            } else {
                2.0 * ((r.fraction - 0.5).abs() - 0.5)
            };
            let slope = amount / (1.01 - amount);
            let y = f32::max(x * slope + 1.0, 0.0);
            quantized_value -= y * (quantized_value - raw_value);
        }

        quantized_value
    }
}
