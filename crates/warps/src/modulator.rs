//! `warps/dsp/modulator.{h,cc}` -- the top-level engine: VCA/saturation on
//! the two input channels, carrier rendering (cross-modulation oscillator /
//! vocoder oscillator / a blend of both), the 6 cross-modulation algorithms
//! (crossfaded pairwise as `modulation_algorithm` sweeps `[0, 8)`, at x6
//! oversampling) or the vocoder, and the "Disastrous Peace"-style easter egg
//! (a frequency shifter built from a quadrature Hilbert transform).
//!
//! Deviations from the C++: the C reuses 2 physical buffers for 4 logically
//! distinct roles purely to save RAM on the STM32F3 (`buffer_[0]` doubles as
//! both `carrier` and, later in the same call, `main_output`;
//! `src_buffer_[0]` doubles as both `oversampled_carrier` and, later,
//! `oversampled_output`) -- verified sample-by-sample that every case is a
//! "fully consumed before reused" or "read-then-immediately-overwritten"
//! pattern, never simultaneous aliasing that changes the result, so this
//! port just gives each role its own array (same "drop the packed-buffer
//! memory trick" call as `mi-grids`/`filter_bank.rs`).

use stmlib::fdsp::{clip16, interpolate, one_pole, soft_clip, soft_limit};
use stmlib::parameter_interpolator::ParameterInterpolator;
use stmlib::units::semitones_to_ratio;

use crate::oscillator::Oscillator;
use crate::parameters::{OscillatorShape, Parameters};
use crate::quadrature_oscillator::QuadratureOscillator;
use crate::quadrature_transform::QuadratureTransform;
use crate::resources::{LUT_AP_POLES, LUT_SIN, LUT_XFADE_IN, LUT_XFADE_OUT};
use crate::sample_rate_converter::{SampleRateConverterDown, SampleRateConverterUp, SRC_DOWN_6_48, SRC_UP_6_48};
use crate::vocoder::Vocoder;

pub const K_MAX_BLOCK_SIZE: usize = 96;
pub const K_OVERSAMPLING: usize = 6;

#[derive(Debug, Clone, Copy, Default)]
pub struct ShortFrame {
    pub l: i16,
    pub r: i16,
}

#[derive(Debug, Clone, Copy, Default)]
struct SaturatingAmplifier {
    level: f32,
    drive: f32,
    post_gain: f32,
    pre_gain: f32,
}

impl SaturatingAmplifier {
    fn init(&mut self) {
        self.drive = 0.0;
    }

