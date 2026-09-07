//! `elements/dsp/part.{h,cc}` + `patch.h` -- the top-level object: it owns the
//! [`Patch`], the modal [`Voice`] and the easter-egg [`OminousVoice`], mixes
//! their raw / centre / side signals into the stereo bus per the "space" macro,
//! soft-limits, meters, and runs the output [`Reverb`].
//!
//! Elements' firmware allows only one voice (`kNumVoices == 1`); this port
//! keeps that -- polyphony needs the mode count cut to 16 and "doesn't sound
//! very good" (the C's words).

use stmlib::fdsp::soft_limit;

use crate::dsp::{MAX_BLOCK_SIZE, SAMPLE_RATE};
use crate::fx::Reverb;
use crate::ominous_voice::OminousVoice;
use crate::resources::{LUT_MIDI_TO_F_HIGH, LUT_MIDI_TO_F_LOW};
use crate::voice::{ResonatorModel, Voice};

/// `elements::PerformanceState` -- the per-block control input.
#[derive(Debug, Clone, Copy, Default)]
pub struct PerformanceState {
    pub gate: bool,
    pub note: f32,
    pub modulation: f32,
    pub strength: f32,
}

/// `elements::Patch` -- the 19 synthesis parameters (all normalised `[0, 1]`
/// except the two modulation values, which are in cycles/sample and semitones).
#[derive(Debug, Clone, Copy)]
pub struct Patch {
    pub exciter_envelope_shape: f32,
    pub exciter_bow_level: f32,
    pub exciter_bow_timbre: f32,
    pub exciter_blow_level: f32,
    pub exciter_blow_meta: f32,
    pub exciter_blow_timbre: f32,
    pub exciter_strike_level: f32,
    pub exciter_strike_meta: f32,
    pub exciter_strike_timbre: f32,
    pub exciter_signature: f32,
    pub resonator_geometry: f32,
    pub resonator_brightness: f32,
    pub resonator_damping: f32,
    pub resonator_position: f32,
    pub resonator_modulation_frequency: f32,
    pub resonator_modulation_offset: f32,
    pub reverb_diffusion: f32,
    pub reverb_lp: f32,
    pub space: f32,
    pub modulation_frequency: f32,
}

impl Default for Patch {
    /// The values `Part::Init` writes.
    fn default() -> Self {
        Self {
            exciter_envelope_shape: 1.0,
            exciter_bow_level: 0.0,
            exciter_bow_timbre: 0.5,
            exciter_blow_level: 0.0,
            exciter_blow_meta: 0.5,
            exciter_blow_timbre: 0.5,
            exciter_strike_level: 0.8,
            exciter_strike_meta: 0.5,
            exciter_strike_timbre: 0.5,
            exciter_signature: 0.0,
            resonator_geometry: 0.2,
            resonator_brightness: 0.5,
            resonator_damping: 0.25,
            resonator_position: 0.3,
            resonator_modulation_frequency: 0.5 / SAMPLE_RATE,
            resonator_modulation_offset: 0.1,
            reverb_diffusion: 0.625,
            reverb_lp: 0.7,
            space: 0.5,
            modulation_frequency: 0.0,
        }
    }
}

/// `elements::Part`.
pub struct Part {
    patch: Patch,
    voice: Voice,
    ominous_voice: OminousVoice,

    panic: bool,
    bypass: bool,
    easter_egg: bool,
    previous_gate: bool,
    note: f32,

    raw_buffer: [f32; MAX_BLOCK_SIZE],
    center_buffer: [f32; MAX_BLOCK_SIZE],
    sides_buffer: [f32; MAX_BLOCK_SIZE],

    scaled_exciter_level: f32,
    scaled_resonator_level: f32,
    resonator_level: f32,

    reverb: Reverb,
    resonator_model: ResonatorModel,
}

impl Default for Part {
    fn default() -> Self {
        Self::new()
    }
}

