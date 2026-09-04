//! `plaits/dsp/drums/hi_hat.h` -- an 808-style hi-hat: 6-oscillator "metallic
//! noise" through a band-pass, blended with clocked white noise, then an
//! envelope-driven VCA and a high-pass.
//!
//! The C parametrises `HiHat` on 4 compile-time knobs (noise source, VCA
//! shape, `resonance`, `two_stage_envelope`); only 2 combinations are ever
//! instantiated (see `hi_hat_engine.cc`), so this port is those two concrete
//! configurations (`Vca::Swing`/`Vca::Linear` runtime enum) rather than a
//! generic type.

use stmlib::filter::{FilterMode, FrequencyApproximation, Svf};
use stmlib::parameter_interpolator::ParameterInterpolator;
use stmlib::units::semitones_to_ratio;
use stmlib::Random;

use crate::dsp::SAMPLE_RATE;
use crate::oscillator::{Oscillator, OscillatorShape};

/// 808-style "metallic noise": 6 square oscillators (nominal f0 414 Hz),
/// summed as a 1-bit-per-oscillator counter.
#[derive(Default)]
pub struct SquareNoise {
    phase: [u32; 6],
}

#[rustfmt::skip]
const RATIOS: [f32; 6] = [1.0, 1.304, 1.466, 1.787, 1.932, 2.536];

impl SquareNoise {
    pub fn init(&mut self) {
        self.phase = [0; 6];
    }

    pub fn render(&mut self, f0: f32, out: &mut [f32]) {
        let mut increment = [0u32; 6];
        for i in 0..6 {
            let f = (f0 * RATIOS[i]).min(0.499);
            increment[i] = (f * 4_294_967_296.0) as u32;
        }

        for o in out.iter_mut() {
            let mut noise = 0u32;
            for i in 0..6 {
                self.phase[i] = self.phase[i].wrapping_add(increment[i]);
                noise += self.phase[i] >> 31;
            }
            *o = 0.33 * noise as f32 - 1.0;
        }
    }
}

/// "KR-55/FM"-style metallic noise: 3 ring-modulated square*saw oscillator pairs.
pub struct RingModNoise {
    oscillator: [Oscillator; 6],
}

impl Default for RingModNoise {
    fn default() -> Self {
        Self {
            oscillator: [Oscillator::default(); 6],
        }
    }
}

impl RingModNoise {
    pub fn init(&mut self) {
        for o in self.oscillator.iter_mut() {
            o.init();
        }
    }

