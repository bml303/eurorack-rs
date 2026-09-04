//! `plaits/dsp/noise/particle.h` -- a random impulse train through a
//! resonant band-pass, re-randomised (frequency, gain) each time an impulse
//! fires; the filter's cutoff/Q stay fixed for the rest of the block.

use stmlib::fdsp::sqrt;
use stmlib::filter::{FilterMode, FrequencyApproximation, Svf};
use stmlib::units::semitones_to_ratio;
use stmlib::Random;

#[derive(Debug, Clone, Copy, Default)]
pub struct Particle {
    pre_gain: f32,
    filter: Svf,
}

impl Particle {
    pub fn init(&mut self) {
        self.pre_gain = 0.0;
        self.filter.init();
    }

    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        sync: bool,
        density: f32,
        gain: f32,
        frequency: f32,
        spread: f32,
        q: f32,
        out: &mut [f32],
        aux: &mut [f32],
    ) {
        let mut u = Random::get_float();
        if sync {
            u = density;
        }
        let mut can_randomize_frequency = true;
        for i in 0..out.len() {
            let mut s = 0.0;
            if u <= density {
                s = u * gain;
                if can_randomize_frequency {
                    let u = 2.0 * Random::get_float() - 1.0;
                    let f = (semitones_to_ratio(spread * u) * frequency).min(0.25);
                    self.pre_gain = 0.5 / sqrt(q * f * sqrt(density));
                    self.filter.set_f_q(f, q, FrequencyApproximation::Dirty);
                    // Keep the cutoff constant for this whole block.
                    can_randomize_frequency = false;
                }
            }
            aux[i] += s;
            out[i] += self.filter.process(FilterMode::BandPass, self.pre_gain * s);
            u = Random::get_float();
        }
    }
}
