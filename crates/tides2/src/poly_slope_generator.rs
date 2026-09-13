//! `tides2/poly_slope_generator.{h,cc}` -- four related slope generators
//! driven off a shared [`crate::ramp_generator::RampGenerator`], producing one
//! of several output personalities (raw gates, amplitude envelopes, per-channel
//! slope/phase, or per-channel frequency division/multiplication).
//!
//! The C's `RenderInternal<ramp_mode, output_mode, range>` is a template
//! instantiated into a `[RAMP_MODE_LAST][OUTPUT_MODE_LAST][RANGE_LAST]`
//! function-pointer table purely so the firmware can place the AR/LOOPING
//! variants `IN_RAM` (they run in the audio ISR); its *body* already branches
//! on `output_mode`/`ramp_mode`/`range` at what would be runtime if they
//! weren't template parameters. This port keeps that body and drops the
//! table, calling the (now singular) `render_internal` directly -- see the
//! `mi-braids` precedent of `match` replacing a `RenderFn fn_table_[]`.

use stmlib::gate_flags::GateFlags;
use stmlib::hysteresis_quantizer::HysteresisQuantizer2;
use stmlib::parameter_interpolator::ParameterInterpolator;
use stmlib::{constrain, fdsp};

use crate::ramp_generator::{OutputMode, RampGenerator, RampMode, Range};
use crate::ramp_shaper::{RampShaper, RampWaveshaper};
use crate::ratio::Ratio;
use crate::resources::{LUT_BIPOLAR_FOLD, LUT_UNIPOLAR_FOLD, LUT_WAVETABLE};

pub const NUM_CHANNELS: usize = 4;

#[derive(Debug, Clone, Copy, Default)]
pub struct OutputSample {
    pub channel: [f32; NUM_CHANNELS],
}

struct Filter<const N: usize> {
    lp_1: [f32; N],
    lp_2: [f32; N],
}

impl<const N: usize> Filter<N> {
    fn new() -> Self {
        Filter { lp_1: [0.0; N], lp_2: [0.0; N] }
    }

    fn init(&mut self) {
        self.lp_1 = [0.0; N];
        self.lp_2 = [0.0; N];
    }

    // `i` indexes `f`, `self.lp_1` and `self.lp_2` in lockstep -- not a walk
    // of any single one of them.
    #[allow(clippy::needless_range_loop)]
    fn process(&mut self, f: &[f32], out: &mut [OutputSample], num_effective_channels: usize) {
        for sample in out.iter_mut() {
            for i in 0..num_effective_channels {
                fdsp::one_pole(&mut self.lp_1[i], sample.channel[i], f[i]);
                fdsp::one_pole(&mut self.lp_2[i], self.lp_1[i], f[i]);
                sample.channel[i] = self.lp_2[i];
            }
        }
    }
}