    pub fn render(&mut self, f0: f32, temp_1: &mut [f32], temp_2: &mut [f32], out: &mut [f32]) {
        let ratio = f0 / (0.01 + f0);
        let f1a = 200.0 / SAMPLE_RATE * ratio;
        let f1b = 7530.0 / SAMPLE_RATE * ratio;
        let f2a = 510.0 / SAMPLE_RATE * ratio;
        let f2b = 8075.0 / SAMPLE_RATE * ratio;
        let f3a = 730.0 / SAMPLE_RATE * ratio;
        let f3b = 10500.0 / SAMPLE_RATE * ratio;
        let f = [[f1a, f1b], [f2a, f2b], [f3a, f3b]];

        out.fill(0.0);

        for i in 0..3 {
            let (a, b) = self.oscillator.split_at_mut(2 * i + 1);
            let osc0 = &mut a[2 * i];
            let osc1 = &mut b[0];
            osc0.render(OscillatorShape::Square, f[i][0], 0.5, temp_1);
            osc1.render(OscillatorShape::Saw, f[i][1], 0.5, temp_2);
            for j in 0..out.len() {
                out[j] += temp_1[j] * temp_2[j];
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetallicNoise {
    Square,
    RingMod,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Vca {
    /// `SwingVCA`.
    Swing,
    /// `LinearVCA`.
    Linear,
}

#[inline]
fn apply_vca(vca: Vca, s: f32, gain: f32) -> f32 {
    match vca {
        Vca::Swing => {
            let s = s * if s > 0.0 { 4.0 } else { 0.1 };
            let s = s / (1.0 + s.abs());
            (s + 0.1) * gain
        }
        Vca::Linear => s * gain,
    }
}

pub struct HiHat {
    source: MetallicNoise,
    vca: Vca,
    resonance: bool,
    two_stage_envelope: bool,

    envelope: f32,
    noise_clock: f32,
    noise_sample: f32,
    sustain_gain: f32,

    square_noise: SquareNoise,
    ring_mod_noise: RingModNoise,
    noise_coloration_svf: Svf,
    hpf: Svf,
}

impl HiHat {
    pub fn new(source: MetallicNoise, vca: Vca, resonance: bool, two_stage_envelope: bool) -> Self {
        let mut h = Self {
            source,
            vca,
            resonance,
            two_stage_envelope,
            envelope: 0.0,
            noise_clock: 0.0,
            noise_sample: 0.0,
            sustain_gain: 0.0,
            square_noise: SquareNoise::default(),
            ring_mod_noise: RingModNoise::default(),
            noise_coloration_svf: Svf::default(),
            hpf: Svf::default(),
        };
        h.init();
        h
    }

    pub fn init(&mut self) {
        self.envelope = 0.0;
        self.noise_clock = 0.0;
        self.noise_sample = 0.0;
        self.sustain_gain = 0.0;
        self.square_noise.init();
        self.ring_mod_noise.init();
        self.noise_coloration_svf.init();
        self.hpf.init();
    }

    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        sustain: bool,
        trigger: bool,
        accent: f32,
        f0: f32,
        tone: f32,
        decay: f32,
        mut noisiness: f32,
        temp_1: &mut [f32],
        temp_2: &mut [f32],
        out: &mut [f32],
    ) {
        let envelope_decay = 1.0 - 0.003 * semitones_to_ratio(-decay * 84.0);
        let cut_decay = 1.0 - 0.0025 * semitones_to_ratio(-decay * 36.0);

        if trigger {
            self.envelope = (1.5 + 0.5 * (1.0 - decay)) * (0.3 + 0.7 * accent);
        }

        // Render the metallic noise.
        match self.source {
            MetallicNoise::Square => self.square_noise.render(2.0 * f0, out),
            MetallicNoise::RingMod => self.ring_mod_noise.render(2.0 * f0, temp_1, temp_2, out),
        }

        // Band-pass the metallic noise.
        let cutoff = (150.0 / SAMPLE_RATE * semitones_to_ratio(tone * 72.0)).clamp(0.0, 16_000.0 / SAMPLE_RATE);
        self.noise_coloration_svf.set_f_q(
            cutoff,
            if self.resonance { 3.0 + 3.0 * tone } else { 1.0 },
            FrequencyApproximation::Accurate,
        );
        self.noise_coloration_svf.process_in_place(FilterMode::BandPass, out);

        // Not part of the 808 circuit, but adds variety: blend in a variable
        // amount of clocked noise.
        noisiness *= noisiness;
        let noise_f = (f0 * (16.0 + 16.0 * (1.0 - noisiness))).clamp(0.0, 0.5);

        for o in out.iter_mut() {
            self.noise_clock += noise_f;
            if self.noise_clock >= 1.0 {
                self.noise_clock -= 1.0;
                self.noise_sample = Random::get_float() - 0.5;
            }
            *o += noisiness * (self.noise_sample - *o);
        }

        // Apply the VCA.
        let size = out.len();
        let mut sustain_gain = ParameterInterpolator::new(&mut self.sustain_gain, accent * decay, size);
        for o in out.iter_mut() {
            self.envelope *= if self.envelope > 0.5 || !self.two_stage_envelope {
                envelope_decay
            } else {
                cut_decay
            };
            let gain = if sustain { sustain_gain.next() } else { self.envelope };
            *o = apply_vca(self.vca, *o, gain);
        }

        self.hpf.set_f_q(cutoff, 0.5, FrequencyApproximation::Accurate);
        self.hpf.process_in_place(FilterMode::HighPass, out);
    }
}

