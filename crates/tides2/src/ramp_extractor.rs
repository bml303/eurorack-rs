//! `tides2/ramp/ramp_extractor.{h,cc}` -- recovers a ramp from a clock input by
//! guessing when the next edge will occur. Prediction strategies:
//! - Moving average of previous intervals.
//! - Periodic rhythmic pattern.
//! - Assume the pulse width is constant, deduce the period from the on-time
//!   and the pulse width.
//!
//! All strategies are tested concurrently and the best-performing one wins (a
//! la early Scheirer/Goto beat trackers).
//!
//! `smooth_audio_rate_tracking` is a C template bool (`ProcessInternal<bool>`)
//! selecting between two largely disjoint bodies; kept as a runtime branch
//! here (see the `mi-tides2` porting note on this pattern).

use stmlib::constrain;
use stmlib::fdsp::{one_pole, slope};
use stmlib::gate_flags::GateFlags;

use crate::ratio::Ratio;

const K_MAX_PATTERN_PERIOD: usize = 8;
const K_HISTORY_SIZE: usize = 16;
const K_PULSE_WIDTH_TOLERANCE: f32 = 0.05;

#[derive(Debug, Clone, Copy, Default)]
struct Pulse {
    on_duration: u32,
    total_duration: u32,
    pulse_width: f32,
}

fn is_within_tolerance(x: f32, y: f32, error: f32) -> bool {
    x >= y * (1.0 - error) && x <= y * (1.0 + error)
}

pub struct RampExtractor {
    current_pulse: usize,
    history: [Pulse; K_HISTORY_SIZE],

    prediction_error: [f32; K_MAX_PATTERN_PERIOD + 1],
    predicted_period: [f32; K_MAX_PATTERN_PERIOD + 1],
    average_pulse_width: f32,

    train_phase: f32,
    frequency_lp: f32,
    frequency: f32,
    target_frequency: f32,
    lp_coefficient: f32,
    period: i32,

    reset_counter: i32,
    max_ramp_value: f32,
    f_ratio: f32,
    max_train_phase: f32,
    reset_interval: u32,

    max_frequency: f32,
    min_period: f32,
    sample_rate: f32,
}

impl Default for RampExtractor {
    fn default() -> Self {
        RampExtractor {
            current_pulse: 0,
            history: [Pulse::default(); K_HISTORY_SIZE],
            prediction_error: [0.0; K_MAX_PATTERN_PERIOD + 1],
            predicted_period: [0.0; K_MAX_PATTERN_PERIOD + 1],
            average_pulse_width: 0.0,
            train_phase: 0.0,
            frequency_lp: 0.0,
            frequency: 0.0,
            target_frequency: 0.0,
            lp_coefficient: 0.0,
            period: 0,
            reset_counter: 0,
            max_ramp_value: 0.0,
            f_ratio: 0.0,
            max_train_phase: 0.0,
            reset_interval: 0,
            max_frequency: 0.0,
            min_period: 0.0,
            sample_rate: 0.0,
        }
    }
}

