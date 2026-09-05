//! `plaits/dsp/engine/swarm_engine.h` -- a swarm of 8 detuned
//! saw+sine grain voices, morphing between a granular cloud and a cluster of
//! glissandi.

use stmlib::fdsp::one_pole;
use stmlib::parameter_interpolator::ParameterInterpolator;
use stmlib::polyblep::{next_blep_sample, this_blep_sample};
use stmlib::units::semitones_to_ratio;
use stmlib::Random;

use crate::engine::{
    note_to_frequency, trigger_state, Engine, EngineParameters, PostProcessingSettings,
};
use crate::oscillator::{sine, FastSineOscillator, MAX_FREQUENCY};

const NUM_SWARM_VOICES: usize = 8;

#[derive(Debug, Clone, Copy)]
struct GrainEnvelope {
    from: f32,
    interval: f32,
    phase: f32,
    fm: f32,
    amplitude: f32,
    previous_size_ratio: f32,
    filter_coefficient: f32,
}

impl Default for GrainEnvelope {
    fn default() -> Self {
        Self {
            from: 0.0,
            interval: 1.0,
            phase: 1.0,
            fm: 0.0,
            amplitude: 0.5,
            previous_size_ratio: 0.0,
            filter_coefficient: 0.0,
        }
    }
}

impl GrainEnvelope {
    fn init(&mut self) {
        *self = Self::default();
    }

    fn step(&mut self, rate: f32, burst_mode: bool, start_burst: bool) {
        let mut randomize = false;
        if start_burst {
            self.phase = 0.5;
            self.fm = 16.0;
            randomize = true;
        } else {
            self.phase += rate * self.fm;
            if self.phase >= 1.0 {
                self.phase -= (self.phase as i32) as f32;
                randomize = true;
            }
        }

        if randomize {
            self.from += self.interval;
            self.interval = Random::get_float() - self.from;
            if burst_mode {
                self.fm *= 0.8 + 0.2 * Random::get_float();
            } else {
                self.fm = 0.5 + 1.5 * Random::get_float();
            }
        }
    }

    fn frequency(&self, size_ratio: f32) -> f32 {
        // Approximate two overlapping grains at f1/f2 by a continuous tone
        // ramping between them -- lets the grain-cloud and glissando-swarm
        // textures blend continuously.
        if size_ratio < 1.0 {
            2.0 * (self.from + self.interval * self.phase) - 1.0
        } else {
            self.from
        }
    }

