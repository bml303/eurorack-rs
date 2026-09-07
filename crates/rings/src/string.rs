//! `rings/dsp/string.{h,cc}` -- one Karplus-Strong waveguide string (a comb
//! with a 3-tap FIR loop-damping filter, an IIR loss filter, a DC blocker and
//! an optional dispersion all-pass / curved-bridge non-linearity).
//!
//! `rings/dsp/string.cc` is byte-identical to `elements/dsp/string.cc`; the
//! only real difference is that Rings' `set_dispersion` is a plain setter (the
//! `< 0.24 ? ...` mapping is applied by `Part` before the call).

use alloc::boxed::Box;

use stmlib::fdsp::crossfade;
use stmlib::filter::{DcBlocker, FilterMode, FrequencyApproximation, Svf};
use stmlib::parameter_interpolator::ParameterInterpolator;
use stmlib::units::semitones_to_ratio;
use stmlib::{DelayLine, Random, constrain};

use crate::dsp::SAMPLE_RATE;
use crate::resources::LUT_SVF_SHIFT;

pub const DELAY_LINE_SIZE: usize = 2048;

/// `rings::DampingFilter` -- the 3-tap FIR in the loop, per-sample ramped.
#[derive(Debug, Clone, Copy, Default)]
struct DampingFilter {
    x: f32,
    x2: f32,
    brightness: f32,
    brightness_increment: f32,
    damping: f32,
    damping_increment: f32,
}

impl DampingFilter {
    fn init(&mut self) {
        *self = Self::default();
    }

    fn configure(&mut self, damping: f32, brightness: f32, size: usize) {
        if size == 0 {
            self.damping = damping;
            self.brightness = brightness;
            self.damping_increment = 0.0;
            self.brightness_increment = 0.0;
        } else {
            let step = 1.0 / size as f32;
            self.damping_increment = (damping - self.damping) * step;
            self.brightness_increment = (brightness - self.brightness) * step;
        }
    }

    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let h0 = (1.0 + self.brightness) * 0.5;
        let h1 = (1.0 - self.brightness) * 0.25;
        let y = self.damping * (h0 * self.x + h1 * (x + self.x2));
        self.x2 = self.x;
        self.x = x;
        self.brightness += self.brightness_increment;
        self.damping += self.damping_increment;
        y
    }
}

/// `rings::String`.
pub struct String {
    frequency: f32,
    dispersion: f32,
    brightness: f32,
    damping: f32,
    position: f32,

    delay: f32,
    clamped_position: f32,
    previous_dispersion: f32,
    previous_damping_compensation: f32,

    enable_dispersion: bool,
    dispersion_noise: f32,

    src_phase: f32,
    out_sample: [f32; 2],
    aux_sample: [f32; 2],

    curved_bridge: f32,

    string: Box<DelayLine<DELAY_LINE_SIZE>>,
    stretch: Box<DelayLine<{ DELAY_LINE_SIZE / 2 }>>,

    fir_damping_filter: DampingFilter,
    iir_damping_filter: Svf,
    dc_blocker: DcBlocker,
}

impl Default for String {
    fn default() -> Self {
        Self::new()
    }
}

impl String {
    pub fn new() -> Self {
        let mut s = Self {
            frequency: 220.0 / SAMPLE_RATE,
            dispersion: 0.0,
            brightness: 0.5,
            damping: 0.3,
            position: 0.8,
            delay: 0.0,
            clamped_position: 0.0,
            previous_dispersion: 0.0,
            previous_damping_compensation: 0.0,
            enable_dispersion: false,
            dispersion_noise: 0.0,
            src_phase: 0.0,
            out_sample: [0.0; 2],
            aux_sample: [0.0; 2],
            curved_bridge: 0.0,
            string: Box::new(DelayLine::default()),
            stretch: Box::new(DelayLine::default()),
            fir_damping_filter: DampingFilter::default(),
            iir_damping_filter: Svf::default(),
            dc_blocker: DcBlocker::default(),
        };
        s.init(false);
        s
    }