impl Part {
    /// `Part()` + `Init` -- ready to [`process`](Self::process).
    pub fn new() -> Self {
        let mut p = Self {
            patch: Patch::default(),
            voice: Voice::new(),
            ominous_voice: OminousVoice::new(),
            panic: false,
            bypass: false,
            easter_egg: false,
            previous_gate: false,
            note: 69.0,
            raw_buffer: [0.0; MAX_BLOCK_SIZE],
            center_buffer: [0.0; MAX_BLOCK_SIZE],
            sides_buffer: [0.0; MAX_BLOCK_SIZE],
            scaled_exciter_level: 0.0,
            scaled_resonator_level: 0.0,
            resonator_level: 0.0,
            reverb: Reverb::new(),
            resonator_model: ResonatorModel::Modal,
        };
        p.init();
        p
    }

    /// `Init`.
    pub fn init(&mut self) {
        self.patch = Patch::default();
        self.previous_gate = false;
        self.note = 69.0;
        self.voice.init();
        self.ominous_voice.init();
        self.reverb.init();
        self.scaled_exciter_level = 0.0;
        self.scaled_resonator_level = 0.0;
        self.resonator_level = 0.0;
        self.panic = false;
        self.bypass = false;
        self.resonator_model = ResonatorModel::Modal;
    }

    #[inline]
    pub fn patch(&self) -> &Patch {
        &self.patch
    }
    #[inline]
    pub fn patch_mut(&mut self) -> &mut Patch {
        &mut self.patch
    }

    /// `exciter_level()` -- the C returns the *scaled* level from this getter.
    #[inline]
    #[allow(clippy::misnamed_getters)]
    pub fn exciter_level(&self) -> f32 {
        self.scaled_exciter_level
    }
    /// `resonator_level()` -- the C returns the *scaled* level from this getter.
    #[inline]
    #[allow(clippy::misnamed_getters)]
    pub fn resonator_level(&self) -> f32 {
        self.scaled_resonator_level
    }
    #[inline]
    pub fn gate(&self) -> bool {
        self.previous_gate
    }
    #[inline]
    pub fn bypass(&self) -> bool {
        self.bypass
    }
    #[inline]
    pub fn set_bypass(&mut self, bypass: bool) {
        self.bypass = bypass;
    }
    #[inline]
    pub fn easter_egg(&self) -> bool {
        self.easter_egg
    }
    #[inline]
    pub fn set_easter_egg(&mut self, easter_egg: bool) {
        self.easter_egg = easter_egg;
    }
    #[inline]
    pub fn resonator_model(&self) -> ResonatorModel {
        self.resonator_model
    }
    #[inline]
    pub fn set_resonator_model(&mut self, r: ResonatorModel) {
        self.resonator_model = r;
    }

    /// `Panic` -- request a resonator-state reset on the next block.
    pub fn panic(&mut self) {
        self.panic = true;
    }

    /// `Seed(seed, size)` -- derive the per-unit "signature" tweaks from the
    /// MCU serial number.
    pub fn seed(&mut self, seed: &[u32]) {
        let mut signature: u32 = 0xf0ca_cc1a;
        for &s in seed {
            signature ^= s;
            signature = signature
                .wrapping_mul(1_664_525)
                .wrapping_add(1_013_904_223);
        }

        let take = |signature: &mut u32| -> f32 {
            let x = (*signature & 7) as f32 / 8.0;
            *signature >>= 3;
            x
        };

        let x = take(&mut signature);
        self.patch.resonator_modulation_frequency = (0.4 + 0.8 * x) / SAMPLE_RATE;
        let x = take(&mut signature);
        self.patch.resonator_modulation_offset = 0.05 + 0.1 * x;
        let x = take(&mut signature);
        self.patch.reverb_diffusion = 0.55 + 0.15 * x;
        let x = take(&mut signature);
        self.patch.reverb_lp = 0.7 + 0.2 * x;
        let x = take(&mut signature);
        self.patch.exciter_signature = x;
    }