    /// `input` is already de-interleaved (the C reads through `in_stride`
    /// directly from the raw `ShortFrame` array; the call site here
    /// extracts the channel into a contiguous buffer first).
    fn process(&mut self, drive: f32, limit: f32, input: &[i16], out: &mut [f32], out_raw: &mut [f32]) {
        let size = out.len();
        {
            let mut drive_modulation = ParameterInterpolator::new(&mut self.drive, drive, size);
            let mut level = self.level;
            for i in 0..size {
                let s0 = input[i] as f32 / 32768.0;
                let error = s0 * s0 - level;
                level += error * (if error > 0.0 { 0.1 } else { 0.0001 });
                let s = if level <= 0.0001 { s0 * (1.0 / 0.0001) * level } else { s0 };
                out[i] = s;
                out_raw[i] += s * drive_modulation.next();
            }
            self.level = level;
        }

        let drive_2 = drive * drive;
        let pre_gain_a = drive * 0.5;
        let pre_gain_b = drive_2 * drive_2 * drive * 24.0;
        let pre_gain = pre_gain_a + (pre_gain_b - pre_gain_a) * drive_2;
        let drive_squished = drive * (2.0 - drive);
        let post_gain = 1.0 / soft_clip(0.33 + drive_squished * (pre_gain - 0.33));

        let mut pre_gain_modulation = ParameterInterpolator::new(&mut self.pre_gain, pre_gain, size);
        let mut post_gain_modulation = ParameterInterpolator::new(&mut self.post_gain, post_gain, size);
        for s in out.iter_mut() {
            let pre = pre_gain_modulation.next() * *s;
            let post = soft_clip(pre) * post_gain_modulation.next();
            *s = pre + (post - pre) * limit;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum XmodAlgorithm {
    Xfade,
    Fold,
    AnalogRingModulation,
    DigitalRingModulation,
    Xor,
    Comparator,
    Nop,
}

/// `xmod_table_[]` -- the 6 adjacent-algorithm pairs `ProcessXmod` crossfades
/// between as `modulation_algorithm` sweeps `[0, 8)`.
const XMOD_TABLE: [(XmodAlgorithm, XmodAlgorithm); 6] = [
    (XmodAlgorithm::Xfade, XmodAlgorithm::Fold),
    (XmodAlgorithm::Fold, XmodAlgorithm::AnalogRingModulation),
    (XmodAlgorithm::AnalogRingModulation, XmodAlgorithm::DigitalRingModulation),
    (XmodAlgorithm::DigitalRingModulation, XmodAlgorithm::Xor),
    (XmodAlgorithm::Xor, XmodAlgorithm::Comparator),
    (XmodAlgorithm::Comparator, XmodAlgorithm::Nop),
];

/// Approximation of diode non-linearity from Julian Parker, "A simple
/// Digital model of the diode-based ring-modulator" (DAFx-11).
#[inline]
fn diode(x: f32) -> f32 {
    let sign = if x > 0.0 { 1.0 } else { -1.0 };
    let mut dead_zone = x.abs() - 0.667;
    dead_zone += dead_zone.abs();
    dead_zone *= dead_zone;
    0.043_247_66 * dead_zone * sign
}

fn xmod(algorithm: XmodAlgorithm, x_1: f32, x_2: f32, parameter: f32) -> f32 {
    match algorithm {
        XmodAlgorithm::Xfade => {
            let fade_in = interpolate(&LUT_XFADE_IN, parameter, 256.0);
            let fade_out = interpolate(&LUT_XFADE_OUT, parameter, 256.0);
            x_1 * fade_in + x_2 * fade_out
        }
        XmodAlgorithm::Fold => {
            let mut sum = 0.0;
            sum += x_1;
            sum += x_2;
            sum += x_1 * x_2 * 0.25;
            sum *= 0.02 + parameter;
            const K_SCALE: f32 = 2048.0 / ((1.0 + 1.0 + 0.25) * 1.02);
            interpolate(&crate::resources::LUT_BIPOLAR_FOLD[2048..], sum, K_SCALE)
        }
        XmodAlgorithm::AnalogRingModulation => {
            let modulator = x_1;
            let carrier = x_2 * 2.0;
            let mut ring = diode(modulator + carrier) + diode(modulator - carrier);
            ring *= 4.0 + parameter * 24.0;
            soft_limit(ring)
        }
        XmodAlgorithm::DigitalRingModulation => {
            let ring = 4.0 * x_1 * x_2 * (1.0 + parameter * 8.0);
            ring / (1.0 + ring.abs())
        }
        XmodAlgorithm::Xor => {
            let x_1_short = clip16((x_1 * 32768.0) as i32) as i16;
            let x_2_short = clip16((x_2 * 32768.0) as i32) as i16;
            let modulated = (x_1_short ^ x_2_short) as f32 / 32768.0;
            let sum = (x_1 + x_2) * 0.7;
            sum + (modulated - sum) * parameter
        }
        XmodAlgorithm::Comparator => {
            let modulator = x_1;
            let carrier = x_2;
            let x = parameter * 2.995;
            let x_integral = x as i32;
            let x_fractional = x - x_integral as f32;

            let direct = if modulator < carrier { modulator } else { carrier };
            let window = if modulator.abs() > carrier.abs() { modulator } else { carrier };
            let window_2 = if modulator.abs() > carrier.abs() { modulator.abs() } else { -carrier.abs() };
            let threshold = if carrier > 0.05 { carrier } else { modulator };

            let sequence = [direct, threshold, window, window_2];
            let a = sequence[x_integral as usize];
            let b = sequence[x_integral as usize + 1];
            a + (b - a) * x_fractional
        }
        XmodAlgorithm::Nop => x_1,
    }
}

#[allow(clippy::too_many_arguments)]
fn process_xmod(
    algorithm_1: XmodAlgorithm,
    algorithm_2: XmodAlgorithm,
    balance: f32,
    balance_end: f32,
    parameter: f32,
    parameter_end: f32,
    in_1: &[f32],
    in_2: &[f32],
    out: &mut [f32],
) {
    let size = out.len();
    let step = 1.0 / size as f32;
    let parameter_increment = (parameter_end - parameter) * step;
    let balance_increment = (balance_end - balance) * step;
    let mut parameter = parameter;
    let mut balance = balance;
    for i in 0..size {
        let x_1 = in_1[i];
        let x_2 = in_2[i];
        let a = xmod(algorithm_1, x_1, x_2, parameter);
        let b = xmod(algorithm_2, x_1, x_2, parameter);
        out[i] = a + (b - a) * balance;
        parameter += parameter_increment;
        balance += balance_increment;
    }
}

pub struct Modulator {
    bypass: bool,
    easter_egg: bool,

    parameters: Parameters,
    previous_parameters: Parameters,

    amplifier: [SaturatingAmplifier; 2],
    xmod_oscillator: Oscillator,
    vocoder_oscillator: Oscillator,
    quadrature_oscillator: QuadratureOscillator,

    src_up: [SampleRateConverterUp<8>; 2],
    src_down: SampleRateConverterDown<48>,

    vocoder: Vocoder,
    quadrature_transform: [QuadratureTransform; 2],

    internal_modulation: [f32; K_MAX_BLOCK_SIZE],

    carrier: [f32; K_MAX_BLOCK_SIZE],
    modulator_buf: [f32; K_MAX_BLOCK_SIZE],
    main_output: [f32; K_MAX_BLOCK_SIZE],
    aux_output: [f32; K_MAX_BLOCK_SIZE],
    carrier_i: [f32; K_MAX_BLOCK_SIZE],
    carrier_q: [f32; K_MAX_BLOCK_SIZE],

    oversampled_carrier: [f32; K_MAX_BLOCK_SIZE * K_OVERSAMPLING],
    oversampled_modulator: [f32; K_MAX_BLOCK_SIZE * K_OVERSAMPLING],
    oversampled_output: [f32; K_MAX_BLOCK_SIZE * K_OVERSAMPLING],

    feedback_sample: f32,
}

impl Default for Modulator {
    fn default() -> Self {
        Self {
            bypass: false,
            easter_egg: false,
            parameters: Parameters::default(),
            previous_parameters: Parameters::default(),
            amplifier: [SaturatingAmplifier::default(); 2],
            xmod_oscillator: Oscillator::default(),
            vocoder_oscillator: Oscillator::default(),
            quadrature_oscillator: QuadratureOscillator::default(),
            src_up: [SampleRateConverterUp::new(K_OVERSAMPLING, &SRC_UP_6_48), SampleRateConverterUp::new(K_OVERSAMPLING, &SRC_UP_6_48)],
            src_down: SampleRateConverterDown::new(K_OVERSAMPLING, &SRC_DOWN_6_48),
            vocoder: Vocoder::default(),
            quadrature_transform: [QuadratureTransform::default(), QuadratureTransform::default()],
            internal_modulation: [0.0; K_MAX_BLOCK_SIZE],
            carrier: [0.0; K_MAX_BLOCK_SIZE],
            modulator_buf: [0.0; K_MAX_BLOCK_SIZE],
            main_output: [0.0; K_MAX_BLOCK_SIZE],
            aux_output: [0.0; K_MAX_BLOCK_SIZE],
            carrier_i: [0.0; K_MAX_BLOCK_SIZE],
            carrier_q: [0.0; K_MAX_BLOCK_SIZE],
            oversampled_carrier: [0.0; K_MAX_BLOCK_SIZE * K_OVERSAMPLING],
            oversampled_modulator: [0.0; K_MAX_BLOCK_SIZE * K_OVERSAMPLING],
            oversampled_output: [0.0; K_MAX_BLOCK_SIZE * K_OVERSAMPLING],
            feedback_sample: 0.0,
        }
    }
}

impl Modulator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn init(&mut self, sample_rate: f32) {
        self.bypass = false;
        self.easter_egg = false;

        for i in 0..2 {
            self.amplifier[i].init();
            self.src_up[i].init();
            self.quadrature_transform[i].init(&LUT_AP_POLES);
        }
        self.src_down.init();

        self.xmod_oscillator.init(sample_rate);
        self.vocoder_oscillator.init(sample_rate);
        self.quadrature_oscillator.init(sample_rate);
        self.vocoder.init(sample_rate);

        self.previous_parameters.carrier_shape = 0;
        self.previous_parameters.channel_drive = [0.0, 0.0];
        self.previous_parameters.modulation_algorithm = 0.0;
        self.previous_parameters.modulation_parameter = 0.0;
        self.previous_parameters.note = 48.0;

        self.feedback_sample = 0.0;
    }

    pub fn mutable_parameters(&mut self) -> &mut Parameters {
        &mut self.parameters
    }
    pub fn parameters(&self) -> &Parameters {
        &self.parameters
    }

    pub fn bypass(&self) -> bool {
        self.bypass
    }
    pub fn set_bypass(&mut self, bypass: bool) {
        self.bypass = bypass;
    }

    pub fn easter_egg(&self) -> bool {
        self.easter_egg
    }
    pub fn set_easter_egg(&mut self, easter_egg: bool) {
        self.easter_egg = easter_egg;
    }

    pub fn process(&mut self, input: &[ShortFrame], output: &mut [ShortFrame], size: usize) {
        if self.bypass {
            output[..size].copy_from_slice(&input[..size]);
            return;
        } else if self.easter_egg {
            self.process_easter_egg(input, output, size);
            return;
        }

        // 0.0: use cross-modulation algorithms. 1.0: use vocoder.
        let vocoder_amount = ((self.parameters.modulation_algorithm - 0.7) * 20.0 + 0.5).clamp(0.0, 1.0);

        if self.parameters.carrier_shape == 0 {
            self.aux_output[..size].fill(0.0);
        }

        // Convert audio inputs to float and apply VCA/saturation.
        let start = if self.parameters.carrier_shape != 0 { 1 } else { 0 };
        for i in start..2 {
            let mut channel = [0i16; K_MAX_BLOCK_SIZE];
            for j in 0..size {
                channel[j] = if i == 0 { input[j].l } else { input[j].r };
            }
            let drive = self.parameters.channel_drive[i];
            let limit = 1.0 - vocoder_amount;
            let mut local_out = [0.0f32; K_MAX_BLOCK_SIZE];
            self.amplifier[i].process(drive, limit, &channel[..size], &mut local_out[..size], &mut self.aux_output[..size]);
            if i == 0 {
                self.carrier[..size].copy_from_slice(&local_out[..size]);
            } else {
                self.modulator_buf[..size].copy_from_slice(&local_out[..size]);
            }
        }

        // If necessary, render carrier. Otherwise, the amplifier loop above
        // already summed signals 1 and 2 into `aux_output`.
        if self.parameters.carrier_shape != 0 {
            for (m, frame) in self.internal_modulation[..size].iter_mut().zip(input[..size].iter()) {
                *m = frame.l as f32 / 32768.0;
            }
            let xmod_shape = OscillatorShape::from_index(self.parameters.carrier_shape - 1);
            let vocoder_shape = OscillatorShape::from_index(self.parameters.carrier_shape + 1);

            const K_XMOD_CARRIER_GAIN: f32 = 0.5;

            if vocoder_amount == 0.0 {
                self.xmod_oscillator.render(xmod_shape, self.parameters.note, &self.internal_modulation[..size], &mut self.aux_output[..size]);
                for j in 0..size {
                    self.carrier[j] = self.aux_output[j] * K_XMOD_CARRIER_GAIN;
                }
            } else if vocoder_amount >= 0.5 {
                let carrier_gain =
                    self.vocoder_oscillator.render(vocoder_shape, self.parameters.note, &self.internal_modulation[..size], &mut self.aux_output[..size]);
                for j in 0..size {
                    self.carrier[j] = self.aux_output[j] * carrier_gain;
                }
            } else {
                let balance = vocoder_amount * 2.0;
                self.xmod_oscillator.render(xmod_shape, self.parameters.note, &self.internal_modulation[..size], &mut self.carrier[..size]);
                let carrier_gain =
                    self.vocoder_oscillator.render(vocoder_shape, self.parameters.note, &self.internal_modulation[..size], &mut self.aux_output[..size]);
                for j in 0..size {
                    let a = self.carrier[j];
                    let b = self.aux_output[j];
                    self.aux_output[j] = a + (b - a) * balance;
                    let a2 = a * K_XMOD_CARRIER_GAIN;
                    let b2 = b * carrier_gain;
                    self.carrier[j] = a2 + (b2 - a2) * balance;
                }
            }
        }

        if vocoder_amount < 0.5 {
            self.src_up[0].process(&self.carrier[..size], &mut self.oversampled_carrier[..size * K_OVERSAMPLING]);
            self.src_up[1].process(&self.modulator_buf[..size], &mut self.oversampled_modulator[..size * K_OVERSAMPLING]);

            let algorithm = (self.parameters.modulation_algorithm * 8.0).min(5.999);
            let previous_algorithm = (self.previous_parameters.modulation_algorithm * 8.0).min(5.999);

            let algorithm_integral = algorithm as i32;
            let algorithm_fractional = algorithm - algorithm_integral as f32;
            let previous_algorithm_integral = previous_algorithm as i32;
            let mut previous_algorithm_fractional = previous_algorithm - previous_algorithm_integral as f32;

            if algorithm_integral != previous_algorithm_integral {
                previous_algorithm_fractional = algorithm_fractional;
            }

            let (algorithm_1, algorithm_2) = XMOD_TABLE[algorithm_integral as usize];
            process_xmod(
                algorithm_1,
                algorithm_2,
                previous_algorithm_fractional,
                algorithm_fractional,
                self.previous_parameters.skewed_modulation_parameter(),
                self.parameters.skewed_modulation_parameter(),
                &self.oversampled_modulator[..size * K_OVERSAMPLING],
                &self.oversampled_carrier[..size * K_OVERSAMPLING],
                &mut self.oversampled_output[..size * K_OVERSAMPLING],
            );

            self.src_down.process(&self.oversampled_output[..size * K_OVERSAMPLING], &mut self.main_output[..size]);
        } else {
            let release_time = (4.0 * (self.parameters.modulation_algorithm - 0.75)).clamp(0.0, 1.0);

            self.vocoder.set_release_time(release_time * (2.0 - release_time));
            self.vocoder.set_formant_shift(self.parameters.modulation_parameter);
            self.vocoder.process(&self.modulator_buf[..size], &self.carrier[..size], &mut self.main_output[..size], size);
        }

        // Cross-fade to raw modulator for the transition between cross-modulation
        // algorithms and vocoding algorithms.
        let transition_gain = 2.0 * if vocoder_amount < 0.5 { vocoder_amount } else { 1.0 - vocoder_amount };
        if transition_gain != 0.0 {
            for (m, &mo) in self.main_output[..size].iter_mut().zip(self.modulator_buf[..size].iter()) {
                *m += transition_gain * (mo - *m);
            }
        }

        for (frame, (&m, &a)) in output[..size].iter_mut().zip(self.main_output[..size].iter().zip(self.aux_output[..size].iter())) {
            frame.l = clip16((m * 32768.0) as i32) as i16;
            frame.r = clip16((a * 16384.0) as i32) as i16;
        }
        self.previous_parameters = self.parameters;
    }

    fn process_easter_egg(&mut self, input: &[ShortFrame], output: &mut [ShortFrame], size: usize) {
        // Generate the I/Q components.
        if self.parameters.carrier_shape != 0 {
            let d = self.parameters.frequency_shift_pot - 0.5;
            let mut linear_modulation_amount = 1.0 - 14.0 * d * d;
            if linear_modulation_amount < 0.0 {
                linear_modulation_amount = 0.0;
            }
            let mut frequency = self.parameters.frequency_shift_pot;
            frequency += linear_modulation_amount * self.parameters.frequency_shift_cv;

            let direction = if frequency >= 0.5 { 1.0 } else { -1.0 };
            let mut frequency = 2.0 * (frequency - 0.5).abs();
            frequency = if frequency <= 0.4 {
                frequency * frequency * frequency * 62.5
            } else {
                4.0 * semitones_to_ratio(180.0 * (frequency - 0.4))
            };
            frequency *= semitones_to_ratio(self.parameters.frequency_shift_cv * 60.0 * (1.0 - linear_modulation_amount) * direction);
            frequency *= direction;

            let shape = (self.parameters.carrier_shape - 1) as f32 * 0.5;
            self.quadrature_oscillator.render(shape, frequency, &mut self.carrier_i[..size], &mut self.carrier_q[..size], size);
        } else {
            for (c, frame) in self.carrier[..size].iter_mut().zip(input[..size].iter()) {
                *c = frame.l as f32 / 32768.0;
            }
            self.quadrature_transform[0].process_block(&self.carrier[..size], &mut self.carrier_i[..size], &mut self.carrier_q[..size]);

            // Dropped at the end of this `else` block -- writes its final
            // ramped value into `previous_parameters_.phase_shift` early;
            // the explicit copy near the end of this function then
            // overwrites it again with the exact (non-ramped) target,
            // matching the C++ (whose `previous_parameters_ = parameters_`
            // there runs *after* this inner-scope interpolator has already
            // dropped).
            let mut phase_shift = ParameterInterpolator::new(&mut self.previous_parameters.phase_shift, self.parameters.phase_shift, size);
            for j in 0..size {
                let x_i = self.carrier_i[j];
                let x_q = self.carrier_q[j];
                let angle = phase_shift.next();
                let r_sin = interpolate(&LUT_SIN, angle, 1024.0);
                let r_cos = interpolate(&LUT_SIN[256..], angle, 1024.0);
                self.carrier_i[j] = r_sin * x_i + r_cos * x_q;
                self.carrier_q[j] = r_sin * x_q - r_cos * x_i;
            }
        }

        // These three interpolators are only dropped when this function
        // returns -- *after* the explicit `previous_parameters` field
        // copies near the end below -- so their ramped values are what's
        // left standing in `modulation_parameter`/`channel_drive[0..2]`,
        // not the exact target `parameters_` copies. This mirrors the C++
        // exactly: its RAII `ParameterInterpolator`s have the same
        // function-scope lifetime, and its single `previous_parameters_ =
        // parameters_` assignment (which this port can't reproduce as one
        // op while these are still borrowed) executes before their
        // destructors run.
        let mut mix = ParameterInterpolator::new(&mut self.previous_parameters.modulation_parameter, self.parameters.modulation_parameter, size);
        let (cd0, cd1) = self.previous_parameters.channel_drive.split_at_mut(1);
        let mut feedback_amount = ParameterInterpolator::new(&mut cd0[0], self.parameters.channel_drive[0], size);
        let mut dry_wet = ParameterInterpolator::new(&mut cd1[0], self.parameters.channel_drive[1], size);

        let mut feedback_sample = self.feedback_sample;
        for j in 0..size {
            let timbre = mix.next();

            let mut in_val = input[j].r as f32 / 32768.0;
            if self.parameters.carrier_shape != 0 {
                in_val += input[j].l as f32 / 32768.0;
            }
            let modulator = in_val;

            // Apply feedback if necessary, and soft limit.
            let mut amount = feedback_amount.next();
            amount *= 2.0 - amount;
            amount *= 2.0 - amount;

            // mic.w feedback amount tweak.
            let max_fb = 1.0 + 2.0 * (timbre - 0.5) * (timbre - 0.5);
            let modulator = modulator + amount * (soft_clip(modulator + max_fb * feedback_sample * amount) - modulator);

            let (modulator_i, modulator_q) = self.quadrature_transform[1].process_sample(modulator);

            // Modulate!
            let a = self.carrier_i[j] * modulator_i;
            let b = self.carrier_q[j] * modulator_q;
            let up = a - b;
            let down = a + b;
            let lut_index = timbre;
            let fade_in = interpolate(&LUT_XFADE_IN, lut_index, 256.0);
            let fade_out = interpolate(&LUT_XFADE_OUT, lut_index, 256.0);
            let mut main = up * fade_in + down * fade_out;
            let mut aux = down * fade_in + up * fade_out;

            // Simple LP to prevent feedback of high-frequencies.
            one_pole(&mut feedback_sample, main, 0.2);

            let wet_dry = 1.0 - dry_wet.next();
            main += wet_dry * (in_val - main);
            aux += wet_dry * (in_val - aux);

            output[j].l = clip16((main * 32768.0) as i32) as i16;
            output[j].r = clip16((aux * 32768.0) as i32) as i16;
        }
        self.feedback_sample = feedback_sample;

        self.previous_parameters.modulation_algorithm = self.parameters.modulation_algorithm;
        self.previous_parameters.frequency_shift_pot = self.parameters.frequency_shift_pot;
        self.previous_parameters.frequency_shift_cv = self.parameters.frequency_shift_cv;
        self.previous_parameters.phase_shift = self.parameters.phase_shift;
        self.previous_parameters.note = self.parameters.note;
        self.previous_parameters.carrier_shape = self.parameters.carrier_shape;
        // `modulation_parameter`/`channel_drive` are deliberately left
        // alone here -- see the comment above `mix`'s declaration.
    }
}