#[rustfmt::skip]
#[allow(clippy::excessive_precision)]
const AUDIO_RATIO_TABLE: [[Ratio; NUM_CHANNELS]; 21] = [
    [Ratio { ratio: 1.0, q: 1 }, Ratio { ratio: 0.5, q: 2 }, Ratio { ratio: 0.25, q: 4 }, Ratio { ratio: 0.125, q: 8 }],
    [Ratio { ratio: 1.0, q: 1 }, Ratio { ratio: 0.5, q: 2 }, Ratio { ratio: 0.33333333, q: 3 }, Ratio { ratio: 0.2, q: 5 }],
    [Ratio { ratio: 1.0, q: 1 }, Ratio { ratio: 0.5, q: 2 }, Ratio { ratio: 0.33333333, q: 3 }, Ratio { ratio: 0.25, q: 4 }],
    [Ratio { ratio: 1.0, q: 1 }, Ratio { ratio: 0.66666666, q: 3 }, Ratio { ratio: 0.44444444, q: 9 }, Ratio { ratio: 0.296296297, q: 27 }],
    [Ratio { ratio: 1.0, q: 1 }, Ratio { ratio: 0.66666666, q: 3 }, Ratio { ratio: 0.5, q: 2 }, Ratio { ratio: 0.33333333, q: 3 }],
    [Ratio { ratio: 1.0, q: 1 }, Ratio { ratio: 0.75, q: 4 }, Ratio { ratio: 0.66666666, q: 3 }, Ratio { ratio: 0.5, q: 2 }],
    [Ratio { ratio: 1.0, q: 1 }, Ratio { ratio: 0.790123456, q: 81 }, Ratio { ratio: 0.66666666, q: 3 }, Ratio { ratio: 0.5, q: 2 }],
    [Ratio { ratio: 1.0, q: 1 }, Ratio { ratio: 0.790123456, q: 81 }, Ratio { ratio: 0.75, q: 4 }, Ratio { ratio: 0.66666666, q: 3 }],
    [Ratio { ratio: 1.0, q: 1 }, Ratio { ratio: 0.88888888, q: 9 }, Ratio { ratio: 0.790123456, q: 81 }, Ratio { ratio: 0.66666666, q: 3 }],

    [Ratio { ratio: 1.0, q: 1 }, Ratio { ratio: 0.99090909091, q: 109 }, Ratio { ratio: 0.987341772, q: 79 }, Ratio { ratio: 0.9811320755, q: 53 }],
    [Ratio { ratio: 1.0, q: 1 }, Ratio { ratio: 1.0, q: 1 }, Ratio { ratio: 1.0, q: 1 }, Ratio { ratio: 1.0, q: 1 }],
    [Ratio { ratio: 1.0, q: 1 }, Ratio { ratio: 1.009174312, q: 109 }, Ratio { ratio: 1.01265823, q: 79 }, Ratio { ratio: 1.0188679245, q: 53 }],

    [Ratio { ratio: 1.0, q: 1 }, Ratio { ratio: 1.125, q: 8 }, Ratio { ratio: 1.265625, q: 64 }, Ratio { ratio: 1.5, q: 2 }],
    [Ratio { ratio: 1.0, q: 1 }, Ratio { ratio: 1.265625, q: 64 }, Ratio { ratio: 1.3333333, q: 3 }, Ratio { ratio: 1.5, q: 2 }],
    [Ratio { ratio: 1.0, q: 1 }, Ratio { ratio: 1.265625, q: 64 }, Ratio { ratio: 1.5, q: 2 }, Ratio { ratio: 2.0, q: 1 }],
    [Ratio { ratio: 1.0, q: 1 }, Ratio { ratio: 1.33333333, q: 3 }, Ratio { ratio: 1.5, q: 2 }, Ratio { ratio: 2.0, q: 1 }],
    [Ratio { ratio: 1.0, q: 1 }, Ratio { ratio: 1.5, q: 2 }, Ratio { ratio: 2.0, q: 1 }, Ratio { ratio: 3.0, q: 1 }],
    [Ratio { ratio: 1.0, q: 1 }, Ratio { ratio: 1.5, q: 2 }, Ratio { ratio: 2.25, q: 4 }, Ratio { ratio: 3.375, q: 8 }],
    [Ratio { ratio: 1.0, q: 1 }, Ratio { ratio: 2.0, q: 1 }, Ratio { ratio: 3.0, q: 1 }, Ratio { ratio: 4.0, q: 1 }],
    [Ratio { ratio: 1.0, q: 1 }, Ratio { ratio: 2.0, q: 1 }, Ratio { ratio: 3.0, q: 1 }, Ratio { ratio: 5.0, q: 1 }],
    [Ratio { ratio: 1.0, q: 1 }, Ratio { ratio: 2.0, q: 1 }, Ratio { ratio: 4.0, q: 1 }, Ratio { ratio: 8.0, q: 1 }],
];

