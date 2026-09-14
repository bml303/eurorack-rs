//! `marbles/random/x_y_generator.h` -- generator for the X/Y outputs.

use stmlib::gate_flags::GateFlags;

use crate::ramp::{RampDivider, RampExtractor, Ratio};

use super::output_channel::{OutputChannel, ScaleOffset};
use super::quantizer::Scale;
use super::random_sequence::RandomSequence;
use super::random_stream::RandomStream;
use super::t_generator::Ramps;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoltageRange {
    Narrow,   // +2V
    Positive, // +5V
    Full,     // +/- 5V
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClockSource {
    InternalT1T2T3,
    InternalT1,
    InternalT2,
    InternalT3,
    External,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlMode {
    Identical,
    Bump,
    Tilt,
}

pub const NUM_X_CHANNELS: usize = 3;
pub const NUM_Y_CHANNELS: usize = 1;
pub const NUM_CHANNELS: usize = NUM_X_CHANNELS + NUM_Y_CHANNELS;

#[derive(Debug, Clone, Copy)]
pub struct GroupSettings {
    pub control_mode: ControlMode,
    pub voltage_range: VoltageRange,
    pub register_mode: bool,
    pub register_value: f32,
    pub spread: f32,
    pub bias: f32,
    pub steps: f32,
    pub deja_vu: f32,
    pub scale_index: usize,
    pub length: i32,
    pub ratio: Ratio,
}

impl Default for GroupSettings {
    fn default() -> Self {
        Self {
            control_mode: ControlMode::Identical,
            voltage_range: VoltageRange::Full,
            register_mode: false,
            register_value: 0.0,
            spread: 0.5,
            bias: 0.5,
            steps: 0.5,
            deja_vu: 0.0,
            scale_index: 0,
            length: 1,
            ratio: Ratio::new(1, 1),
        }
    }
}

/// Which of the shared `Ramps` buffers a channel currently reads its phase
/// from.
#[derive(Debug, Clone, Copy)]
enum RampSource {
    Master,
    Slave0,
    Slave1,
}

const HASHES: [u32; NUM_X_CHANNELS] = [0, 0xbeca55e5, 0xf0cacc1a];

#[derive(Clone)]
pub struct XYGenerator {
    random_sequence: [RandomSequence; NUM_CHANNELS],
    output_channel: [OutputChannel; NUM_CHANNELS],
    ramp_extractor: RampExtractor,
    ramp_divider: RampDivider,

    external_clock_stabilization_counter: i32,

    use_shifted_sequences: [bool; NUM_CHANNELS],
}

impl Default for XYGenerator {
    fn default() -> Self {
        Self {
            random_sequence: core::array::from_fn(|_| RandomSequence::default()),
            output_channel: core::array::from_fn(|_| OutputChannel::default()),
            ramp_extractor: RampExtractor::default(),
            ramp_divider: RampDivider::default(),
            external_clock_stabilization_counter: 16,
            use_shifted_sequences: [false; NUM_CHANNELS],
        }
    }
}

impl XYGenerator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn init(&mut self, random_stream: &mut RandomStream, sr: f32) {
        for i in 0..NUM_CHANNELS {
            self.random_sequence[i].init(random_stream);
            self.output_channel[i].init();
        }
        self.ramp_extractor.init(8000.0 / sr);
        self.ramp_divider.init();
        self.external_clock_stabilization_counter = 16;

        self.use_shifted_sequences = [false; NUM_CHANNELS];
    }

    pub fn load_scale_channel(&mut self, channel: usize, scale_index: usize, scale: &Scale) {
        self.output_channel[channel].load_scale(scale_index, scale);
    }

    pub fn load_scale(&mut self, scale_index: usize, scale: &Scale) {
        for i in 0..NUM_X_CHANNELS {
            self.output_channel[i].load_scale(scale_index, scale);
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn process(
        &mut self,
        random_stream: &mut RandomStream,
        clock_source: ClockSource,
        x_settings: &GroupSettings,
        y_settings: &GroupSettings,
        reset: &mut bool,
        external_clock: &[GateFlags],
        ramps: &mut Ramps<'_>,
        output: &mut [f32],
    ) {
        let size = external_clock.len();

        if clock_source != ClockSource::External {
            // For a couple of upcoming blocks, we'll still be receiving
            // garbage from the normalization pin that we need to ignore.
            self.external_clock_stabilization_counter = 16;
        } else if self.external_clock_stabilization_counter != 0 {
            self.external_clock_stabilization_counter -= 1;
            if self.external_clock_stabilization_counter == 0 {
                self.ramp_extractor.reset();
            }
        }

        // Determine which `Ramps` buffer feeds each of the 3 X channels
        // (channel 3, Y, always ends up reading the freshly-computed
        // `ramps.external` -- see below).
        let x_source: [RampSource; NUM_X_CHANNELS] = match clock_source {
            ClockSource::External => {
                let r = Ratio::new(1, 1);
                self.ramp_extractor
                    .process(r, false, reset, external_clock, ramps.slave[0]);
                if self.external_clock_stabilization_counter != 0 {
                    ramps.slave[0][..size].fill(0.0);
                }
                [RampSource::Slave0, RampSource::Slave0, RampSource::Slave0]
            }
            ClockSource::InternalT1 => {
                [RampSource::Slave0, RampSource::Slave0, RampSource::Slave0]
            }
            ClockSource::InternalT2 => [RampSource::Master, RampSource::Master, RampSource::Master],
            ClockSource::InternalT3 => {
                [RampSource::Slave1, RampSource::Slave1, RampSource::Slave1]
            }
            ClockSource::InternalT1T2T3 => {
                [RampSource::Slave0, RampSource::Master, RampSource::Slave1]
            }
        };

        if *reset {
            self.ramp_divider.reset();
        }

        // `x_source[1]` is never `Slave` aliasing `ramps.external` (checked
        // above by construction), so this never reads and writes the same
        // buffer. The field access is written out inline (rather than
        // through a helper taking `&Ramps`) so the borrow checker can see
        // exactly which field of `ramps` each branch borrows, and verify it
        // never conflicts with the `ramps.external` borrow below.
        match x_source[1] {
            RampSource::Master => {
                self.ramp_divider.process(y_settings.ratio, ramps.master, ramps.external);
            }
            RampSource::Slave0 => {
                self.ramp_divider.process(y_settings.ratio, ramps.slave[0], ramps.external);
            }
            RampSource::Slave1 => {
                self.ramp_divider.process(y_settings.ratio, ramps.slave[1], ramps.external);
            }
        }

        // All mutation of `ramps` is done: build shared views for the final,
        // read-only pass below.
        let channel_ramp: [&[f32]; NUM_CHANNELS] = [
            match x_source[0] {
                RampSource::Master => ramps.master,
                RampSource::Slave0 => ramps.slave[0],
                RampSource::Slave1 => ramps.slave[1],
            },
            match x_source[1] {
                RampSource::Master => ramps.master,
                RampSource::Slave0 => ramps.slave[0],
                RampSource::Slave1 => ramps.slave[1],
            },
            match x_source[2] {
                RampSource::Master => ramps.master,
                RampSource::Slave0 => ramps.slave[0],
                RampSource::Slave1 => ramps.slave[1],
            },
            ramps.external,
        ];

        for i in 0..NUM_CHANNELS {
            let settings = if i < NUM_X_CHANNELS { x_settings } else { y_settings };

            match settings.voltage_range {
                VoltageRange::Narrow => {
                    self.output_channel[i].set_scale_offset(ScaleOffset::new(2.0, 0.0));
                }
                VoltageRange::Positive => {
                    self.output_channel[i].set_scale_offset(ScaleOffset::new(5.0, 0.0));
                }
                VoltageRange::Full => {
                    self.output_channel[i].set_scale_offset(ScaleOffset::new(10.0, -5.0));
                }
            }

            let amount = match settings.control_mode {
                ControlMode::Identical => 1.0,
                ControlMode::Bump => {
                    if i == NUM_X_CHANNELS / 2 {
                        1.0
                    } else {
                        -1.0
                    }
                }
                ControlMode::Tilt => 2.0 * i as f32 / (NUM_X_CHANNELS - 1) as f32 - 1.0,
            };

            let channel = &mut self.output_channel[i];
            channel.set_spread(0.5 + (settings.spread - 0.5) * amount);
            channel.set_bias(0.5 + (settings.bias - 0.5) * amount);
            channel.set_steps(
                0.5 + (settings.steps - 0.5) * (if settings.register_mode { 1.0 } else { amount }),
            );
            channel.set_scale_index(settings.scale_index);
            channel.set_register_mode(settings.register_mode);
            channel.set_register_value(settings.register_value);
            channel.set_register_transposition(4.0 * settings.spread * (settings.bias - 0.5) * amount);

            // `Record`/`set_length`/`set_deja_vu`/`Reset` always target
            // `random_sequence_[i]` in the C++, even on iterations that
            // later redirect `sequence` to `random_sequence_[0]` below.
            self.random_sequence[i].record();
            self.random_sequence[i].set_length(settings.length);
            self.random_sequence[i].set_deja_vu(settings.deja_vu);
            if *reset {
                self.random_sequence[i].reset();
            }

            let mut use_shifted_sequences = false;

            // When all channels follow the same clock, the deja-vu random
            // looping will follow the same pattern and the constant-mode
            // input will be shifted!
            let target_idx = if clock_source != ClockSource::InternalT1T2T3
                && i > 0
                && i < NUM_X_CHANNELS
            {
                if settings.register_mode {
                    use_shifted_sequences = true;

                    let shift = match settings.control_mode {
                        ControlMode::Identical => i as u32,
                        ControlMode::Bump => {
                            if i == 2 {
                                1
                            } else {
                                0
                            }
                        }
                        ControlMode::Tilt => 0,
                    };
                    self.random_sequence[0].replay_shifted(shift);
                } else {
                    self.random_sequence[0].replay_pseudo_random(HASHES[i]);
                }
                0
            } else {
                i
            };

            if !use_shifted_sequences && self.use_shifted_sequences[i] && target_idx != 0 {
                let source = self.random_sequence[0].clone();
                self.random_sequence[target_idx].clone_from_sequence(&source);
            }
            self.use_shifted_sequences[i] = use_shifted_sequences;

            self.output_channel[i].process(
                &mut self.random_sequence[target_idx],
                random_stream,
                channel_ramp[i],
                &mut output[i..],
                NUM_CHANNELS,
            );
        }
    }
}
