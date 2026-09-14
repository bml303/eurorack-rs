//! `marbles/ramp/ramp_extractor.h` -- recovers a ramp from a clock input by
//! guessing at what time the next edge will occur. Prediction strategies:
//! - Moving average of previous intervals.
//! - Trigram model on quantized intervals.
//! - Periodic rhythmic pattern.
//! - Assume that the pulse width is constant, deduct the period from the on
//!   time and the pulse width.
//!
//! All prediction strategies are concurrently tested, and the output from the
//! best performing one is selected (a la early Scheirer/Goto beat trackers).

use stmlib::fdsp::{one_pole, slope};
use stmlib::gate_flags::GateFlags;

use super::Ratio;

const LOG_ONE_FOURTH: f32 = 1.189207115;
const PULSE_WIDTH_TOLERANCE: f32 = 0.05;
const NUM_CONSISTENT_PULSES: i32 = 3;

const HISTORY_SIZE: usize = 16;
const HASH_TABLE_SIZE: usize = 256;

#[inline]
fn is_within_tolerance(x: f32, y: f32, error: f32) -> bool {
    x >= y * (1.0 - error) && x <= y * (1.0 + error)
}

#[derive(Debug, Clone, Copy, Default)]
struct Pulse {
    on_duration: u32,
    total_duration: u32,
    bucket: u32,
    pulse_width: f32,
}

struct Prediction {
    period: f32,
    // Computed (matching the C++'s `Prediction::accuracy`) but never read
    // again after `predict_next_period` returns -- dead in the original too.
    #[allow(dead_code)]
    accuracy: f32,
}

/// `RampExtractor::Predictor`. The C enum also declares `PERIOD_2` through
/// `PERIOD_10`, but (like this port) never refers to them by name -- every
/// use past `Hash` is positional, via the loop index and
/// `PREDICTOR_PERIOD_1`'s offset -- so only the by-name variants are kept
/// here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
enum Predictor {
    SlowMovingAverage = 0,
    FastMovingAverage,
    Hash,
    Period1,
}
const PREDICTOR_LAST: usize = 13;
const PREDICTOR_PERIOD_1: usize = Predictor::Period1 as usize;

#[derive(Debug, Clone)]
pub struct RampExtractor {
    current_pulse: usize,
    history: [Pulse; HISTORY_SIZE],
    next_bucket: f32,

    prediction_hash_table: [f32; HASH_TABLE_SIZE],
    predicted_period: [f32; PREDICTOR_LAST],
    prediction_accuracy: [f32; PREDICTOR_LAST],
    average_pulse_width: f32,

    train_phase: f32,
    frequency: f32,
    // Declared (matching the C++'s `max_output_phase_`) but never assigned
    // or read -- dead in the original too.
    #[allow(dead_code)]
    max_output_phase: f32,
    max_train_phase: f32,
    reset_frequency: f32,
    target_frequency: f32,
    lp_coefficient: f32,

    f_ratio: f32,
    next_f_ratio: f32,
    next_max_train_phase: f32,
    reset_counter: i32,
    reset_interval: u32,
    audio_rate: bool,
    num_consistent_audio_rate_pulses: i32,

    max_frequency: f32,
    audio_rate_period: f32,
    audio_rate_period_hysteresis: f32,

    reset_at_next_pulse: bool,
}

impl Default for RampExtractor {
    fn default() -> Self {
        Self {
            current_pulse: 0,
            history: [Pulse::default(); HISTORY_SIZE],
            next_bucket: 0.0,
            prediction_hash_table: [0.0; HASH_TABLE_SIZE],
            predicted_period: [0.0; PREDICTOR_LAST],
            prediction_accuracy: [0.0; PREDICTOR_LAST],
            average_pulse_width: 0.0,
            train_phase: 0.0,
            frequency: 0.0,
            max_output_phase: 0.0,
            max_train_phase: 0.0,
            reset_frequency: 0.0,
            target_frequency: 0.0,
            lp_coefficient: 0.0,
            f_ratio: 0.0,
            next_f_ratio: 0.0,
            next_max_train_phase: 0.0,
            reset_counter: 0,
            reset_interval: 0,
            audio_rate: false,
            num_consistent_audio_rate_pulses: 0,
            max_frequency: 0.0,
            audio_rate_period: 0.0,
            audio_rate_period_hysteresis: 0.0,
            reset_at_next_pulse: false,
        }
    }
}

