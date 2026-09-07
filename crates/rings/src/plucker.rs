//! `rings/dsp/plucker.h` -- the internal exciter: a short burst of white noise
//! shaped by a comb filter and a low-pass, for Karplus-Strong plucking.

use stmlib::filter::{FilterMode, FrequencyApproximation, Svf};
use stmlib::{DelayLine, Random};

/// `rings::Plucker`.
#[derive(Debug, Clone)]
pub struct Plucker {
    svf: Svf,
    comb_filter: DelayLine<256>,
    remaining_samples: usize,
    comb_filter_period: f32,
    comb_filter_gain: f32,
}

impl Default for Plucker {
    fn default() -> Self {
        Self::new()
    }
}

impl Plucker {
    pub fn new() -> Self {
        let mut p = Self {
            svf: Svf::default(),
            comb_filter: DelayLine::default(),
            remaining_samples: 0,
            comb_filter_period: 0.0,
            comb_filter_gain: 0.0,
        };
        p.init();
        p
    }

    /// `Init`.
    pub fn init(&mut self) {
        self.svf.init();
        self.comb_filter.init();
        self.remaining_samples = 0;
        self.comb_filter_period = 0.0;
    }

    /// `Trigger(frequency, cutoff, position)`.
    pub fn trigger(&mut self, frequency: f32, cutoff: f32, position: f32) {
        let ratio = position * 0.9 + 0.05;
        let mut comb_period = 1.0 / frequency * ratio;
        self.remaining_samples = comb_period as usize;
        while comb_period >= 255.0 {
            comb_period *= 0.5;
        }
        self.comb_filter_period = comb_period;
        self.comb_filter_gain = (1.0 - position) * 0.8;
        self.svf
            .set_f_q(cutoff.min(0.499), 1.0, FrequencyApproximation::Dirty);
    }

    /// `Process(out, size)`.
    pub fn process(&mut self, out: &mut [f32], size: usize) {
        let comb_gain = self.comb_filter_gain;
        let comb_delay = self.comb_filter_period;
        for o in out.iter_mut().take(size) {
            let mut input = 0.0;
            if self.remaining_samples != 0 {
                input = 2.0 * Random::get_float() - 1.0;
                self.remaining_samples -= 1;
            }
            *o = input + comb_gain * self.comb_filter.read_frac(comb_delay);
            self.comb_filter.write(*o);
        }
        self.svf
            .process_in_place(FilterMode::LowPass, &mut out[..size]);
    }
}