#[rustfmt::skip]
#[allow(clippy::excessive_precision)]
const CONTROL_RATIO_TABLE: [[Ratio; NUM_CHANNELS]; 21] = [
    [Ratio { ratio: 1.0, q: 1 }, Ratio { ratio: 0.5, q: 2 }, Ratio { ratio: 0.25, q: 4 }, Ratio { ratio: 0.125, q: 8 }],
    [Ratio { ratio: 1.0, q: 1 }, Ratio { ratio: 0.5, q: 2 }, Ratio { ratio: 0.33333333, q: 3 }, Ratio { ratio: 0.2, q: 5 }],
    [Ratio { ratio: 1.0, q: 1 }, Ratio { ratio: 0.5, q: 2 }, Ratio { ratio: 0.33333333, q: 3 }, Ratio { ratio: 0.25, q: 4 }],
    [Ratio { ratio: 1.0, q: 1 }, Ratio { ratio: 0.66666666, q: 3 }, Ratio { ratio: 0.5, q: 2 }, Ratio { ratio: 0.25, q: 4 }],
    [Ratio { ratio: 1.0, q: 1 }, Ratio { ratio: 0.66666666, q: 3 }, Ratio { ratio: 0.5, q: 2 }, Ratio { ratio: 0.33333333, q: 3 }],
    [Ratio { ratio: 1.0, q: 1 }, Ratio { ratio: 0.75, q: 4 }, Ratio { ratio: 0.66666666, q: 3 }, Ratio { ratio: 0.5, q: 2 }],
    [Ratio { ratio: 1.0, q: 1 }, Ratio { ratio: 0.8, q: 5 }, Ratio { ratio: 0.66666666, q: 3 }, Ratio { ratio: 0.5, q: 2 }],
    [Ratio { ratio: 1.0, q: 1 }, Ratio { ratio: 0.8, q: 5 }, Ratio { ratio: 0.75, q: 3 }, Ratio { ratio: 0.5, q: 2 }],
    [Ratio { ratio: 1.0, q: 1 }, Ratio { ratio: 0.8, q: 5 }, Ratio { ratio: 0.75, q: 4 }, Ratio { ratio: 0.66666666, q: 3 }],

    [Ratio { ratio: 1.0, q: 1 }, Ratio { ratio: 0.909090909091, q: 11 }, Ratio { ratio: 0.857142857143, q: 7 }, Ratio { ratio: 0.8, q: 5 }],
    [Ratio { ratio: 1.0, q: 1 }, Ratio { ratio: 1.0, q: 1 }, Ratio { ratio: 1.0, q: 1 }, Ratio { ratio: 1.0, q: 1 }],
    [Ratio { ratio: 1.0, q: 1 }, Ratio { ratio: 1.09090909091, q: 11 }, Ratio { ratio: 1.142857143, q: 7 }, Ratio { ratio: 1.2, q: 5 }],

    [Ratio { ratio: 1.0, q: 1 }, Ratio { ratio: 1.25, q: 4 }, Ratio { ratio: 1.33333333, q: 3 }, Ratio { ratio: 1.5, q: 2 }],
    [Ratio { ratio: 1.0, q: 1 }, Ratio { ratio: 1.25, q: 4 }, Ratio { ratio: 1.33333333, q: 3 }, Ratio { ratio: 2.0, q: 2 }],
    [Ratio { ratio: 1.0, q: 1 }, Ratio { ratio: 1.25, q: 4 }, Ratio { ratio: 1.5, q: 3 }, Ratio { ratio: 2.0, q: 2 }],
    [Ratio { ratio: 1.0, q: 1 }, Ratio { ratio: 1.33333333, q: 3 }, Ratio { ratio: 1.5, q: 2 }, Ratio { ratio: 2.0, q: 1 }],
    [Ratio { ratio: 1.0, q: 1 }, Ratio { ratio: 1.5, q: 2 }, Ratio { ratio: 2.0, q: 1 }, Ratio { ratio: 3.0, q: 1 }],
    [Ratio { ratio: 1.0, q: 1 }, Ratio { ratio: 1.5, q: 2 }, Ratio { ratio: 2.0, q: 1 }, Ratio { ratio: 4.0, q: 1 }],
    [Ratio { ratio: 1.0, q: 1 }, Ratio { ratio: 2.0, q: 1 }, Ratio { ratio: 3.0, q: 1 }, Ratio { ratio: 4.0, q: 1 }],
    [Ratio { ratio: 1.0, q: 1 }, Ratio { ratio: 2.0, q: 1 }, Ratio { ratio: 3.0, q: 1 }, Ratio { ratio: 5.0, q: 1 }],
    [Ratio { ratio: 1.0, q: 1 }, Ratio { ratio: 2.0, q: 1 }, Ratio { ratio: 4.0, q: 1 }, Ratio { ratio: 8.0, q: 1 }],
];