impl RampExtractor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn init(&mut self, sample_rate: f32, max_frequency: f32) {
        self.max_frequency = max_frequency;
        self.min_period = 1.0 / self.max_frequency;
        self.sample_rate = sample_rate;
        self.reset();
    }

    pub fn reset(&mut self) {
        self.train_phase = 0.0;
        self.target_frequency = 0.1 / self.sample_rate;
        self.frequency_lp = self.target_frequency;
        self.frequency = self.target_frequency;
        self.period = (1.0 / self.frequency) as i32;

        self.lp_coefficient = 0.1;
        self.max_ramp_value = 1.0;
        self.f_ratio = 1.0;
        self.reset_counter = 1;
        self.reset_interval = (self.sample_rate as u32).wrapping_mul(3);

        let p = Pulse {
            on_duration: (self.sample_rate * 0.25) as u32,
            total_duration: (self.sample_rate * 0.5) as u32,
            pulse_width: 0.5,
        };
        self.history = [p; K_HISTORY_SIZE];
        self.current_pulse = 0;
        self.history[self.current_pulse].on_duration = 0;
        self.history[self.current_pulse].total_duration = 0;

        self.average_pulse_width = 0.0;
        self.prediction_error = [50.0; K_MAX_PATTERN_PERIOD + 1];
        self.predicted_period = [self.sample_rate * 0.5; K_MAX_PATTERN_PERIOD + 1];
        self.prediction_error[0] = 0.0;
    }

    fn compute_average_pulse_width(&self, tolerance: f32) -> f32 {
        let mut sum = 0.0;
        for i in 0..K_HISTORY_SIZE {
            if !is_within_tolerance(
                self.history[i].pulse_width,
                self.history[self.current_pulse].pulse_width,
                tolerance,
            ) {
                return 0.0;
            }
            sum += self.history[i].pulse_width;
        }
        sum / K_HISTORY_SIZE as f32
    }

    fn predict_next_period(&mut self) -> f32 {
        let last_period = self.history[self.current_pulse].total_duration as f32;

        let mut best_pattern_period = 0usize;
        for i in 0..=K_MAX_PATTERN_PERIOD {
            let error = self.predicted_period[i] - last_period;
            let error_sq = error * error;
            slope(&mut self.prediction_error[i], error_sq, 0.7, 0.2);

            if i == 0 {
                one_pole(&mut self.predicted_period[0], last_period, 0.5);
            } else {
                let t = self.current_pulse + 1 + K_HISTORY_SIZE - i;
                self.predicted_period[i] = self.history[t % K_HISTORY_SIZE].total_duration as f32;
            }

            if self.prediction_error[i] < self.prediction_error[best_pattern_period] {
                best_pattern_period = i;
            }
        }
        self.predicted_period[best_pattern_period]
    }

    pub fn process(
        &mut self,
        smooth_audio_rate_tracking: bool,
        force_integer_period: bool,
        ratio: Ratio,
        gate_flags: &[GateFlags],
        ramp: &mut [f32],
        size: usize,
    ) -> f32 {
        if smooth_audio_rate_tracking {
            self.process_internal::<true>(force_integer_period, ratio, gate_flags, ramp, size)
        } else {
            self.process_internal::<false>(force_integer_period, ratio, gate_flags, ramp, size)
        }
    }

    fn process_internal<const SMOOTH_AUDIO_RATE_TRACKING: bool>(
        &mut self,
        force_integer_period: bool,
        ratio: Ratio,
        gate_flags: &[GateFlags],
        ramp: &mut [f32],
        size: usize,
    ) -> f32 {
        let block_size = size;
        for i in 0..size {
            let flags = gate_flags[i];
            // We are done with the previous pulse.
            if flags.contains(GateFlags::RISING) {
                let record_pulse = self.history[self.current_pulse].total_duration < self.reset_interval;
                if !record_pulse {
                    self.reset_counter = ratio.q;
                    self.train_phase = 0.0;
                    self.f_ratio = ratio.ratio;
                    self.max_train_phase = ratio.q as f32;
                    self.reset_interval = 4 * self.history[self.current_pulse].total_duration;
                } else {
                    let period = self.history[self.current_pulse].total_duration as f32;
                    if SMOOTH_AUDIO_RATE_TRACKING {
                        let mut no_glide = self.f_ratio != ratio.ratio;
                        self.f_ratio = ratio.ratio;

                        self.reset_counter -= 1;

                        let mut phase_error = 0.0;
                        if self.reset_counter == 0 {
                            self.reset_counter = ratio.q;

                            // Compensates for the latency in the acquisition of
                            // the external signal.
                            let mut expected_phase = 2.0 * block_size as f32 / period * self.f_ratio;
                            while expected_phase >= 1.0 {
                                expected_phase -= 1.0;
                            }
                            phase_error = self.train_phase - expected_phase;
                            if phase_error > 0.5 {
                                phase_error -= 1.0;
                            }
                            if phase_error < -0.5 {
                                phase_error += 1.0;
                            }
                        }

                        let frequency = 1.0 / period;
                        let mut pll_adjustment = 1.0 - self.lp_coefficient * phase_error / self.f_ratio;
                        pll_adjustment = constrain(pll_adjustment, 0.99, 1.01);
                        self.target_frequency = (self.f_ratio * frequency * pll_adjustment).min(0.125);

                        let up_tolerance = (1.02 + 2.0 * frequency) * self.frequency_lp;
                        let down_tolerance = (0.98 - 2.0 * frequency) * self.frequency_lp;
                        no_glide = no_glide
                            || self.target_frequency > up_tolerance
                            || self.target_frequency < down_tolerance;
                        self.lp_coefficient = if no_glide {
                            1.0
                        } else {
                            (period * 0.00001).min(0.1)
                        };
                    } else {
                        // Compute the pulse width of the previous pulse, and
                        // check if the PW has been consistent over the past
                        // pulses.
                        if period < self.min_period {
                            self.frequency = 1.0 / period;
                            self.target_frequency = self.frequency;
                        } else {
                            self.history[self.current_pulse].pulse_width =
                                self.history[self.current_pulse].on_duration as f32 / period;
                            self.average_pulse_width =
                                self.compute_average_pulse_width(K_PULSE_WIDTH_TOLERANCE);
                            if self.history[self.current_pulse].on_duration < 32 {
                                self.average_pulse_width = 0.0;
                            }
                            self.frequency = 1.0 / self.predict_next_period();
                            self.target_frequency = self.frequency;
                        }

                        self.reset_counter -= 1;
                        if self.reset_counter == 0 {
                            self.train_phase = 0.0;
                            self.reset_counter = ratio.q;
                            self.f_ratio = ratio.ratio;
                            self.max_train_phase = ratio.q as f32;
                        } else {
                            let expected = self.max_train_phase - self.reset_counter as f32;
                            let warp = expected - self.train_phase + 1.0;
                            self.frequency *= warp.max(0.01);
                        }
                    }
                    self.reset_interval =
                        (4.0 / self.target_frequency).max(self.sample_rate * 3.0) as u32;
                    self.current_pulse = (self.current_pulse + 1) % K_HISTORY_SIZE;
                }
                // Record a new pulse.
                self.history[self.current_pulse].on_duration = 0;
                self.history[self.current_pulse].total_duration = 0;
            }

            // Update history buffer with total duration and on duration.
            self.history[self.current_pulse].total_duration += 1;
            if flags.contains(GateFlags::HIGH) {
                self.history[self.current_pulse].on_duration += 1;
            }

            if SMOOTH_AUDIO_RATE_TRACKING {
                one_pole(&mut self.frequency_lp, self.target_frequency, self.lp_coefficient);
                if force_integer_period {
                    let new_period = (1.0 / self.frequency_lp) as i32;
                    if (new_period - self.period).abs() > 1 {
                        self.period = new_period;
                        self.frequency = 1.0 / self.period as f32;
                    }
                } else {
                    self.frequency = self.frequency_lp;
                }
                self.train_phase += self.frequency;
                if self.train_phase >= 1.0 {
                    self.train_phase -= 1.0;
                }
                ramp[i] = self.train_phase;
            } else {
                if flags.contains(GateFlags::FALLING) && self.average_pulse_width > 0.0 {
                    let t_on = self.history[self.current_pulse].on_duration as f32;
                    let next = self.max_train_phase - self.reset_counter as f32 + 1.0;
                    let pw = self.average_pulse_width;
                    self.frequency = (next - self.train_phase).max(0.0) * pw / ((1.0 - pw) * t_on);
                }
                self.train_phase += self.frequency;
                if self.train_phase >= self.max_train_phase {
                    self.train_phase = self.max_train_phase;
                }
                let mut phase = self.train_phase * self.f_ratio;
                phase -= phase as i32 as f32;
                ramp[i] = phase;
            }
        }
        if SMOOTH_AUDIO_RATE_TRACKING {
            self.frequency
        } else {
            self.frequency * self.f_ratio
        }
    }
}
