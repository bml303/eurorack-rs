//! `marbles/random/*` -- the random generation engine: PRNG plumbing,
//! distributions, quantizers, and the T/X/Y generators built from them.

pub mod discrete_distribution_quantizer;
pub mod distributions;
pub mod lag_processor;
pub mod output_channel;
pub mod quantizer;
pub mod random_generator;
pub mod random_sequence;
pub mod random_stream;
pub mod t_generator;
pub mod x_y_generator;

pub use discrete_distribution_quantizer::DiscreteDistributionQuantizer;
pub use lag_processor::LagProcessor;
pub use output_channel::{OutputChannel, ScaleOffset};
pub use quantizer::{Degree, MAX_DEGREES, NUM_THRESHOLDS, Quantizer, Scale};
pub use random_generator::RandomGenerator;
pub use random_sequence::RandomSequence;
pub use random_stream::RandomStream;
pub use t_generator::{
    NUM_T_CHANNELS, Ramps, TGenerator, TGeneratorModel, TGeneratorRange,
};
pub use x_y_generator::{
    ClockSource, ControlMode, GroupSettings, NUM_CHANNELS, NUM_X_CHANNELS, NUM_Y_CHANNELS,
    VoltageRange, XYGenerator,
};
