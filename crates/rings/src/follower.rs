//! `rings/dsp/follower.h` -- a 3-band envelope + spectral-centroid follower that
//! drives the FM voice's internal LPG-like behaviour.

use stmlib::fdsp::{slope, sqrt};
use stmlib::filter::{FilterMode, FrequencyApproximation, NaiveSvf};

/// `rings::Follower`.
#[derive(Debug, Clone, Default)]
pub struct Follower {
    low_mid_filter: NaiveSvf,
    mid_high_filter: NaiveSvf,
    attack: [f32; 3],
    decay: [f32; 3],
    detector: [f32; 3],
    centroid: f32,
}

impl Follower {
    pub fn new() -> Self {
        Self::default()
    }

    /// `Init(low, low_mid, mid_high)`.
    pub fn init(&mut self, low: f32, low_mid: f32, mid_high: f32) {
        self.low_mid_filter.init();
        self.mid_high_filter.init();

        self.low_mid_filter
            .set_f_q(low_mid, 0.5, FrequencyApproximation::Dirty);
        self.mid_high_filter
            .set_f_q(mid_high, 0.5, FrequencyApproximation::Dirty);

        self.attack[0] = low_mid;
        self.decay[0] = sqrt(low_mid * low);
        self.attack[1] = sqrt(low_mid * mid_high);
        self.decay[1] = low_mid;
        self.attack[2] = sqrt(mid_high * 0.5);
        self.decay[2] = sqrt(mid_high * low_mid);

        self.detector = [0.0; 3];
        self.centroid = 0.0;
    }

    /// `Process(sample) -> (envelope, centroid)`.
    pub fn process(&mut self, sample: f32) -> (f32, f32) {
        let mut bands = [0.0f32; 3];
        bands[2] = self.mid_high_filter.process(FilterMode::HighPass, sample);
        bands[1] = self
            .low_mid_filter
            .process(FilterMode::HighPass, self.mid_high_filter.lp());
        bands[0] = self.low_mid_filter.lp();

        let mut weighted = 0.0f32;
        let mut total = 0.0f32;
        let mut frequency = 0.0f32;
        for i in 0..3 {
            slope(
                &mut self.detector[i],
                bands[i].abs(),
                self.attack[i],
                self.decay[i],
            );
            weighted += self.detector[i] * frequency;
            total += self.detector[i];
            frequency += 0.5;
        }

        let error = weighted / (total + 0.001) - self.centroid;
        let coefficient = if error > 0.0 { 0.05 } else { 0.001 };
        self.centroid += error * coefficient;

        (total, self.centroid)
    }
}