pub struct PolySlopeGenerator {
    frequency: f32,
    pw: f32,
    shift: f32,
    shape: f32,
    fold: f32,

    ratio_index_quantizer: HysteresisQuantizer2,
    ramp_generator: RampGenerator<NUM_CHANNELS>,
    ramp_shaper: [RampShaper; NUM_CHANNELS],
    ramp_waveshaper: [RampWaveshaper; NUM_CHANNELS],
    filter: Filter<NUM_CHANNELS>,
}

impl Default for PolySlopeGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl PolySlopeGenerator {
    pub fn new() -> Self {
        let mut g = PolySlopeGenerator {
            frequency: 0.0,
            pw: 0.0,
            shift: 0.0,
            shape: 0.0,
            fold: 0.0,
            ratio_index_quantizer: HysteresisQuantizer2::default(),
            ramp_generator: RampGenerator::new(),
            ramp_shaper: core::array::from_fn(|_| RampShaper::new()),
            ramp_waveshaper: core::array::from_fn(|_| RampWaveshaper::new()),
            filter: Filter::new(),
        };
        g.init();
        g
    }

    pub fn reset(&mut self) {
        self.filter.init();
    }

    pub fn init(&mut self) {
        self.frequency = 0.01;
        self.pw = 0.0;
        self.shift = 0.0;
        self.shape = 0.0;
        self.fold = 0.0;

        self.ramp_generator.init();
        for i in 0..NUM_CHANNELS {
            self.ramp_shaper[i].init();
            self.ramp_waveshaper[i].init();
        }
        self.filter.init();

        self.ratio_index_quantizer.init(21, 0.05, false);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        ramp_mode: RampMode,
        output_mode: OutputMode,
        range: Range,
        frequency: f32,
        pw: f32,
        shape: f32,
        smoothness: f32,
        shift: f32,
        gate_flags: &[GateFlags],
        ramp: Option<&[f32]>,
        out: &mut [OutputSample],
        size: usize,
    ) {
        let max_ratio = 1.0f32;
        let mut frequency = frequency.min(0.25 * max_ratio);

        let mut pw = pw;
        if range == Range::Control && pw < 0.5 {
            // Skew the response of the pulse width parameter, so that a more
            // interesting range of attack times can be set.
            pw = 0.5 + 0.6 * (pw - 0.5) / ((pw - 0.5).abs() + 0.1);
        }

        if ramp.is_some() && ramp_mode == RampMode::Ar {
            // When locking onto an external ramp, adjust the frequency to get
            // interesting trapezoidal shapes.
            frequency *= 1.0 + 2.0 * (pw - 0.5).abs();
        }

        let slope = 3.0 + (pw - 0.5).abs() * 5.0;
        let shape_amount = (shape - 0.5).abs() * 2.0;
        let shape_amount_attenuation = Self::tame(frequency, slope, 16.0);
        let shape = 0.5 + (shape - 0.5) * shape_amount_attenuation;

        let mut smoothness = smoothness;
        if smoothness > 0.5 {
            smoothness = 0.5
                + (smoothness - 0.5)
                    * Self::tame(
                        frequency,
                        slope * (3.0 + shape_amount * shape_amount_attenuation * 5.0),
                        12.0,
                    );
        }

        self.render_internal(
            ramp_mode, output_mode, range, frequency, pw, shape, smoothness, shift, gate_flags, ramp, out, size,
        );

        if smoothness < 0.5 {
            let mut ratio = smoothness * 2.0;
            ratio *= ratio;
            ratio *= ratio;

            let mut f = [0.0f32; NUM_CHANNELS];
            let last_channel = if output_mode == OutputMode::Gates { 1 } else { NUM_CHANNELS };
            for (i, f_i) in f.iter_mut().enumerate().take(last_channel) {
                let source = if output_mode == OutputMode::Frequency { i } else { 0 };
                *f_i = self.ramp_generator.frequency(source) * 0.5;
                *f_i += (1.0 - *f_i) * ratio;
            }
            if output_mode == OutputMode::Gates {
                self.filter.process(&f, out, 1);
            } else {
                self.filter.process(&f, out, NUM_CHANNELS);
            }
        }
    }

