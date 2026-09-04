//! `plaits/dsp/physical_modelling/string.h` -- a Karplus-Strong-style comb
//! filter / plucked string, "lite" version of the one used in Rings, with two
//! interchangeable non-linearities (a curved bridge, or dispersion).
//!
//! Uses `stmlib::DelayLine` (the plaits-local `DelayLine` in the C differs
//! from stmlib's only in *not* owning its buffer, so a Rust port -- which
//! doesn't need the external `BufferAllocator` dance -- can just reuse the
//! stmlib one directly).

use stmlib::fdsp::{crossfade, interpolate, one_pole};
use stmlib::filter::{DcBlocker, FilterMode, FrequencyApproximation, Svf};
use stmlib::parameter_interpolator::ParameterInterpolator;
use stmlib::units::semitones_to_ratio;
use stmlib::DelayLine;
use stmlib::Random;

use crate::dsp::SAMPLE_RATE;
use crate::resources::LUT_SVF_SHIFT;

pub const DELAY_LINE_SIZE: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StringNonLinearity {
    CurvedBridge,
    Dispersion,
}

pub struct String {
    string: DelayLine<DELAY_LINE_SIZE>,
    stretch: DelayLine<{ DELAY_LINE_SIZE / 4 }>,
    iir_damping_filter: Svf,
    dc_blocker: DcBlocker,
    delay: f32,
    dispersion_noise: f32,
    curved_bridge: f32,
    src_phase: f32,
    out_sample: [f32; 2],
}

impl Default for String {
    fn default() -> Self {
        Self {
            string: DelayLine::default(),
            stretch: DelayLine::default(),
            iir_damping_filter: Svf::default(),
            dc_blocker: DcBlocker::default(),
            delay: 100.0,
            dispersion_noise: 0.0,
            curved_bridge: 0.0,
            src_phase: 0.0,
            out_sample: [0.0; 2],
        }
    }
}

impl String {
    pub fn init(&mut self) {
        self.delay = 100.0;
        self.reset();
    }

    pub fn reset(&mut self) {
        self.string.reset();
        self.stretch.reset();
        self.iir_damping_filter.init();
        self.dc_blocker.init(1.0 - 20.0 / SAMPLE_RATE);
        self.dispersion_noise = 0.0;
        self.curved_bridge = 0.0;
        self.out_sample = [0.0, 0.0];
        self.src_phase = 0.0;
    }

    pub fn process(
        &mut self,
        f0: f32,
        non_linearity_amount: f32,
        brightness: f32,
        damping: f32,
        input: &[f32],
        out: &mut [f32],
    ) {
        if non_linearity_amount <= 0.0 {
            self.process_internal(
                StringNonLinearity::CurvedBridge,
                f0,
                -non_linearity_amount,
                brightness,
                damping,
                input,
                out,
            );
        } else {
            self.process_internal(
                StringNonLinearity::Dispersion,
                f0,
                non_linearity_amount,
                brightness,
                damping,
                input,
                out,
            );
        }
    }

    fn process_internal(
        &mut self,
        non_linearity: StringNonLinearity,
        f0: f32,
        non_linearity_amount: f32,
        brightness: f32,
        damping: f32,
        input: &[f32],
        out: &mut [f32],
    ) {
        let delay = (1.0 / f0).clamp(4.0, DELAY_LINE_SIZE as f32 - 4.0);

        // Corner case (f0 < 11.7 Hz): not enough delay time in the line, so
        // play at the lowest possible note and upsample on the fly with a
        // crude linear interpolator.
        let mut src_ratio = delay * f0;
        if src_ratio >= 0.9999 {
            self.src_phase = 1.0;
            src_ratio = 1.0;
        }

        let mut damping_cutoff = (12.0 + damping * damping * 60.0 + brightness * 24.0).min(84.0);
        let mut damping_f = (f0 * semitones_to_ratio(damping_cutoff)).min(0.499);

        let mut brightness = brightness;
        // Crossfade to infinite decay.
        if damping >= 0.95 {
            let to_infinite = 20.0 * (damping - 0.95);
            brightness += to_infinite * (1.0 - brightness);
            damping_f += to_infinite * (0.4999 - damping_f);
            damping_cutoff += to_infinite * (128.0 - damping_cutoff);
        }

        self.iir_damping_filter.set_f_q(damping_f, 0.5, FrequencyApproximation::Fast);

        let damping_compensation = interpolate(&LUT_SVF_SHIFT, damping_cutoff, 1.0);

        let size = input.len();
        let mut delay_modulation =
            ParameterInterpolator::new(&mut self.delay, delay * damping_compensation, size);

        let stretch_point = non_linearity_amount * (2.0 - non_linearity_amount) * 0.225;
        let stretch_correction = ((160.0 / SAMPLE_RATE) * delay).clamp(1.0, 2.1);

        let noise_amount_sqrt = if non_linearity_amount > 0.75 {
            4.0 * (non_linearity_amount - 0.75)
        } else {
            0.0
        };
        let noise_amount = noise_amount_sqrt * noise_amount_sqrt * 0.1;
        let noise_filter = 0.06 + 0.94 * brightness * brightness;

        let bridge_curving_sqrt = non_linearity_amount;
        let bridge_curving = bridge_curving_sqrt * bridge_curving_sqrt * 0.01;

        let ap_gain = -0.618 * non_linearity_amount / (0.15 + non_linearity_amount.abs());

        for (i, o) in out.iter_mut().enumerate() {
            self.src_phase += src_ratio;
            if self.src_phase > 1.0 {
                self.src_phase -= 1.0;

                let mut delay = delay_modulation.next();
                let mut s;

                if non_linearity == StringNonLinearity::Dispersion {
                    let noise = Random::get_float() - 0.5;
                    one_pole(&mut self.dispersion_noise, noise, noise_filter);
                    delay *= 1.0 + self.dispersion_noise * noise_amount;
                } else {
                    delay *= 1.0 - self.curved_bridge * bridge_curving;
                }

                if non_linearity == StringNonLinearity::Dispersion {
                    let ap_delay = delay * stretch_point;
                    let main_delay =
                        delay - ap_delay * (0.408 - stretch_point * 0.308) * stretch_correction;
                    if ap_delay >= 4.0 && main_delay >= 4.0 {
                        s = self.string.read_frac(main_delay);
                        s = self.stretch.allpass(s, ap_delay as usize, ap_gain);
                    } else {
                        s = self.string.read_hermite(delay);
                    }
                } else {
                    s = self.string.read_hermite(delay);
                }

                if non_linearity == StringNonLinearity::CurvedBridge {
                    let value = s.abs() - 0.025;
                    let sign = if s > 0.0 { 1.0 } else { -1.5 };
                    self.curved_bridge = (value.abs() + value) * sign;
                }

                s += input[i];
                s = s.clamp(-20.0, 20.0);

                let mut buf = [s];
                self.dc_blocker.process(&mut buf);
                s = self.iir_damping_filter.process(FilterMode::LowPass, buf[0]);
                self.string.write(s);

                self.out_sample[1] = self.out_sample[0];
                self.out_sample[0] = s;
            }
            *o += crossfade(self.out_sample[1], self.out_sample[0], self.src_phase);
        }
    }
}
