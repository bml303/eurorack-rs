//! `plaits/dsp/physical_modelling/string_voice.h` -- extended Karplus-Strong,
//! with the excitation/non-linearity niceties from Rings.

use stmlib::filter::{FilterMode, FrequencyApproximation, Svf};
use stmlib::units::semitones_to_ratio;
use stmlib::Random;

use super::string::String;
use crate::noise::dust;

#[derive(Default, Debug)]
pub struct StringVoice {
    excitation_filter: Svf,
    string: String,
    remaining_noise_samples: usize,
}

impl StringVoice {
    pub fn init(&mut self) {
        self.excitation_filter.init();
        self.string.init();
        self.remaining_noise_samples = 0;
    }

    pub fn reset(&mut self) {
        self.string.reset();
    }

    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        sustain: bool,
        trigger: bool,
        accent: f32,
        f0: f32,
        structure: f32,
        mut brightness: f32,
        mut damping: f32,
        temp: &mut [f32],
        out: &mut [f32],
        aux: &mut [f32],
    ) {
        let density = brightness * brightness;

        brightness += 0.25 * accent * (1.0 - brightness);
        damping += 0.25 * accent * (1.0 - damping);

        // Synthesize the excitation signal.
        if trigger || sustain {
            let range = 72.0;
            let f = 4.0 * f0;
            let cutoff = (f * semitones_to_ratio((brightness * (2.0 - brightness) - 0.5) * range))
                .min(0.499);
            let q = if sustain { 1.0 } else { 0.5 };
            self.remaining_noise_samples = (1.0 / f0) as usize;
            self.excitation_filter
                .set_f_q(cutoff, q, FrequencyApproximation::Dirty);
        }

        if sustain {
            let dust_f = 0.00005 + 0.99995 * density * density;
            for t in temp.iter_mut() {
                *t = dust(dust_f) * (8.0 - dust_f * 6.0) * accent;
            }
        } else if self.remaining_noise_samples != 0 {
            let noise_samples = self.remaining_noise_samples.min(temp.len());
            self.remaining_noise_samples -= noise_samples;
            for t in temp[..noise_samples].iter_mut() {
                *t = 2.0 * Random::get_float() - 1.0;
            }
            for t in temp[noise_samples..].iter_mut() {
                *t = 0.0;
            }
        } else {
            temp.fill(0.0);
        }

        self.excitation_filter
            .process_in_place(FilterMode::LowPass, temp);
        for i in 0..temp.len() {
            aux[i] += temp[i];
        }

        let non_linearity = if structure < 0.24 {
            (structure - 0.24) * 4.166
        } else if structure > 0.26 {
            (structure - 0.26) * 1.35135
        } else {
            0.0
        };
        self.string
            .process(f0, non_linearity, brightness, damping, temp, out);
    }
}
