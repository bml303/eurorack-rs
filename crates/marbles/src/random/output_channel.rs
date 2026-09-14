//! `marbles/random/output_channel.h` -- random generation channel.

use stmlib::constrain;

use super::distributions::beta_distribution_sample;
use super::lag_processor::LagProcessor;
use super::quantizer::{Quantizer, Scale};
use super::random_sequence::RandomSequence;
use super::random_stream::RandomStream;

const NUM_REACQUISITIONS: u32 = 20; // 6.4 samples per millisecond

#[derive(Debug, Clone, Copy)]
pub struct ScaleOffset {
    pub scale: f32,
    pub offset: f32,
}

impl ScaleOffset {
    pub fn new(scale: f32, offset: f32) -> Self {
        Self { scale, offset }
    }

    #[inline]
    pub fn apply(&self, x: f32) -> f32 {
        x * self.scale + self.offset
    }
}

impl Default for ScaleOffset {
    fn default() -> Self {
        Self { scale: 1.0, offset: 0.0 }
    }
}

#[derive(Debug, Clone)]
pub struct OutputChannel {
    spread: f32,
    bias: f32,
    steps: f32,
    scale_index: usize,

    register_mode: bool,
    register_value: f32,
    register_transposition: f32,

    previous_steps: f32,
    previous_phase: f32,
    reacquisition_counter: u32,

    previous_voltage: f32,
    voltage: f32,
    quantized_voltage: f32,

    scale_offset: ScaleOffset,

    lag_processor: LagProcessor,

    quantizer: [Quantizer; 6],
}

impl Default for OutputChannel {
    fn default() -> Self {
        Self {
            spread: 0.5,
            bias: 0.5,
            steps: 0.5,
            scale_index: 0,
            register_mode: false,
            register_value: 0.0,
            register_transposition: 0.0,
            previous_steps: 0.0,
            previous_phase: 0.0,
            reacquisition_counter: 0,
            previous_voltage: 0.0,
            voltage: 0.0,
            quantized_voltage: 0.0,
            scale_offset: ScaleOffset::default(),
            lag_processor: LagProcessor::default(),
            quantizer: core::array::from_fn(|_| Quantizer::new()),
        }
    }
}

impl OutputChannel {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn init(&mut self) {
        self.spread = 0.5;
        self.bias = 0.5;
        self.steps = 0.5;
        self.scale_index = 0;

        self.register_mode = false;
        self.register_value = 0.0;
        self.register_transposition = 0.0;

        self.previous_steps = 0.0;
        self.previous_phase = 0.0;
        self.reacquisition_counter = 0;

        self.previous_voltage = 0.0;
        self.voltage = 0.0;
        self.quantized_voltage = 0.0;

        self.scale_offset = ScaleOffset::new(10.0, -5.0);

        self.lag_processor.init();

        let mut scale = Scale::default();
        scale.init();
        for q in self.quantizer.iter_mut() {
            q.init(&scale);
        }
    }

    pub fn load_scale(&mut self, i: usize, scale: &Scale) {
        self.quantizer[i].init(scale);
    }

    pub fn set_spread(&mut self, spread: f32) {
        self.spread = spread;
    }

    pub fn set_bias(&mut self, bias: f32) {
        self.bias = bias;
    }

    pub fn set_scale_index(&mut self, i: usize) {
        self.scale_index = i;
    }

    pub fn set_steps(&mut self, steps: f32) {
        self.steps = steps;
    }

    pub fn set_register_mode(&mut self, register_mode: bool) {
        self.register_mode = register_mode;
    }

    pub fn set_register_value(&mut self, register_value: f32) {
        self.register_value = register_value;
    }

    pub fn set_register_transposition(&mut self, register_transposition: f32) {
        self.register_transposition = register_transposition;
    }

    pub fn set_scale_offset(&mut self, scale_offset: ScaleOffset) {
        self.scale_offset = scale_offset;
    }

    pub fn quantize(&mut self, voltage: f32, amount: f32) -> f32 {
        self.quantizer[self.scale_index].process(voltage, amount, false)
    }

    fn generate_new_voltage(&mut self, random_sequence: &mut RandomSequence, random_stream: &mut RandomStream) -> f32 {
        let u = random_sequence.next_value(random_stream, self.register_mode, self.register_value);

        if self.register_mode {
            10.0 * (u - 0.5) + self.register_transposition
        } else {
            let degenerate_amount = constrain(1.25 - self.spread * 25.0, 0.0, 1.0);
            let bernoulli_amount = constrain(self.spread * 25.0 - 23.75, 0.0, 1.0);

            let mut value = beta_distribution_sample(u, self.spread, self.bias);
            let bernoulli_value = if u >= (1.0 - self.bias) { 0.999999 } else { 0.0 };

            value += degenerate_amount * (self.bias - value);
            value += bernoulli_amount * (bernoulli_value - value);
            self.scale_offset.apply(value)
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn process(
        &mut self,
        random_sequence: &mut RandomSequence,
        random_stream: &mut RandomStream,
        phase: &[f32],
        output: &mut [f32],
        stride: usize,
    ) {
        let size = phase.len();
        // `ParameterInterpolator::new(&mut self.previous_steps, ...)` would
        // hold a live `&mut self.previous_steps` across the loop below,
        // which also calls other `&mut self` methods (`quantize`,
        // `generate_new_voltage`) -- so the ramp is inlined by hand instead.
        let steps_increment = (self.steps - self.previous_steps) / size as f32;
        let mut steps_value = self.previous_steps;

        // This is a horrible hack that wouldn't be here if all the
        // sequencers and MIDI/CV interfaces in this world didn't have
        // *horrible* slew on their CV output (I'm looking at you KORG).
        // Without this hack, the shift register gets its value as soon as
        // the rising edge is observed on the GATE input. Problem: the CV
        // input is probably still slewing up, so we acquire the wrong value
        // in the shift register. What to do then? Over the next 2ms, we'll
        // just track the CV input until it reaches its final value - which
        // means that Marbles output will be slewed too. Another option
        // would have been to wait 2ms between the rising edge and the
        // actual acquisition, but we don't want to penalize people who use
        // tighter sequencers.
        if self.reacquisition_counter != 0 {
            self.reacquisition_counter -= 1;
            let u = random_sequence.rewrite_value(self.register_value);
            self.voltage = 10.0 * (u - 0.5) + self.register_transposition;
            self.quantized_voltage = self.quantize(self.voltage, 2.0 * self.steps - 1.0);
        }

        for i in 0..size {
            steps_value += steps_increment;
            let steps = steps_value;
            if phase[i] < self.previous_phase {
                self.previous_voltage = self.voltage;
                self.voltage = self.generate_new_voltage(random_sequence, random_stream);
                self.lag_processor.reset_ramp();
                self.quantized_voltage = self.quantize(self.voltage, 2.0 * steps - 1.0);
                if self.register_mode {
                    self.reacquisition_counter = NUM_REACQUISITIONS;
                }
            }

            output[i * stride] = if steps >= 0.5 {
                self.quantized_voltage
            } else {
                let smoothness = 1.0 - 2.0 * steps;
                self.lag_processor.process(self.voltage, smoothness, phase[i])
            };
            self.previous_phase = phase[i];
        }
        self.previous_steps = steps_value;
    }
}
