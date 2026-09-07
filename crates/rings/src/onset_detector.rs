//! `rings/dsp/onset_detector.h` -- a compressor + 3-band filter bank + spectral
//! onset-detection function, used by [`Strummer`](crate::Strummer) to fire an
//! internal strum from an audio transient.

use stmlib::fdsp::{slope, sqrt};
use stmlib::filter::{FrequencyApproximation, NaiveSvf};

use crate::dsp::MAX_BLOCK_SIZE;

/// `rings::ZScorer` -- running mean / variance, tests for outliers.
#[derive(Debug, Clone, Copy, Default)]
struct ZScorer {
    coefficient: f32,
    mean: f32,
    variance: f32,
}

impl ZScorer {
    fn init(&mut self, cutoff: f32) {
        self.coefficient = cutoff;
        self.mean = 0.0;
        self.variance = 0.0;
    }

    #[inline]
    fn update(&mut self, sample: f32) -> f32 {
        let centered = sample - self.mean;
        self.mean += self.coefficient * centered;
        self.variance += self.coefficient * (centered * centered - self.variance);
        centered
    }

    #[inline]
    fn test(&mut self, sample: f32, threshold: f32, absolute_threshold: f32) -> bool {
        let value = self.update(sample);
        value > sqrt(self.variance) * threshold && value > absolute_threshold
    }
}

/// `rings::Compressor` -- automatic gain control ahead of the filter bank.
#[derive(Debug, Clone, Copy, Default)]
struct Compressor {
    attack: f32,
    decay: f32,
    level: f32,
    skew: f32,
}

impl Compressor {
    fn init(&mut self, attack: f32, decay: f32, max_gain: f32) {
        self.attack = attack;
        self.decay = decay;
        self.level = 0.0;
        self.skew = 1.0 / max_gain;
    }

    fn process(&mut self, input: &[f32], out: &mut [f32]) {
        let mut level = self.level;
        for (i, o) in input.iter().zip(out.iter_mut()) {
            slope(&mut level, i.abs(), self.attack, self.decay);
            *o = *i / (self.skew + level);
        }
        self.level = level;
    }
}

/// `rings::OnsetDetector`.
#[derive(Debug, Clone)]
pub struct OnsetDetector {
    compressor: Compressor,
    low_mid_filter: NaiveSvf,
    mid_high_filter: NaiveSvf,

    attack: [f32; 3],
    decay: [f32; 3],
    energy: [f32; 3],
    envelope: [f32; 3],
    onset_df: f32,

    z_df: ZScorer,

    inhibit_threshold: f32,
    inhibit_decay: f32,
    inhibit_time: i32,
    inhibit_counter: i32,
}

impl Default for OnsetDetector {
    fn default() -> Self {
        Self {
            compressor: Compressor::default(),
            low_mid_filter: NaiveSvf::default(),
            mid_high_filter: NaiveSvf::default(),
            attack: [0.0; 3],
            decay: [0.0; 3],
            energy: [0.0; 3],
            envelope: [0.0; 3],
            onset_df: 0.0,
            z_df: ZScorer::default(),
            inhibit_threshold: 0.0,
            inhibit_decay: 0.0,
            inhibit_time: 0,
            inhibit_counter: 0,
        }
    }
}

impl OnsetDetector {
    pub fn new() -> Self {
        Self::default()
    }

    /// `Init(low, low_mid, mid_high, decimated_sr, ioi_time)`.
    pub fn init(
        &mut self,
        low: f32,
        low_mid: f32,
        mid_high: f32,
        decimated_sr: f32,
        ioi_time: f32,
    ) {
        let ioi_f = 1.0 / (ioi_time * decimated_sr);
        self.compressor.init(ioi_f * 10.0, ioi_f * 0.05, 40.0);

        self.low_mid_filter.init();
        self.mid_high_filter.init();
        self.low_mid_filter
            .set_f_q(low_mid, 0.5, FrequencyApproximation::Dirty);
        self.mid_high_filter
            .set_f_q(mid_high, 0.5, FrequencyApproximation::Dirty);

        for i in 0..3 {
            self.attack[i] = low_mid;
            self.decay[i] = low * 0.25;
        }

        self.envelope = [0.0; 3];
        self.energy = [0.0; 3];

        self.z_df.init(ioi_f * 0.05);

        self.inhibit_time = (ioi_time * decimated_sr) as i32;
        self.inhibit_decay = 1.0 / (ioi_time * decimated_sr);
        self.inhibit_threshold = 0.0;
        self.inhibit_counter = 0;
        self.onset_df = 0.0;
    }

    /// `Process(samples, size) -> has_onset`.
    pub fn process(&mut self, samples: &[f32], size: usize) -> bool {
        let mut band0 = [0.0f32; MAX_BLOCK_SIZE];
        let mut band1 = [0.0f32; MAX_BLOCK_SIZE];
        let mut band2 = [0.0f32; MAX_BLOCK_SIZE];

        // Automatic gain control -> band0.
        self.compressor
            .process(&samples[..size], &mut band0[..size]);

        // Split into three bands.
        // mid_high: band0 -> low=band1, high=band2
        self.mid_high_filter
            .split(&band0[..size], &mut band1[..size], &mut band2[..size]);
        // low_mid: band1 -> low=band0, high=band1 (in place)
        self.low_mid_filter
            .split_high_in_place(&mut band1[..size], &mut band0[..size]);

        let bands: [&[f32]; 3] = [&band0[..size], &band1[..size], &band2[..size]];

        let mut onset_df = 0.0f32;
        let mut total_energy = 0.0f32;
        for i in 0..3 {
            let s = bands[i];
            let mut energy = 0.0f32;
            let mut envelope = self.envelope[i];
            let increment = 4usize >> i;
            let mut j = 0;
            while j < size {
                slope(&mut envelope, s[j] * s[j], self.attack[i], self.decay[i]);
                energy += envelope;
                j += increment;
            }
            energy = sqrt(energy) * increment as f32;
            self.envelope[i] = envelope;

            let derivative = energy - self.energy[i];
            onset_df += derivative + derivative.abs();
            self.energy[i] = energy;
            total_energy += energy;
        }

        self.onset_df += 0.05 * (onset_df - self.onset_df);
        let outlier_in_df = self.z_df.test(self.onset_df, 1.0, 0.01);
        let exceeds_energy_threshold = total_energy >= self.inhibit_threshold;
        let not_inhibited = self.inhibit_counter == 0;
        let has_onset = outlier_in_df && exceeds_energy_threshold && not_inhibited;

        if has_onset {
            self.inhibit_threshold = total_energy * 1.5;
            self.inhibit_counter = self.inhibit_time;
        } else {
            self.inhibit_threshold -= self.inhibit_decay * self.inhibit_threshold;
            if self.inhibit_counter != 0 {
                self.inhibit_counter -= 1;
            }
        }
        has_onset
    }
}
