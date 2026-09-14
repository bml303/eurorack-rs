//! `marbles/random/t_generator.h` -- generator for the T outputs.

use stmlib::HysteresisQuantizer2;
use stmlib::gate_flags::GateFlags;
use stmlib::units::semitones_to_ratio;

use crate::ramp::{RampExtractor, RampGenerator, Ratio, SlaveRamp};
use crate::resources::LUT_LOGIT;

use super::distributions::{beta_distribution_sample, fast_beta_distribution_sample};
use super::random_sequence::RandomSequence;
use super::random_stream::RandomStream;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TGeneratorModel {
    #[default]
    ComplementaryBernoulli,
    Clusters,
    Drums,
    IndependentBernoulli,
    Divider,
    ThreeStates,
    Markov,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TGeneratorRange {
    Range0_25x,
    #[default]
    Range1x,
    Range4x,
}

pub const NUM_T_CHANNELS: usize = 2;
const MARKOV_HISTORY_SIZE: usize = 16;
const NUM_DRUM_PATTERNS: usize = 18;
const DRUM_PATTERN_SIZE: usize = 8;
const NUM_DIVIDER_PATTERNS: usize = 17;
const NUM_INPUT_DIVIDER_RATIOS: usize = 9;

#[derive(Debug, Clone, Copy)]
struct DividerPattern {
    ratios: [Ratio; NUM_T_CHANNELS],
    length: i32,
}

impl DividerPattern {
    const fn new(r0: Ratio, r1: Ratio, length: i32) -> Self {
        Self { ratios: [r0, r1], length }
    }
}

/// `marbles::Ramps` -- the shared buffers threaded through `TGenerator` and
/// `XYGenerator`. Deviation from the C++: there, `Ramps` is a small
/// by-value struct of raw `float*`s, and `XYGenerator::Process` takes it as
/// `const Ramps&` yet still writes through the pointees (C++'s "shallow
/// const" -- top-level const on a struct of pointers doesn't make the
/// pointees const). Rust has no such loophole, so both generators' `process`
/// take `&mut Ramps<'_>` instead of taking (or moving) the struct by value.
pub struct Ramps<'a> {
    pub external: &'a mut [f32],
    pub master: &'a mut [f32],
    pub slave: [&'a mut [f32]; NUM_T_CHANNELS],
}

const DIVIDER_PATTERNS: [DividerPattern; NUM_DIVIDER_PATTERNS] = [
    DividerPattern::new(Ratio::new(1, 1), Ratio::new(1, 1), 1),
    DividerPattern::new(Ratio::new(1, 1), Ratio::new(2, 1), 1),
    DividerPattern::new(Ratio::new(1, 2), Ratio::new(1, 1), 2),
    DividerPattern::new(Ratio::new(1, 1), Ratio::new(4, 1), 1),
    DividerPattern::new(Ratio::new(1, 2), Ratio::new(2, 1), 2),
    DividerPattern::new(Ratio::new(1, 1), Ratio::new(3, 2), 2),
    DividerPattern::new(Ratio::new(1, 4), Ratio::new(4, 1), 4),
    DividerPattern::new(Ratio::new(1, 4), Ratio::new(2, 1), 4),
    DividerPattern::new(Ratio::new(1, 2), Ratio::new(3, 2), 2),
    DividerPattern::new(Ratio::new(1, 1), Ratio::new(8, 1), 1),
    DividerPattern::new(Ratio::new(1, 1), Ratio::new(3, 1), 1),
    DividerPattern::new(Ratio::new(1, 3), Ratio::new(1, 1), 3),
    DividerPattern::new(Ratio::new(1, 1), Ratio::new(5, 4), 4),
    DividerPattern::new(Ratio::new(1, 2), Ratio::new(5, 4), 4),
    DividerPattern::new(Ratio::new(1, 1), Ratio::new(6, 1), 1),
    DividerPattern::new(Ratio::new(1, 3), Ratio::new(2, 1), 3),
    DividerPattern::new(Ratio::new(1, 1), Ratio::new(16, 1), 1),
];

const FIXED_DIVIDER_PATTERNS: [DividerPattern; NUM_DIVIDER_PATTERNS] = [
    DividerPattern::new(Ratio::new(8, 1), Ratio::new(1, 8), 8),
    DividerPattern::new(Ratio::new(6, 1), Ratio::new(1, 6), 6),
    DividerPattern::new(Ratio::new(4, 1), Ratio::new(1, 4), 4),
    DividerPattern::new(Ratio::new(3, 1), Ratio::new(1, 3), 3),
    DividerPattern::new(Ratio::new(2, 1), Ratio::new(1, 2), 2),
    DividerPattern::new(Ratio::new(3, 2), Ratio::new(2, 3), 6),
    DividerPattern::new(Ratio::new(4, 3), Ratio::new(3, 4), 12),
    DividerPattern::new(Ratio::new(5, 4), Ratio::new(4, 5), 20),
    DividerPattern::new(Ratio::new(1, 1), Ratio::new(1, 1), 1),
    DividerPattern::new(Ratio::new(4, 5), Ratio::new(5, 4), 20),
    DividerPattern::new(Ratio::new(3, 4), Ratio::new(4, 3), 12),
    DividerPattern::new(Ratio::new(2, 2), Ratio::new(3, 2), 6),
    DividerPattern::new(Ratio::new(1, 2), Ratio::new(2, 1), 2),
    DividerPattern::new(Ratio::new(1, 3), Ratio::new(3, 1), 3),
    DividerPattern::new(Ratio::new(1, 4), Ratio::new(4, 1), 4),
    DividerPattern::new(Ratio::new(1, 6), Ratio::new(6, 1), 6),
    DividerPattern::new(Ratio::new(1, 8), Ratio::new(8, 1), 8),
];

const INPUT_DIVIDER_RATIOS: [Ratio; NUM_INPUT_DIVIDER_RATIOS] = [
    Ratio::new(1, 4),
    Ratio::new(1, 3),
    Ratio::new(1, 2),
    Ratio::new(2, 3),
    Ratio::new(1, 1),
    Ratio::new(3, 2),
    Ratio::new(2, 1),
    Ratio::new(3, 1),
    Ratio::new(4, 1),
];

#[rustfmt::skip]
const DRUM_PATTERNS: [[u8; DRUM_PATTERN_SIZE]; NUM_DRUM_PATTERNS] = [
    [1, 0, 0, 0, 2, 0, 0, 0],
    [0, 0, 1, 0, 2, 0, 0, 0],

    [1, 0, 1, 0, 2, 0, 0, 0],
    [0, 0, 1, 0, 2, 0, 0, 2],

    [1, 0, 1, 0, 2, 0, 1, 0],
    [0, 2, 1, 0, 2, 0, 0, 2],

    [1, 0, 0, 0, 2, 0, 1, 0],
    [0, 2, 1, 0, 2, 0, 1, 2],

    [1, 0, 0, 1, 2, 0, 0, 0],
    [0, 2, 1, 1, 2, 0, 1, 2],

    [1, 0, 0, 1, 2, 0, 1, 0],
    [0, 2, 1, 1, 2, 2, 1, 2],

    [1, 0, 0, 1, 2, 0, 1, 2],
    [0, 2, 0, 1, 2, 0, 1, 2],

    [1, 0, 1, 1, 2, 0, 1, 2],
    [2, 0, 1, 2, 0, 1, 2, 0],

    [1, 2, 1, 1, 2, 0, 1, 2],
    [2, 0, 1, 2, 0, 1, 2, 2],
];

/// `TGenerator::RandomVector` -- the C++ is a `union` of a named-fields
/// struct and a flat `float[2 * kNumTChannels + 2]`, filled in one shot by
/// `RandomSequence::NextVector`. Ported as a plain struct assembled from
/// that flat buffer (see `fill`), rather than a union + `unsafe` transmute.
#[derive(Debug, Clone, Copy, Default)]
struct RandomVector {
    pulse_width: [f32; NUM_T_CHANNELS],
    u: [f32; NUM_T_CHANNELS],
    p: f32,
    jitter: f32,
}

impl RandomVector {
    fn fill(random_sequence: &mut RandomSequence, random_stream: &mut RandomStream) -> Self {
        let mut x = [0.0f32; 2 * NUM_T_CHANNELS + 2];
        random_sequence.next_vector(random_stream, &mut x);
        Self {
            pulse_width: [x[0], x[1]],
            u: [x[2], x[3]],
            p: x[4],
            jitter: x[5],
        }
    }
}

#[derive(Clone)]
pub struct TGenerator {
    one_hertz: f32,

    model: TGeneratorModel,
    range: TGeneratorRange,

    rate: f32,
    bias: f32,
    jitter: f32,
    pulse_width_mean: f32,
    pulse_width_std: f32,

    master_phase: f32,
    jitter_multiplier: f32,
    phase_difference: f32,
    previous_external_ramp_value: f32,

    use_external_clock: bool,

    divider_pattern_length: i32,
    streak_counter: [i32; MARKOV_HISTORY_SIZE],
    markov_history: [i32; MARKOV_HISTORY_SIZE],
    markov_history_ptr: usize,
    drum_pattern_step: usize,
    drum_pattern_index: usize,

    sequence: RandomSequence,
    ramp_extractor: RampExtractor,
    ramp_generator: RampGenerator,

    slave_ramp: [SlaveRamp; NUM_T_CHANNELS],

    bias_quantizer: HysteresisQuantizer2,
    rate_quantizer: HysteresisQuantizer2,
}

impl Default for TGenerator {
    fn default() -> Self {
        Self {
            one_hertz: 0.0,
            model: TGeneratorModel::default(),
            range: TGeneratorRange::default(),
            rate: 0.0,
            bias: 0.5,
            jitter: 0.0,
            pulse_width_mean: 0.0,
            pulse_width_std: 0.0,
            master_phase: 0.0,
            jitter_multiplier: 1.0,
            phase_difference: 0.0,
            previous_external_ramp_value: 0.0,
            use_external_clock: false,
            divider_pattern_length: 0,
            streak_counter: [0; MARKOV_HISTORY_SIZE],
            markov_history: [0; MARKOV_HISTORY_SIZE],
            markov_history_ptr: 0,
            drum_pattern_step: 0,
            drum_pattern_index: 0,
            sequence: RandomSequence::default(),
            ramp_extractor: RampExtractor::default(),
            ramp_generator: RampGenerator::default(),
            slave_ramp: [SlaveRamp::default(), SlaveRamp::default()],
            bias_quantizer: HysteresisQuantizer2::default(),
            rate_quantizer: HysteresisQuantizer2::default(),
        }
    }
}

impl TGenerator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn init(&mut self, random_stream: &mut RandomStream, sr: f32) {
        self.one_hertz = 1.0 / sr;
        self.model = TGeneratorModel::ComplementaryBernoulli;
        self.range = TGeneratorRange::Range1x;

        self.rate = 0.0;
        self.bias = 0.5;
        self.jitter = 0.0;
        self.pulse_width_mean = 0.0;
        self.pulse_width_std = 0.0;

        self.master_phase = 0.0;
        self.jitter_multiplier = 1.0;
        self.phase_difference = 0.0;
        self.previous_external_ramp_value = 0.0;

        self.divider_pattern_length = 0;
        self.streak_counter = [0; MARKOV_HISTORY_SIZE];
        self.markov_history = [0; MARKOV_HISTORY_SIZE];
        self.markov_history_ptr = 0;
        self.drum_pattern_step = 0;
        self.drum_pattern_index = 0;

        self.sequence.init(random_stream);
        self.ramp_extractor.init(1000.0 / sr);
        self.ramp_generator.init();
        for r in self.slave_ramp.iter_mut() {
            r.init();
        }
        // Ideal Hysteresis: voltage error / voltage range * num_values, or
        // 1 / 2^(adc_reliable_bits) * num_values.
        self.bias_quantizer.init(NUM_DIVIDER_PATTERNS as i32, 0.1, false);
        self.rate_quantizer.init(NUM_INPUT_DIVIDER_RATIOS as i32, 0.05, false);

        self.use_external_clock = false;
    }

    pub fn set_model(&mut self, model: TGeneratorModel) {
        self.model = model;
    }

    pub fn set_range(&mut self, range: TGeneratorRange) {
        self.range = range;
    }

    pub fn set_rate(&mut self, rate: f32) {
        self.rate = rate;
    }

    pub fn set_bias(&mut self, bias: f32) {
        self.bias = bias;
    }

    pub fn set_jitter(&mut self, jitter: f32) {
        self.jitter = jitter;
    }

    pub fn set_deja_vu(&mut self, deja_vu: f32) {
        self.sequence.set_deja_vu(deja_vu);
    }

    pub fn set_length(&mut self, length: i32) {
        self.sequence.set_length(length);
    }

    pub fn set_pulse_width_mean(&mut self, pulse_width_mean: f32) {
        self.pulse_width_mean = pulse_width_mean;
    }

    pub fn set_pulse_width_std(&mut self, pulse_width_std: f32) {
        self.pulse_width_std = pulse_width_std;
    }

    fn random_pulse_width(&self, u: f32) -> f32 {
        if self.pulse_width_std == 0.0 {
            0.05 + 0.9 * self.pulse_width_mean
        } else {
            0.05 + 0.9 * beta_distribution_sample(u, self.pulse_width_std, self.pulse_width_mean)
        }
    }

    fn generate_complementary_bernoulli(&self, x: &RandomVector) -> u32 {
        let mut bitmask = 0u32;
        for i in 0..NUM_T_CHANNELS {
            if (x.u[i >> 1] > self.bias) ^ (i & 1 != 0) {
                bitmask |= 1 << i;
            }
        }
        bitmask
    }

    fn generate_independent_bernoulli(&self, x: &RandomVector) -> u32 {
        let mut bitmask = 0u32;
        for i in 0..NUM_T_CHANNELS {
            if (x.u[i] > self.bias) ^ (i & 1 != 0) {
                bitmask |= 1 << i;
            }
        }
        bitmask
    }

    fn generate_three_states(&self, x: &RandomVector) -> u32 {
        let mut bitmask = 0u32;
        let p_none = 0.75 - (self.bias - 0.5).abs();
        let threshold = p_none + (1.0 - p_none) * (0.25 + self.bias * 0.5);

        for i in 0..NUM_T_CHANNELS {
            let u = x.u[i >> 1];
            if u > p_none && ((u > threshold) ^ (i & 1 != 0)) {
                bitmask |= 1 << i;
            }
        }
        bitmask
    }

    fn generate_drums(&mut self, x: &RandomVector) -> u32 {
        self.drum_pattern_step += 1;
        if self.drum_pattern_step >= DRUM_PATTERN_SIZE {
            self.drum_pattern_step = 0;
            let u = x.u[0] * 2.0 * (self.bias - 0.5).abs();
            self.drum_pattern_index = (NUM_DRUM_PATTERNS as f32 * u) as usize;
            if self.bias <= 0.5 {
                self.drum_pattern_index -= self.drum_pattern_index % 2;
            }
        }
        DRUM_PATTERNS[self.drum_pattern_index][self.drum_pattern_step] as u32
    }

    fn generate_markov(&mut self, x: &RandomVector) -> u32 {
        let mut bitmask = 0u32;
        let b = 1.5 * self.bias - 0.5;
        self.markov_history[self.markov_history_ptr] = 0;
        let p = self.markov_history_ptr;
        for i in 0..NUM_T_CHANNELS {
            let mask = 1i32 << i;
            // 4 rules:
            // * We favor repeating what we played 8 ticks ago.
            // * We do not favor pulses appearing on both channels.
            // * We favor sparse patterns (no consecutive hits).
            // * We favor patterns in which one channel "echoes" what the
            //   other channel played 4 ticks before.
            let periodic = self.markov_history[(p + 8) % MARKOV_HISTORY_SIZE] & mask != 0;
            let simultaneous = self.markov_history[(p + 8) % MARKOV_HISTORY_SIZE] & !mask != 0;
            let dense = self.markov_history[(p + 1) % MARKOV_HISTORY_SIZE] & mask != 0;
            let alternate = self.markov_history[(p + 4) % MARKOV_HISTORY_SIZE] & !mask != 0;

            let mut logit = -1.5;
            logit += if self.streak_counter[i] > 24 { 10.0 } else { 0.0 };
            logit += 8.0 * b.abs() * (if periodic { b } else { -b });
            logit -= 2.0 * (if simultaneous { b } else { -b });
            logit -= if dense { b } else { 0.0 };
            logit += if alternate { b } else { 0.0 };
            let logit = logit.clamp(-10.0, 10.0);
            let probability = LUT_LOGIT[(logit * 12.8 + 128.0) as usize];
            let mut state = x.u[i] < probability;

            if self.sequence.deja_vu() >= x.p {
                state = self.markov_history[(p + self.sequence.length() as usize) % MARKOV_HISTORY_SIZE]
                    & mask
                    != 0;
            }
            if state {
                bitmask |= mask as u32;
                self.streak_counter[i] = 0;
            } else {
                self.streak_counter[i] += 1;
            }
        }
        self.markov_history[p] |= bitmask as i32;
        self.markov_history_ptr = (p + MARKOV_HISTORY_SIZE - 1) % MARKOV_HISTORY_SIZE;
        bitmask
    }

    fn schedule_output_pulses(&mut self, x: &RandomVector, mut bitmask: u32) {
        for i in 0..NUM_T_CHANNELS {
            let pw = self.random_pulse_width(x.pulse_width[i]);
            self.slave_ramp[i].init_bernoulli(bitmask & 1 != 0, pw, 0.5);
            bitmask >>= 1;
        }
    }

    fn configure_slave_ramps(&mut self, x: &RandomVector) {
        match self.model {
            // Generate a bitmask that will describe which outputs are
            // active at this clock tick. Use this bitmask to actually
            // schedule pulses on the outputs.
            TGeneratorModel::ComplementaryBernoulli => {
                let bitmask = self.generate_complementary_bernoulli(x);
                self.schedule_output_pulses(x, bitmask);
            }
            TGeneratorModel::IndependentBernoulli => {
                let bitmask = self.generate_independent_bernoulli(x);
                self.schedule_output_pulses(x, bitmask);
            }
            TGeneratorModel::ThreeStates => {
                let bitmask = self.generate_three_states(x);
                self.schedule_output_pulses(x, bitmask);
            }
            TGeneratorModel::Drums => {
                let bitmask = self.generate_drums(x);
                self.schedule_output_pulses(x, bitmask);
            }
            TGeneratorModel::Markov => {
                let bitmask = self.generate_markov(x);
                self.schedule_output_pulses(x, bitmask);
            }
            TGeneratorModel::Clusters | TGeneratorModel::Divider => {
                self.divider_pattern_length -= 1;
                if self.divider_pattern_length <= 0 {
                    let pattern = if self.model == TGeneratorModel::Divider {
                        let idx = self.bias_quantizer.process(self.bias);
                        FIXED_DIVIDER_PATTERNS[idx as usize]
                    } else {
                        let strength = (self.bias - 0.5).abs() * 2.0;
                        let mut u = x.u[0];
                        u *= u + strength * strength * (1.0 - u);
                        u *= strength;
                        let mut pattern =
                            DIVIDER_PATTERNS[(u * NUM_DIVIDER_PATTERNS as f32) as usize];
                        if self.bias < 0.5 {
                            for i in 0..NUM_T_CHANNELS / 2 {
                                pattern.ratios.swap(i, NUM_T_CHANNELS - 1 - i);
                            }
                        }
                        pattern
                    };
                    for i in 0..NUM_T_CHANNELS {
                        let pw = self.random_pulse_width(x.pulse_width[i]);
                        self.slave_ramp[i].init_divided(pattern.length, pattern.ratios[i], pw);
                    }
                    self.divider_pattern_length = pattern.length;
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn process(
        &mut self,
        random_stream: &mut RandomStream,
        use_external_clock: bool,
        reset: &mut bool,
        external_clock: &[GateFlags],
        ramps: &mut Ramps<'_>,
        gate: &mut [bool],
    ) {
        let size = external_clock.len();

        let internal_frequency = if use_external_clock {
            if !self.use_external_clock {
                self.ramp_extractor.reset();
            }

            let mut ratio = INPUT_DIVIDER_RATIOS[self
                .rate_quantizer
                .process(1.05 * self.rate / 96.0 + 0.5)
                as usize];
            if self.range == TGeneratorRange::Range0_25x {
                ratio.q *= 4;
            } else if self.range == TGeneratorRange::Range4x {
                ratio.p *= 4;
            }
            ratio.simplify(2);
            self.ramp_extractor.process(
                ratio,
                true,
                reset,
                external_clock,
                ramps.external,
            );
            0.0
        } else {
            let rate = if self.range == TGeneratorRange::Range4x {
                8.0
            } else if self.range == TGeneratorRange::Range0_25x {
                0.5
            } else {
                2.0
            };
            rate * self.one_hertz * semitones_to_ratio(self.rate)
        };

        self.use_external_clock = use_external_clock;

        if *reset {
            for r in self.slave_ramp.iter_mut() {
                r.reset();
            }
            self.sequence.reset();

            self.divider_pattern_length = 0;
            self.drum_pattern_step = DRUM_PATTERN_SIZE;
            if self.model != TGeneratorModel::Divider {
                let random_vector = RandomVector::fill(&mut self.sequence, random_stream);
                self.configure_slave_ramps(&random_vector);
            }
        }

        for i in 0..size {
            let mut frequency = if use_external_clock {
                ramps.external[i] - self.previous_external_ramp_value
            } else {
                internal_frequency
            };
            if frequency < 0.0 {
                frequency += 1.0;
            }

            let jittery_frequency = frequency * self.jitter_multiplier;
            self.master_phase += jittery_frequency;
            self.phase_difference += frequency - jittery_frequency;

            if self.master_phase > 1.0 {
                self.master_phase -= 1.0;
                let random_vector = RandomVector::fill(&mut self.sequence, random_stream);

                let jitter_amount = self.jitter * self.jitter * self.jitter * self.jitter * 36.0;
                let x = fast_beta_distribution_sample(random_vector.jitter);
                let mut multiplier = semitones_to_ratio((x * 2.0 - 1.0) * jitter_amount);

                // This step is crucial in making sure that the jittered
                // clock does not deviate too much from the master clock.
                // The larger the phase difference between the two, the more
                // likely the jittery clock will speed up or down to catch
                // up with the straight clock.
                multiplier *= if self.phase_difference > 0.0 {
                    1.0 + self.phase_difference
                } else {
                    1.0 / (1.0 - self.phase_difference)
                };

                self.jitter_multiplier = multiplier;
                self.configure_slave_ramps(&random_vector);
            }

            if internal_frequency != 0.0 {
                ramps.external[i] = self.master_phase;
            }

            self.previous_external_ramp_value = ramps.external[i];
            ramps.master[i] = self.master_phase;
            for j in 0..NUM_T_CHANNELS {
                let (phase, g) = self.slave_ramp[j].process(frequency * self.jitter_multiplier);
                ramps.slave[j][i] = phase;
                gate[i * NUM_T_CHANNELS + j] = g;
            }
        }
    }
}