    fn amplitude(&mut self, size_ratio: f32) -> f32 {
        let mut target_amplitude = 1.0f32;
        if size_ratio >= 1.0 {
            let phase = ((self.phase - 0.5) * size_ratio).clamp(-1.0, 1.0);
            let e = sine(0.5 * phase + 1.25);
            target_amplitude = 0.5 * (e + 1.0);
        }

        if (size_ratio >= 1.0) ^ (self.previous_size_ratio >= 1.0) {
            self.filter_coefficient = 0.5;
        }
        self.filter_coefficient *= 0.95;

        self.previous_size_ratio = size_ratio;
        one_pole(
            &mut self.amplitude,
            target_amplitude,
            0.5 - self.filter_coefficient,
        );
        self.amplitude
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct AdditiveSawOscillator {
    phase: f32,
    next_sample: f32,
    frequency: f32,
    gain: f32,
}

impl AdditiveSawOscillator {
    fn init(&mut self) {
        self.phase = 0.0;
        self.next_sample = 0.0;
        self.frequency = 0.01;
        self.gain = 0.0;
    }

    fn render(&mut self, mut frequency: f32, level: f32, out: &mut [f32]) {
        if frequency >= MAX_FREQUENCY {
            frequency = MAX_FREQUENCY;
        }
        let size = out.len();
        let mut fm = ParameterInterpolator::new(&mut self.frequency, frequency, size);
        let mut gain = ParameterInterpolator::new(&mut self.gain, level, size);

        let mut next_sample = self.next_sample;
        let mut phase = self.phase;

        for o in out.iter_mut() {
            let mut this_sample = next_sample;
            next_sample = 0.0;

            let frequency = fm.next();
            phase += frequency;

            if phase >= 1.0 {
                phase -= 1.0;
                let t = phase / frequency;
                this_sample -= this_blep_sample(t);
                next_sample -= next_blep_sample(t);
            }

            next_sample += phase;
            *o += (2.0 * this_sample - 1.0) * gain.next();
        }
        self.phase = phase;
        self.next_sample = next_sample;
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct SwarmVoice {
    rank: f32,
    envelope: GrainEnvelope,
    saw: AdditiveSawOscillator,
    sine: FastSineOscillator,
}

impl SwarmVoice {
    fn init(&mut self, rank: f32) {
        self.rank = rank;
        self.envelope.init();
        self.saw.init();
        self.sine.init();
    }

    #[allow(clippy::too_many_arguments)]
    fn render(
        &mut self,
        mut f0: f32,
        density: f32,
        burst_mode: bool,
        start_burst: bool,
        spread: f32,
        size_ratio: f32,
        saw: &mut [f32],
        sine: &mut [f32],
    ) {
        self.envelope.step(density, burst_mode, start_burst);

        let scale = 1.0 / NUM_SWARM_VOICES as f32;
        let amplitude = self.envelope.amplitude(size_ratio) * scale;

        let expo_amount = self.envelope.frequency(size_ratio);
        f0 *= semitones_to_ratio(48.0 * expo_amount * spread * self.rank);

        let linear_amount = self.rank * (self.rank + 0.01) * spread * 0.25;
        f0 *= 1.0 + linear_amount;

        self.saw.render(f0, amplitude, saw);
        self.sine.render_additive(f0, amplitude, sine);
    }
}

#[derive(Debug)]
pub struct SwarmEngine {
    swarm_voice: [SwarmVoice; NUM_SWARM_VOICES],
}

impl Default for SwarmEngine {
    fn default() -> Self {
        Self {
            swarm_voice: [SwarmVoice::default(); NUM_SWARM_VOICES],
        }
    }
}

impl Engine for SwarmEngine {
    fn init(&mut self) {
        self.swarm_voice = [SwarmVoice::default(); NUM_SWARM_VOICES];
    }

    fn reset(&mut self) {
        let n = (NUM_SWARM_VOICES as f32 - 1.0) / 2.0;
        for (i, voice) in self.swarm_voice.iter_mut().enumerate() {
            let rank = (i as f32 - n) / n;
            voice.init(rank);
        }
    }

    fn load_user_data(&mut self, _user_data: Option<&'static [u8]>) {}

    fn render(
        &mut self,
        parameters: &EngineParameters,
        out: &mut [f32],
        aux: &mut [f32],
        already_enveloped: bool,
    ) -> bool {
        let size = out.len();
        let f0 = note_to_frequency(parameters.note);
        let control_rate = size as f32;
        let density = note_to_frequency(parameters.timbre * 120.0) * 0.025 * control_rate;
        let spread = parameters.harmonics * parameters.harmonics * parameters.harmonics;
        let mut size_ratio = 0.25 * semitones_to_ratio((1.0 - parameters.morph) * 84.0);

        let burst_mode = parameters.trigger & trigger_state::UNPATCHED == 0;
        let start_burst = parameters.trigger & trigger_state::RISING_EDGE != 0;

        out.fill(0.0);
        aux.fill(0.0);

        for voice in self.swarm_voice.iter_mut() {
            voice.render(
                f0,
                density,
                burst_mode,
                start_burst,
                spread,
                size_ratio,
                out,
                aux,
            );
            size_ratio *= 0.97;
        }
        already_enveloped
    }

    fn post_processing_settings(&self) -> PostProcessingSettings {
        PostProcessingSettings {
            out_gain: -3.0,
            aux_gain: 1.0,
            already_enveloped: false,
        }
    }
}