    /// `Init(enable_dispersion)`.
    pub fn init(&mut self, enable_dispersion: bool) {
        self.enable_dispersion = enable_dispersion;

        self.string.init();
        self.stretch.init();
        self.fir_damping_filter.init();
        self.iir_damping_filter.init();

        self.set_frequency(220.0 / SAMPLE_RATE);
        self.set_dispersion(0.0);
        self.set_brightness(0.5);
        self.set_damping(0.3);
        self.set_position(0.8);

        self.delay = 1.0 / self.frequency;
        self.clamped_position = 0.0;
        self.previous_dispersion = 0.0;
        self.dispersion_noise = 0.0;
        self.curved_bridge = 0.0;
        self.previous_damping_compensation = 0.0;

        self.out_sample = [0.0; 2];
        self.aux_sample = [0.0; 2];

        self.dc_blocker.init(1.0 - 20.0 / SAMPLE_RATE);
    }

    #[inline]
    pub fn set_frequency(&mut self, frequency: f32) {
        self.frequency = frequency;
    }

    /// `set_frequency(frequency, coefficient)` -- one-pole smoothed (glide).
    #[inline]
    pub fn set_frequency_glide(&mut self, frequency: f32, coefficient: f32) {
        self.frequency += coefficient * (frequency - self.frequency);
    }

    /// `set_dispersion` -- plain setter (the mapping lives in `Part`).
    #[inline]
    pub fn set_dispersion(&mut self, dispersion: f32) {
        self.dispersion = dispersion;
    }
    #[inline]
    pub fn set_brightness(&mut self, brightness: f32) {
        self.brightness = brightness;
    }
    #[inline]
    pub fn set_damping(&mut self, damping: f32) {
        self.damping = damping;
    }
    #[inline]
    pub fn set_position(&mut self, position: f32) {
        self.position = position;
    }

    /// `Process(in, out, aux, size)` -- accumulates into `out` and `aux`.
    pub fn process(&mut self, input: &[f32], out: &mut [f32], aux: &mut [f32], size: usize) {
        if self.enable_dispersion {
            self.process_internal::<true>(input, out, aux, size);
        } else {
            self.process_internal::<false>(input, out, aux, size);
        }
    }

