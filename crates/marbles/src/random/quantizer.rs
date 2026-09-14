//! `marbles/random/quantizer.h` -- variable resolution quantizer.

use stmlib::HysteresisQuantizer;

pub const MAX_DEGREES: usize = 16;
pub const NUM_THRESHOLDS: usize = 7;

#[derive(Debug, Clone, Copy, Default)]
pub struct Degree {
    pub voltage: f32,
    pub weight: u8,
}

#[derive(Debug, Clone, Copy)]
pub struct Scale {
    pub base_interval: f32,
    pub num_degrees: usize,
    pub degree: [Degree; MAX_DEGREES],
}

impl Default for Scale {
    fn default() -> Self {
        let mut s = Self {
            base_interval: 1.0,
            num_degrees: 1,
            degree: [Degree::default(); MAX_DEGREES],
        };
        s.init();
        s
    }
}

impl Scale {
    pub fn cell_voltage(&self, i: usize) -> f32 {
        let transposition = (i / self.num_degrees) as f32 * self.base_interval;
        self.degree[i % self.num_degrees].voltage + transposition
    }

    pub fn init(&mut self) {
        self.base_interval = 1.0;
        self.num_degrees = 1;
        self.degree[0].voltage = 0.0;
        self.degree[0].weight = 0;
    }

    pub fn init_major(&mut self) {
        const MAJOR_SCALE_WEIGHTS: [u8; 12] =
            [255, 16, 128, 16, 192, 64, 8, 224, 16, 96, 32, 160];

        self.base_interval = 1.0;
        self.num_degrees = 12;
        for i in 0..12 {
            self.degree[i].voltage = i as f32 * 0.0833333333;
            self.degree[i].weight = MAJOR_SCALE_WEIGHTS[i];
        }
    }

    pub fn init_tenth(&mut self) {
        const MAJOR_SCALE_WEIGHTS: [u8; 10] = [255, 255, 255, 255, 255, 255, 255, 255, 255, 25];

        self.base_interval = 1.0;
        self.num_degrees = 10;
        for i in 0..10 {
            self.degree[i].voltage = i as f32 * 0.1;
            self.degree[i].weight = MAJOR_SCALE_WEIGHTS[i];
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct Level {
    bitmask: u16,
    first: u8,
    last: u8,
}

#[derive(Debug, Clone)]
pub struct Quantizer {
    voltage: [f32; MAX_DEGREES],

    level: [Level; NUM_THRESHOLDS],
    feedback: [f32; NUM_THRESHOLDS],

    base_interval: f32,
    base_interval_reciprocal: f32,
    num_degrees: usize,
    level_quantizer: HysteresisQuantizer,
}

impl Default for Quantizer {
    fn default() -> Self {
        Self {
            voltage: [0.0; MAX_DEGREES],
            level: [Level::default(); NUM_THRESHOLDS],
            feedback: [0.0; NUM_THRESHOLDS],
            base_interval: 0.0,
            base_interval_reciprocal: 0.0,
            num_degrees: 0,
            level_quantizer: HysteresisQuantizer::default(),
        }
    }
}

impl Quantizer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn init(&mut self, scale: &Scale) {
        let n = scale.num_degrees;

        // We don't want garbage scale data here...
        if n == 0 || n > MAX_DEGREES || scale.base_interval == 0.0 {
            return;
        }

        self.num_degrees = n;
        self.base_interval = scale.base_interval;
        self.base_interval_reciprocal = 1.0 / scale.base_interval;

        let mut second_largest_threshold: u8 = 0;
        for i in 0..n {
            self.voltage[i] = scale.degree[i].voltage;
            if scale.degree[i].weight != 255 && scale.degree[i].weight >= second_largest_threshold
            {
                second_largest_threshold = scale.degree[i].weight;
            }
        }

        let mut thresholds: [u8; NUM_THRESHOLDS] = [0, 16, 32, 64, 128, 192, 255];

        if second_largest_threshold > 192 {
            // Be more selective to only include the notes at rank 1 and 2 at
            // the last but one position.
            thresholds[NUM_THRESHOLDS - 2] = second_largest_threshold;
        }

        for (t, &threshold) in thresholds.iter().enumerate() {
            let mut bitmask: u16 = 0;
            let mut first: u8 = 0xff;
            let mut last: u8 = 0;
            for i in 0..n {
                if scale.degree[i].weight >= threshold {
                    bitmask |= 1 << i;
                    if first == 0xff {
                        first = i as u8;
                    }
                    last = i as u8;
                }
            }
            self.level[t].bitmask = bitmask;
            self.level[t].first = first;
            self.level[t].last = last;
        }

        self.level_quantizer = HysteresisQuantizer::default();
        self.level_quantizer.init();
        self.feedback = [0.0; NUM_THRESHOLDS];
    }

    pub fn process(&mut self, value: f32, amount: f32, hysteresis: bool) -> f32 {
        let mut level = self.level_quantizer.process(amount, NUM_THRESHOLDS as i32 + 1);
        let mut quantized_voltage = value;

        if level > 0 {
            level -= 1;
            let raw_value = value;
            let mut value = value;
            if hysteresis {
                value += self.feedback[level as usize];
            }

            let note = value * self.base_interval_reciprocal;
            let mut note_integral = note as i32;
            let mut note_fractional = note - note_integral as f32;
            if value < 0.0 {
                note_integral -= 1;
                note_fractional += 1.0;
            }
            note_fractional *= self.base_interval;

            // Search for the tightest upper/lower bound in the set of
            // available voltages. `upper_bound`/`lower_bound` wouldn't work
            // here because some entries are masked.
            let l = self.level[level as usize];
            // `l.first`/`l.last` stay at their `Init`-time sentinels (`0xff`
            // / `0`) when no degree meets this threshold level's weight cut
            // -- reachable with a low-weight scale (the crate's own default
            // `Scale`, for instance). The C reads `voltage_[0xff]` in that
            // case (undefined behaviour, but a harmless stray read of static
            // data there); clamped in bounds here since Rust would panic.
            let last_idx = (l.last as usize).min(self.num_degrees - 1);
            let first_idx = (l.first as usize).min(self.num_degrees - 1);
            let mut a = self.voltage[last_idx] - self.base_interval;
            let mut b = self.voltage[first_idx] + self.base_interval;

            let mut bitmask = l.bitmask;
            for i in 0..self.num_degrees {
                if bitmask & 1 != 0 {
                    let v = self.voltage[i];
                    if note_fractional > v {
                        a = v;
                    } else {
                        b = v;
                        break;
                    }
                }
                bitmask >>= 1;
            }

            quantized_voltage = if note_fractional < (a + b) * 0.5 { a } else { b };
            quantized_voltage += note_integral as f32 * self.base_interval;
            self.feedback[level as usize] = (quantized_voltage - raw_value) * 0.25;
        }
        quantized_voltage
    }
}