    /// `Process`.
    pub fn process(
        &mut self,
        performance_state: &PerformanceState,
        blow_in: &[f32],
        strike_in: &[f32],
        main: &mut [f32],
        aux: &mut [f32],
        size: usize,
    ) {
        // Bypass / panic: pass the inputs through.
        if self.bypass || self.panic {
            if self.panic {
                self.voice.panic();
                self.resonator_level = 0.0;
                self.panic = false;
            }
            aux[..size].copy_from_slice(&blow_in[..size]);
            main[..size].copy_from_slice(&strike_in[..size]);
            return;
        }

        // (The C cycles `active_voice_` on a new gate here; with `kNumVoices == 1`
        // there is nothing to cycle to.)
        self.previous_gate = performance_state.gate;
        self.note = performance_state.note;
        main[..size].fill(0.0);
        aux[..size].fill(0.0);

        // "space" macro -> raw gain, stereo spread, reverb send/time.
        let mut space = if self.patch.space >= 1.0 {
            1.0
        } else {
            self.patch.space
        };
        let raw_gain = if space <= 0.05 {
            1.0
        } else if space <= 0.1 {
            2.0 - space * 20.0
        } else {
            0.0
        };
        space = if space >= 0.1 { space - 0.1 } else { 0.0 };
        let spread = if space <= 0.7 { space } else { 0.7 };
        let reverb_amount = if space >= 0.5 { space - 0.5 } else { 0.0 };
        let reverb_time = 0.35 + 1.2 * reverb_amount;

        let midi_pitch = self.note + performance_state.modulation;
        if self.easter_egg {
            self.ominous_voice.process(
                &self.patch,
                midi_pitch,
                performance_state.strength,
                performance_state.gate,
                blow_in,
                strike_in,
                &mut self.raw_buffer,
                &mut self.center_buffer,
                &mut self.sides_buffer,
                size,
            );
        } else {
            let mut pitch = ((midi_pitch + 48.0) * 256.0) as i32;
            pitch = pitch.clamp(0, 65535);
            let frequency = LUT_MIDI_TO_F_HIGH[(pitch >> 8) as usize]
                * LUT_MIDI_TO_F_LOW[(pitch & 0xff) as usize];
            self.voice.set_resonator_model(self.resonator_model);
            self.voice.process(
                &self.patch,
                frequency,
                performance_state.strength,
                performance_state.gate,
                blow_in,
                strike_in,
                &mut self.raw_buffer,
                &mut self.center_buffer,
                &mut self.sides_buffer,
                size,
            );
        }

        // Mixdown.
        for j in 0..size {
            let side = self.sides_buffer[j] * spread;
            let r = self.center_buffer[j] - side;
            let l = self.center_buffer[j] + side;
            main[j] += r;
            aux[j] += l + (self.raw_buffer[j] - l) * raw_gain;
        }

        // Pre-clipping.
        if !self.easter_egg {
            for j in 0..size {
                main[j] = soft_limit(main[j]);
                aux[j] = soft_limit(aux[j]);
            }
        }

        // Metering.
        let mut exciter_level = self.voice.exciter_level();
        let mut resonator_level = self.resonator_level;
        for j in 0..size {
            let error = main[j] * main[j] - resonator_level;
            resonator_level += error * if error > 0.0 { 0.05 } else { 0.0005 };
        }
        self.resonator_level = resonator_level;
        if resonator_level >= 200.0 {
            self.panic = true;
        }

        if self.easter_egg {
            let l = (self.patch.exciter_blow_level + self.patch.exciter_strike_level) * 0.5;
            self.scaled_exciter_level = l * (2.0 - l);
        } else {
            exciter_level *= 16.0;
            self.scaled_exciter_level = if exciter_level > 0.1 {
                1.0
            } else {
                exciter_level
            };
        }

        resonator_level *= 16.0;
        self.scaled_resonator_level = if resonator_level < 1.0 {
            resonator_level
        } else {
            1.0
        };

        // Reverb.
        self.reverb.set_amount(reverb_amount);
        self.reverb.set_diffusion(self.patch.reverb_diffusion);
        let freeze = self.patch.space >= 1.75;
        if freeze {
            self.reverb.set_time(1.0);
            self.reverb.set_input_gain(0.0);
            self.reverb.set_lp(1.0);
        } else {
            self.reverb.set_time(reverb_time);
            self.reverb.set_input_gain(0.2);
            self.reverb.set_lp(self.patch.reverb_lp);
        }
        self.reverb.process(&mut main[..size], &mut aux[..size]);
    }
}