    // `needless_range_loop`: the per-channel loops below index several
    // parallel arrays at once (`self.ramp_shaper`/`self.ramp_waveshaper`,
    // `per_channel_pw`, `out[i].channel`) plus a mode-dependent `source`
    // index that isn't a simple walk of any one of them.
    #[allow(clippy::too_many_arguments, clippy::needless_range_loop)]
    fn render_internal(
        &mut self,
        ramp_mode: RampMode,
        output_mode: OutputMode,
        range: Range,
        frequency: f32,
        pw: f32,
        shape: f32,
        smoothness: f32,
        shift: f32,
        gate_flags: &[GateFlags],
        ramp: Option<&[f32]>,
        out: &mut [OutputSample],
        size: usize,
    ) {
        let is_phasor = !(range == Range::Audio && ramp_mode == RampMode::Looping);

        let mut fm = ParameterInterpolator::new(&mut self.frequency, frequency, size);
        let mut pwm = ParameterInterpolator::new(&mut self.pw, pw, size);
        let mut shift_modulation = ParameterInterpolator::new(&mut self.shift, 2.0 * shift - 1.0, size);
        let mut shape_modulation = ParameterInterpolator::new(
            &mut self.shape,
            if is_phasor { shape * 5.9999 + 5.0 } else { shape * 3.9999 },
            size,
        );
        let mut fold_modulation =
            ParameterInterpolator::new(&mut self.fold, (2.0 * (smoothness - 0.5)).max(0.0), size);

        if output_mode == OutputMode::Frequency {
            let ratio_index = self.ratio_index_quantizer.process(shift) as usize;
            if range == Range::Control {
                self.ramp_generator.set_next_ratio(&CONTROL_RATIO_TABLE[ratio_index]);
            } else {
                self.ramp_generator.set_next_ratio(&AUDIO_RATIO_TABLE[ratio_index]);
            }
        }

        for i in 0..size {
            let f0 = fm.next();
            let pw = pwm.next();
            let shift = shift_modulation.next();
            let step = shift * (1.0 / (NUM_CHANNELS - 1) as f32);
            let partial_step = shift * (1.0 / NUM_CHANNELS as f32);
            let fold = fold_modulation.next();

            let pw_increment = (if shift > 0.0 { 1.0 - pw } else { pw }) * step;
            let mut per_channel_pw = [0.0f32; NUM_CHANNELS];
            for (j, v) in per_channel_pw.iter_mut().enumerate() {
                *v = pw + pw_increment * j as f32;
            }

            // Increment ramps.
            let ramp_i = ramp.map(|r| r[i]);
            if output_mode == OutputMode::SlopePhase && ramp_mode == RampMode::Ar {
                match ramp_i {
                    Some(r) => {
                        self.ramp_generator.step(ramp_mode, output_mode, range, f0, &per_channel_pw, GateFlags::LOW, Some(r));
                    }
                    None => {
                        self.ramp_generator.step(ramp_mode, output_mode, range, f0, &per_channel_pw, gate_flags[i], None);
                    }
                }
            } else {
                match ramp_i {
                    Some(r) => {
                        self.ramp_generator.step(ramp_mode, output_mode, range, f0, &[pw], GateFlags::LOW, Some(r));
                    }
                    None => {
                        self.ramp_generator.step(ramp_mode, output_mode, range, f0, &[pw], gate_flags[i], None);
                    }
                }
            }

            // Compute shape.
            let shape = shape_modulation.next();
            let shape_integral = shape as i32;
            let shape_fractional = shape - shape_integral as f32;
            let shape_table = &LUT_WAVETABLE[(shape_integral as usize) * 1025..];

            match output_mode {
                OutputMode::Gates => {
                    let phase = self.ramp_generator.phase(0);
                    let frequency = self.ramp_generator.frequency(0);
                    let raw = self.ramp_shaper[0].slope(ramp_mode, range, phase, 0.0, frequency, pw);
                    let slope = self.ramp_waveshaper[0].shape(ramp_mode, raw, shape_table, shape_fractional);

                    out[i].channel[0] = Self::fold(ramp_mode, slope, fold) * shift;
                    out[i].channel[1] = Self::scale(
                        ramp_mode,
                        if is_phasor {
                            self.ramp_waveshaper[1].shape(ramp_mode, raw, &LUT_WAVETABLE[8200..], 0.0)
                        } else {
                            raw
                        },
                    );
                    out[i].channel[2] = self.ramp_shaper[2].eoa(ramp_mode, range, phase, frequency, pw) * 8.0;
                    out[i].channel[3] = self.ramp_shaper[3].eor(ramp_mode, range, phase, frequency, pw) * 8.0;
                }
                OutputMode::Amplitude => {
                    let phase = self.ramp_generator.phase(0);
                    let frequency = self.ramp_generator.frequency(0);
                    let raw = self.ramp_shaper[0].slope(ramp_mode, range, phase, 0.0, frequency, pw);
                    let shaped = self.ramp_waveshaper[0].shape(ramp_mode, raw, shape_table, shape_fractional);
                    let slope = Self::fold(ramp_mode, shaped, fold) * if shift < 0.0 { -1.0 } else { 1.0 };
                    let channel_index = (shift * 5.1).abs();
                    for j in 0..NUM_CHANNELS {
                        let channel = (j + 1) as f32;
                        let gain = (1.0 - (channel - channel_index).abs()).max(0.0);
                        let equal_pow = range == Range::Audio;
                        out[i].channel[j] = slope * gain * (if equal_pow { 2.0 - gain } else { 1.0 });
                    }
                }
                OutputMode::SlopePhase => {
                    let mut phase_shift = 0.0f32;
                    for j in 0..NUM_CHANNELS {
                        let source = if ramp_mode == RampMode::Ar { j } else { 0 };
                        let this_pw = if ramp_mode == RampMode::Ad { per_channel_pw[j] } else { pw };
                        let raw = self.ramp_shaper[j].slope(
                            ramp_mode,
                            range,
                            self.ramp_generator.phase(source),
                            phase_shift,
                            self.ramp_generator.frequency(source),
                            this_pw,
                        );
                        let shaped = self.ramp_waveshaper[j].shape(ramp_mode, raw, shape_table, shape_fractional);
                        out[i].channel[j] = Self::fold(ramp_mode, shaped, fold);
                        phase_shift -= if range == Range::Audio { step } else { partial_step };
                    }
                }
                OutputMode::Frequency => {
                    for j in 0..NUM_CHANNELS {
                        let raw = self.ramp_shaper[j].slope(
                            ramp_mode,
                            range,
                            self.ramp_generator.phase(j),
                            0.0,
                            self.ramp_generator.frequency(j),
                            pw,
                        );
                        let shaped = self.ramp_waveshaper[j].shape(ramp_mode, raw, shape_table, shape_fractional);
                        out[i].channel[j] = Self::fold(ramp_mode, shaped, fold);
                    }
                }
            }
        }
    }

