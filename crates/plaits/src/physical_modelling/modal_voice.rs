//! `plaits/dsp/physical_modelling/modal_voice.h` -- a mallet exciter (click ->
//! low-pass -> resonator); the click is replaced by continuous noise when
//! sustaining (unpatched trigger).

use stmlib::filter::FilterMode;
use stmlib::units::semitones_to_ratio;

use super::resonator::{Resonator, ResonatorSvf, MAX_NUM_MODES};
use crate::noise::dust;

#[derive(Default, Debug)]
pub struct ModalVoice {
    excitation_filter: ResonatorSvf<1>,
    resonator: Resonator,
}

impl ModalVoice {
    pub fn init(&mut self) {
        self.excitation_filter.init();
        self.resonator.init(0.015, MAX_NUM_MODES);
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
        temp_2: &mut [f32],
        out: &mut [f32],
        aux: &mut [f32],
    ) {
        let density = brightness * brightness;

        brightness += 0.25 * accent * (1.0 - brightness);
        damping += 0.25 * accent * (1.0 - damping);

        let range = if sustain { 36.0 } else { 60.0 };
        let f = if sustain { 4.0 * f0 } else { 2.0 * f0 };
        let cutoff =
            (f * semitones_to_ratio((brightness * (2.0 - brightness) - 0.5) * range)).min(0.499);
        let q = if sustain { 0.7 } else { 1.5 };

        // Synthesize the excitation signal.
        if sustain {
            let dust_f = 0.00005 + 0.99995 * density * density;
            for t in temp.iter_mut() {
                *t = dust(dust_f) * (4.0 - dust_f * 3.0) * accent;
            }
        } else {
            temp.fill(0.0);
            if trigger {
                let attenuation = 1.0 - damping * 0.5;
                let amplitude = (0.12 + 0.08 * accent) * attenuation;
                temp[0] = amplitude * semitones_to_ratio(cutoff * cutoff * 24.0) / cutoff;
            }
        }

        // `Process<FILTER_MODE_LOW_PASS, false>(&cutoff, &q, &one, temp, temp, size)`
        // in the C -- an in-place call. Rust can't alias `temp` as both `&[f32]`
        // and `&mut [f32]`, so filter into a scratch buffer and copy back.
        self.excitation_filter.process(
            FilterMode::LowPass,
            false,
            &[cutoff],
            &[q],
            &[1.0],
            temp,
            temp_2,
        );

        for i in 0..temp.len() {
            aux[i] += temp_2[i];
        }

        self.resonator
            .process(f0, structure, brightness, damping, temp_2, out);
    }
}