impl RampExtractor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn init(&mut self, max_frequency: f32) {
        self.max_frequency = max_frequency;
        self.audio_rate_period = 1.0 / (100.0 / 32000.0);
        self.audio_rate_period_hysteresis = self.audio_rate_period;
        self.reset();
    }

    pub fn reset(&mut self) {
        self.audio_rate = false;
        self.num_consistent_audio_rate_pulses = 0;
        self.train_phase = 0.0;
        self.frequency = 0.0001;
        self.target_frequency = self.frequency;
        self.lp_coefficient = 0.5;
        self.max_train_phase = 0.999;
        self.next_max_train_phase = self.max_train_phase;
        self.f_ratio = 1.0;
        self.next_f_ratio = self.f_ratio;
        self.reset_counter = 1;
        self.reset_frequency = 0.0;
        self.reset_interval = 32000 * 3;
        self.reset_at_next_pulse = false;

        let p = Pulse {
            bucket: 1,
            on_duration: 2000,
            total_duration: 4000,
            pulse_width: 0.5,
        };
        self.history = [p; HISTORY_SIZE];

        self.current_pulse = 0;
        self.next_bucket = 48.0;

        self.average_pulse_width = 0.0;
        self.predicted_period = [4000.0; PREDICTOR_LAST];
        self.prediction_accuracy = [0.0; PREDICTOR_LAST];
        self.prediction_hash_table = [0.0; HASH_TABLE_SIZE];
    }

    fn compute_average_pulse_width(&self, tolerance: f32) -> f32 {
        let mut sum = 0.0;
        for i in 0..HISTORY_SIZE {
            if !is_within_tolerance(
                self.history[i].pulse_width,
                self.history[self.current_pulse].pulse_width,
                tolerance,
            ) {
                return 0.0;
            }
            sum += self.history[i].pulse_width;
        }
        sum / HISTORY_SIZE as f32
    }

    fn predict_next_period(&mut self) -> Prediction {
        let last_period = self.history[self.current_pulse].total_duration as f32;

        let mut best_predictor = Predictor::FastMovingAverage as usize;

        for i in (Predictor::FastMovingAverage as usize)..PREDICTOR_LAST {
            let error = (self.predicted_period[i] - last_period) / (last_period + 0.01);
            // Scoring function: 10% error is half as good as 0% error.
            let accuracy = 1.0 / (1.0 + 100.0 * error * error);
            // Slowly trust good predictors, quickly demote predictors who make errors.
            slope(&mut self.prediction_accuracy[i], accuracy, 0.1, 0.5);

            // (Ugly code but I don't want virtuals for these.)
            if i == Predictor::SlowMovingAverage as usize {
                one_pole(&mut self.predicted_period[i], last_period, 0.1);
            } else if i == Predictor::FastMovingAverage as usize {
                one_pole(&mut self.predicted_period[i], last_period, 0.5);
            } else if i == Predictor::Hash as usize {
                let t_2 = (self.current_pulse + HISTORY_SIZE - 2) % HISTORY_SIZE;
                let t_1 = (self.current_pulse + HISTORY_SIZE - 1) % HISTORY_SIZE;
                let t_0 = self.current_pulse;

                let hash = self.history[t_1].bucket.wrapping_add(17 * self.history[t_2].bucket);
                one_pole(
                    &mut self.prediction_hash_table[(hash as usize) % HASH_TABLE_SIZE],
                    last_period,
                    0.5,
                );

                let hash = self.history[t_0].bucket.wrapping_add(17 * self.history[t_1].bucket);
                self.predicted_period[i] =
                    self.prediction_hash_table[(hash as usize) % HASH_TABLE_SIZE];
                if self.predicted_period[i] == 0.0 {
                    self.predicted_period[i] = last_period;
                }
            } else {
                // Periodicity detector.
                let candidate_period = i - PREDICTOR_PERIOD_1 + 1;
                let t = self.current_pulse + 1 + HISTORY_SIZE - candidate_period;
                self.predicted_period[i] = self.history[t % HISTORY_SIZE].total_duration as f32;
            }

            if self.prediction_accuracy[i] >= self.prediction_accuracy[best_predictor] {
                best_predictor = i;
            }
        }

        Prediction {
            period: self.predicted_period[best_predictor],
            accuracy: self.prediction_accuracy[best_predictor],
        }
    }

    pub fn process(
        &mut self,
        ratio: Ratio,
        always_ramp_to_maximum: bool,
        reset: &mut bool,
        gate_flags: &[GateFlags],
        ramp: &mut [f32],
    ) {
        if *reset {
            self.reset_at_next_pulse = true;
        }
        for (flags, ramp_sample) in gate_flags.iter().zip(ramp.iter_mut()) {
            let flags = *flags;
            // We are done with the previous pulse.
            if flags.contains(GateFlags::RISING) {
                let record_pulse = self.history[self.current_pulse].total_duration < self.reset_interval;

                if !record_pulse {
                    // Quite a long pause - the clock has probably been stopped
                    // and restarted.
                    self.reset_frequency = 0.0;
                    self.train_phase = 0.0;
                    self.reset_counter = ratio.q;
                    self.reset_interval = 4 * self.history[self.current_pulse].total_duration;

                    // Flag a reset so that everything can be reset downstream.
                    *reset = true;
                } else {
                    if self.reset_at_next_pulse {
                        self.reset_counter = 1;
                        self.reset_at_next_pulse = false;
                    }

                    let period = self.history[self.current_pulse].total_duration as f32;
                    if period <= self.audio_rate_period_hysteresis {
                        self.num_consistent_audio_rate_pulses = i32::min(
                            self.num_consistent_audio_rate_pulses + 1,
                            NUM_CONSISTENT_PULSES,
                        );
                        self.audio_rate_period_hysteresis = self.audio_rate_period * 1.1;
                    } else {
                        self.num_consistent_audio_rate_pulses = 0;
                        self.audio_rate_period_hysteresis = self.audio_rate_period;
                    }

                    // Only switch to audio rate after a consistent number of
                    // uninterrupted audio rate pulses.
                    self.audio_rate =
                        self.num_consistent_audio_rate_pulses == NUM_CONSISTENT_PULSES;
                    if self.audio_rate {
                        self.average_pulse_width = 0.0;

                        let mut no_glide = self.f_ratio != ratio.to_float();
                        self.f_ratio = ratio.to_float();

                        let frequency = 1.0 / period;
                        self.target_frequency = f32::min(self.f_ratio * frequency, self.max_frequency);
                        let up_tolerance = (1.02 + 2.0 * frequency) * self.frequency;
                        let down_tolerance = (0.98 - 2.0 * frequency) * self.frequency;
                        no_glide = no_glide
                            || self.target_frequency > up_tolerance
                            || self.target_frequency < down_tolerance;
                        self.lp_coefficient = if no_glide {
                            1.0
                        } else {
                            f32::min(period * 0.00001, 0.1)
                        };
                    } else {
                        // Compute the pulse width of the previous pulse, and check
                        // if the PW has been consistent over the past pulses.
                        let on_duration = self.history[self.current_pulse].on_duration;
                        self.history[self.current_pulse].pulse_width = on_duration as f32 / period;
                        self.average_pulse_width =
                            self.compute_average_pulse_width(PULSE_WIDTH_TOLERANCE);

                        if on_duration < 32 {
                            self.average_pulse_width = 0.0;
                        }

                        // Try to predict the next interval between pulses. If the
                        // prediction has been reliable over the past pulses, or if
                        // the PW is steady, we'll be able to make reliable
                        // prediction about the time at which the next pulse will
                        // occur.
                        let prediction = self.predict_next_period();
                        self.frequency = 1.0 / prediction.period;

                        self.reset_counter -= 1;
                        if self.reset_counter == 0 {
                            self.next_f_ratio = ratio.to_float() * super::MAX_RAMP_VALUE;
                            self.next_max_train_phase = ratio.q as f32;
                            if always_ramp_to_maximum && self.train_phase < self.max_train_phase {
                                self.reset_frequency =
                                    (0.01 + self.max_train_phase - self.train_phase) * 0.0625;
                            } else {
                                self.reset_frequency = 0.0;
                                self.train_phase = 0.0;
                                self.f_ratio = self.next_f_ratio;
                                self.max_train_phase = self.next_max_train_phase;
                            }
                            self.reset_counter = ratio.q;
                        } else {
                            let expected = self.max_train_phase - self.reset_counter as f32;
                            let warp = expected - self.train_phase + 1.0;
                            self.frequency *= f32::max(warp, 0.01);
                        }
                    }
                    self.reset_interval =
                        f32::max(4.0 / self.target_frequency, 32000.0 * 3.0) as u32;
                    self.current_pulse = (self.current_pulse + 1) % HISTORY_SIZE;
                }
                self.history[self.current_pulse].on_duration = 0;
                self.history[self.current_pulse].total_duration = 0;
                self.history[self.current_pulse].bucket = 0;
                self.next_bucket = 48.0;
            }

            // Update history buffer with total duration and on duration.
            self.history[self.current_pulse].total_duration += 1;
            if flags.contains(GateFlags::HIGH) {
                self.history[self.current_pulse].on_duration += 1;
            }
            if self.history[self.current_pulse].total_duration as f32 >= self.next_bucket {
                self.history[self.current_pulse].bucket += 1;
                self.next_bucket *= LOG_ONE_FOURTH;
            }

            // If the pulse width is constant, and if a clock falling edge is
            // detected, estimate the period using the on time and the pulse
            // width, and correct the phase increment accordingly.
            if flags.contains(GateFlags::FALLING) && self.average_pulse_width > 0.0 {
                let t_on = self.history[self.current_pulse].on_duration as f32;
                let next = self.max_train_phase - self.reset_counter as f32 + 1.0;
                let pw = self.average_pulse_width;
                self.frequency = f32::max(next - self.train_phase, 0.0) * pw / ((1.0 - pw) * t_on);
            }

            if self.audio_rate {
                one_pole(&mut self.frequency, self.target_frequency, self.lp_coefficient);
                self.train_phase += self.frequency;
                if self.train_phase >= 1.0 {
                    self.train_phase -= 1.0;
                }
                *ramp_sample = self.train_phase;
            } else {
                if self.reset_frequency != 0.0 {
                    self.train_phase += self.reset_frequency;
                    if self.train_phase >= self.max_train_phase {
                        self.train_phase = 0.0;
                        self.reset_frequency = 0.0;
                        self.f_ratio = self.next_f_ratio;
                        self.max_train_phase = self.next_max_train_phase;
                    }
                } else {
                    self.train_phase += self.frequency;
                    if self.train_phase >= self.max_train_phase {
                        if self.frequency == self.max_frequency {
                            self.train_phase -= self.max_train_phase;
                        } else {
                            self.train_phase = self.max_train_phase;
                        }
                    }
                }

                let mut output_phase = self.train_phase * self.f_ratio;
                output_phase -= output_phase as i32 as f32;
                *ramp_sample = output_phase;
            }
        }
    }

    pub fn process_no_reset(&mut self, ratio: Ratio, always_ramp_to_maximum: bool, gate_flags: &[GateFlags], ramp: &mut [f32]) {
        let mut reset = false;
        self.process(ratio, always_ramp_to_maximum, &mut reset, gate_flags, ramp);
    }
}