    fn fold(ramp_mode: RampMode, unipolar: f32, fold_amount: f32) -> f32 {
        if ramp_mode == RampMode::Looping {
            let bipolar = 2.0 * unipolar - 1.0;
            let folded = if fold_amount > 0.0 {
                fdsp::interpolate(&LUT_BIPOLAR_FOLD, 0.5 + bipolar * (0.03 + 0.46 * fold_amount), 1024.0)
            } else {
                0.0
            };
            5.0 * (bipolar + (folded - bipolar) * fold_amount)
        } else {
            let folded = if fold_amount > 0.0 {
                fdsp::interpolate(&LUT_UNIPOLAR_FOLD, unipolar * fold_amount, 1024.0)
            } else {
                0.0
            };
            8.0 * (unipolar + (folded - unipolar) * fold_amount)
        }
    }

    fn scale(ramp_mode: RampMode, unipolar: f32) -> f32 {
        if ramp_mode == RampMode::Looping {
            10.0 * unipolar - 5.0
        } else {
            8.0 * unipolar
        }
    }

    fn tame(f0: f32, harmonics: f32, order: f32) -> f32 {
        let f0 = f0 * harmonics;
        let max_f = 0.5 * (1.0 / order);
        let max_amount = constrain(1.0 - (f0 - max_f) / (0.5 - max_f), 0.0, 1.0);
        max_amount * max_amount * max_amount
    }
}
