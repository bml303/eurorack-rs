//! `plaits/dsp/engine/particle_engine.h` -- 6 filtered random-impulse
//! [`Particle`] voices, low-passed, then run through a granular [`Diffuser`].

use stmlib::filter::{FilterMode, FrequencyApproximation, Svf};
use stmlib::units::semitones_to_ratio;

use crate::engine::{note_to_frequency, trigger_state, Engine, EngineParameters, PostProcessingSettings};
use crate::fx::Diffuser;
use crate::noise::Particle;

const NUM_PARTICLES: usize = 6;

pub struct ParticleEngine {
    particle: [Particle; NUM_PARTICLES],
    diffuser: Diffuser,
    post_filter: Svf,
}

impl Default for ParticleEngine {
    fn default() -> Self {
        Self {
            particle: [Particle::default(); NUM_PARTICLES],
            diffuser: Diffuser::default(),
            post_filter: Svf::default(),
        }
    }
}

impl Engine for ParticleEngine {
    fn init(&mut self) {
        for p in self.particle.iter_mut() {
            p.init();
        }
        self.diffuser.init();
        self.post_filter.init();
    }

    fn reset(&mut self) {
        self.diffuser.reset();
    }

    fn load_user_data(&mut self, _user_data: Option<&'static [u8]>) {}

    fn render(
        &mut self,
        parameters: &EngineParameters,
        out: &mut [f32],
        aux: &mut [f32],
        already_enveloped: bool,
    ) -> bool {
        let f0 = note_to_frequency(parameters.note);
        let density_sqrt = note_to_frequency(60.0 + parameters.timbre * parameters.timbre * 72.0);
        let density = density_sqrt * density_sqrt * (1.0 / NUM_PARTICLES as f32);
        let gain = 1.0 / density;
        let q_sqrt = semitones_to_ratio(if parameters.morph >= 0.5 {
            (parameters.morph - 0.5) * 120.0
        } else {
            0.0
        });
        let q = 0.5 + q_sqrt * q_sqrt;
        let spread = 48.0 * parameters.harmonics * parameters.harmonics;
        let raw_diffusion_sqrt = 2.0 * (parameters.morph - 0.5).abs();
        let raw_diffusion = raw_diffusion_sqrt * raw_diffusion_sqrt;
        let diffusion = if parameters.morph < 0.5 { raw_diffusion } else { 0.0 };
        let sync = parameters.trigger & trigger_state::RISING_EDGE != 0;

        out.fill(0.0);
        aux.fill(0.0);

        for p in self.particle.iter_mut() {
            p.render(sync, density, gain, f0, spread, q, out, aux);
        }

        self.post_filter.set_f_q(f0.min(0.49), 0.5, FrequencyApproximation::Dirty);
        self.post_filter.process_in_place(FilterMode::LowPass, out);

        self.diffuser.process(0.8 * diffusion * diffusion, 0.5 * diffusion + 0.25, out);
        already_enveloped
    }

    fn post_processing_settings(&self) -> PostProcessingSettings {
        PostProcessingSettings {
            out_gain: -2.0,
            aux_gain: 1.0,
            already_enveloped: false,
        }
    }
}