    fn process_internal<const DISPERSION: bool>(
        &mut self,
        input: &[f32],
        out: &mut [f32],
        aux: &mut [f32],
        size: usize,
    ) {
        let delay = constrain(1.0 / self.frequency, 4.0, DELAY_LINE_SIZE as f32 - 4.0);

        let mut src_ratio = delay * self.frequency;
        if src_ratio >= 0.9999 {
            self.src_phase = 1.0;
            src_ratio = 1.0;
        }

        let clamped_position = 0.5 - 0.98 * (self.position - 0.5).abs();

        let mut delay_modulation = ParameterInterpolator::new(&mut self.delay, delay, size);
        let mut position_modulation =
            ParameterInterpolator::new(&mut self.clamped_position, clamped_position, size);
        let mut dispersion_modulation =
            ParameterInterpolator::new(&mut self.previous_dispersion, self.dispersion, size);

        let lf_damping = self.damping * (2.0 - self.damping);
        let rt60 = 0.07 * semitones_to_ratio(lf_damping * 96.0) * SAMPLE_RATE;
        let rt60_base_2_12 = (-120.0 * delay / src_ratio / rt60).max(-127.0);
        let mut damping_coefficient = semitones_to_ratio(rt60_base_2_12);
        let mut brightness = self.brightness * self.brightness;
        let noise_filter = semitones_to_ratio((self.brightness - 1.0) * 48.0);
        let mut damping_cutoff =
            (24.0 + self.damping * self.damping * 48.0 + self.brightness * self.brightness * 24.0)
                .min(84.0);
        let mut damping_f = (self.frequency * semitones_to_ratio(damping_cutoff)).min(0.499);

        if self.damping >= 0.95 {
            let to_infinite = 20.0 * (self.damping - 0.95);
            damping_coefficient += to_infinite * (1.0 - damping_coefficient);
            brightness += to_infinite * (1.0 - brightness);
            damping_f += to_infinite * (0.4999 - damping_f);
            damping_cutoff += to_infinite * (128.0 - damping_cutoff);
        }

        self.fir_damping_filter
            .configure(damping_coefficient, brightness, size);
        self.iir_damping_filter
            .set_f_q(damping_f, 0.5, FrequencyApproximation::Accurate);

        let damping_compensation_target = 1.0 - interpolate_1(&LUT_SVF_SHIFT, damping_cutoff);
        let mut damping_compensation_modulation = ParameterInterpolator::new(
            &mut self.previous_damping_compensation,
            damping_compensation_target,
            size,
        );

        for i in 0..size {
            self.src_phase += src_ratio;
            if self.src_phase > 1.0 {
                self.src_phase -= 1.0;

                let mut delay = delay_modulation.next();
                let comb_delay = delay * position_modulation.next();

                delay *= damping_compensation_modulation.next(); // IIR group delay.
                delay -= 1.0; // FIR delay.

                let mut s;

                if DISPERSION {
                    let mut noise = 2.0 * Random::get_float() - 1.0;
                    noise *= 1.0 / (0.2 + noise_filter);
                    self.dispersion_noise += noise_filter * (noise - self.dispersion_noise);

                    let dispersion = dispersion_modulation.next();
                    let stretch_point = if dispersion <= 0.0 {
                        0.0
                    } else {
                        dispersion * (2.0 - dispersion) * 0.475
                    };
                    let mut noise_amount = if dispersion > 0.75 {
                        4.0 * (dispersion - 0.75)
                    } else {
                        0.0
                    };
                    let bridge_curving_raw = if dispersion < 0.0 { -dispersion } else { 0.0 };

                    noise_amount = noise_amount * noise_amount * 0.025;
                    let ac_blocking_amount = bridge_curving_raw;

                    let bridge_curving = bridge_curving_raw * bridge_curving_raw * 0.01;
                    let ap_gain = -0.618 * dispersion / (0.15 + dispersion.abs());

                    let mut delay_fm = 1.0;
                    delay_fm += self.dispersion_noise * noise_amount;
                    delay_fm -= self.curved_bridge * bridge_curving;
                    delay *= delay_fm;

                    let ap_delay = delay * stretch_point;
                    let main_delay = delay - ap_delay;
                    if ap_delay >= 4.0 && main_delay >= 4.0 {
                        s = self.string.read_hermite(main_delay);
                        s = self.stretch.allpass(s, ap_delay as usize, ap_gain);
                    } else {
                        s = self.string.read_hermite(delay);
                    }
                    let mut s_ac = [s];
                    self.dc_blocker.process(&mut s_ac);
                    s += ac_blocking_amount * (s_ac[0] - s);

                    let value = s.abs() - 0.025;
                    let sign = if s > 0.0 { 1.0 } else { -1.5 };
                    self.curved_bridge = (value.abs() + value) * sign;
                } else {
                    s = self.string.read_hermite(delay);
                }

                s += input[i];
                s = self.fir_damping_filter.process(s);
                s = self.iir_damping_filter.process(FilterMode::LowPass, s);
                self.string.write(s);

                self.out_sample[1] = self.out_sample[0];
                self.aux_sample[1] = self.aux_sample[0];

                self.out_sample[0] = s;
                self.aux_sample[0] = self.string.read_frac(comb_delay);
            }
            out[i] += crossfade(self.out_sample[1], self.out_sample[0], self.src_phase);
            aux[i] += crossfade(self.aux_sample[1], self.aux_sample[0], self.src_phase);
        }
    }
}

/// `Interpolate(table, index, 1.0f)` -- `index` is used directly.
#[inline]
fn interpolate_1(table: &[f32], index: f32) -> f32 {
    let integral = index as usize;
    let fractional = index - integral as f32;
    let a = table[integral];
    let b = table[integral + 1];
    a + (b - a) * fractional
}
