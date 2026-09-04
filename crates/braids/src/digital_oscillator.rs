//! `braids/digital_oscillator.{h,cc}` -- the ~35 "digital" synthesis models.
//!
//! Faithful fixed-point port of every model. Structural changes only:
//!
//! * The C `union DigitalOscillatorState` (a RAM optimisation for the MCU) is a
//!   plain flat struct here -- models are mutually exclusive and [`init`] zeroes
//!   everything on every shape change, so the aliasing the union relied on is a
//!   non-issue. Likewise the `union` of delay lines becomes named fields.
//! * The C `fn_table_` function-pointer dispatch becomes a `match` on
//!   [`DigitalModel`] (whose discriminants are exactly the `fn_table_` indices).
//! * `divide by (x >> n)` sites that are UB in C for sub-audio pitches use
//!   [`safe_div`], which yields `0` there; unaffected at real note pitches.
//!
//! [`init`]: DigitalOscillator::init

use stmlib::clip16_sym;
use stmlib::fixed::{
    crossfade_u8, interpolate_1022, interpolate_824_i16, interpolate_824_u16, interpolate_88_i16,
    mix_i16,
};
use stmlib::Random;

use crate::dsp::{compute_delay, compute_phase_increment_digital, ParamRamp, HIGHEST_NOTE_DIGITAL};
use crate::excitation::Excitation;
use crate::resources::{
    LUT_BELL, LUT_BLOWING_ENVELOPE, LUT_BLOWING_JET, LUT_BOWING_ENVELOPE, LUT_BOWING_FRICTION,
    LUT_FLUTE_BODY_FILTER, LUT_FM_FREQUENCY_QUANTIZER, LUT_GRANULAR_ENVELOPE,
    LUT_GRANULAR_ENVELOPE_RATE, LUT_RESONATOR_COEFFICIENT, LUT_RESONATOR_SCALE, LUT_SVF_CUTOFF,
    LUT_SVF_DAMP, LUT_SVF_SCALE, WAV_FORMANT_SINE, WAV_FORMANT_SQUARE, WAV_SINE,
    WS_MODERATE_OVERDRIVE, WT_CODE, WT_MAP, WT_WAVES,
};
use crate::shapes::DigitalModel;
use crate::svf::{Svf, SvfMode};

const WG_BRIDGE_LENGTH: usize = 1024;
const WG_NECK_LENGTH: usize = 4096;
const WG_BORE_LENGTH: usize = 2048;
const WG_JET_LENGTH: usize = 1024;
const WG_FBORE_LENGTH: usize = 4096;
const COMB_DELAY_LENGTH: usize = 8192;

const NUM_FORMANTS: usize = 5;
const NUM_PLUCK_VOICES: usize = 3;
const NUM_BELL_PARTIALS: usize = 11;
const NUM_DRUM_PARTIALS: usize = 6;
const NUM_ADDITIVE_HARMONICS: usize = 12;

const LUT_BOWING_ENVELOPE_SIZE: usize = 752;
const LUT_BLOWING_ENVELOPE_SIZE: usize = 392;

const FIR4_COEFFICIENTS: [u32; 4] = [10530, 14751, 16384, 14751];
const FIR4_DC_OFFSET: u32 = 28208;

#[inline]
fn safe_div_u32(a: u32, b: u32) -> u32 {
    if b == 0 {
        0
    } else {
        a / b
    }
}

#[inline]
fn constrain_i32(v: i32, lo: i32, hi: i32) -> i32 {
    v.clamp(lo, hi)
}

/// All per-model scratch state. Zeroed by [`DigitalOscillator::init`].
#[derive(Debug, Clone)]
struct State {
    // ResoSquareState / bare modulator_phase / VowelSynthesizerState share these.
    modulator_phase: u32,
    modulator_phase_increment: u32,
    square_modulator_phase: u32,
    integrator: i32,
    polarity: bool,

    formant_increment: [u32; 3],
    formant_phase: [u32; 3],
    formant_amplitude: [u32; 3],
    consonant_frames: u16,
    vow_noise: u16,

    // SawSwarmState + WaveParaphonic reuse saw_phase.
    saw_phase: [u32; 6],
    saw_lp: i32,
    saw_bp: i32,

    hrm_amplitude: [i32; NUM_ADDITIVE_HARMONICS],

    // AdditiveState (bell / drum).
    partial_phase: [u32; NUM_BELL_PARTIALS],
    partial_phase_increment: [u32; NUM_BELL_PARTIALS],
    partial_amplitude: [i32; NUM_BELL_PARTIALS],
    target_partial_amplitude: [i32; NUM_BELL_PARTIALS],
    add_previous_sample: i16,
    current_partial: usize,
    lp_noise: [i32; 3],

    // FeedbackFmState (also filtered-pitch scratch for RenderComb).
    ffm_modulator_phase: u32,
    ffm_previous_sample: i16,

    // ParticleNoiseState / TwinPeaksNoise.
    pno_amplitude: u16,
    pno_filter_state: [[i32; 2]; 3],
    pno_filter_scale: [i32; 3],
    pno_filter_coefficient: [i32; 3],

    // PhysicalModellingState.
    phy_delay_ptr: u16,
    phy_excitation_ptr: u16,
    phy_lp_state: i32,
    phy_filter_state: [i32; 2],
    phy_previous_sample: i16,

    // GranularCloud grains.
    grain_phase: [u32; 4],
    grain_phase_increment: [u32; 4],
    grain_envelope_phase: [u32; 4],
    grain_envelope_phase_increment: [u32; 4],

    // FofState.
    fof_next_saw_sample: i32,
    fof_previous_sample: i16,
    fof_svf_lp: [i32; NUM_FORMANTS],
    fof_svf_bp: [i32; NUM_FORMANTS],

    // ToyState.
    toy_held_sample: u8,
    toy_decimation_counter: u16,

    // SvfState (FilteredNoise / Kick lp).
    svf_bp: i32,
    svf_lp: i32,

    // DigitalModulationState.
    dmd_symbol_phase: u32,
    dmd_symbol_count: u16,
    dmd_filter_state: i32,
    dmd_data_byte: u8,

    // ClockedNoiseState / QuestionMark.
    clk_cycle_phase: u32,
    clk_cycle_phase_increment: u32,
    clk_rng_state: u32,
    clk_seed: i32,
    clk_sample: i16,

    // HatState (Cymbal).
    hat_phase: [u32; 6],
    hat_rng_state: u32,

    // PluckState[4].
    plk: [PluckState; 4],
}

#[derive(Debug, Clone, Copy, Default)]
struct PluckState {
    size: usize,
    write_ptr: usize,
    shift: usize,
    mask: usize,
    initialization_ptr: usize,
    phase: u32,
    phase_increment: u32,
    max_phase_increment: u32,
    previous_sample: i16,
}

impl State {
    fn zeroed() -> Self {
        State {
            modulator_phase: 0,
            modulator_phase_increment: 0,
            square_modulator_phase: 0,
            integrator: 0,
            polarity: false,
            formant_increment: [0; 3],
            formant_phase: [0; 3],
            formant_amplitude: [0; 3],
            consonant_frames: 0,
            vow_noise: 0,
            saw_phase: [0; 6],
            saw_lp: 0,
            saw_bp: 0,
            hrm_amplitude: [0; NUM_ADDITIVE_HARMONICS],
            partial_phase: [0; NUM_BELL_PARTIALS],
            partial_phase_increment: [0; NUM_BELL_PARTIALS],
            partial_amplitude: [0; NUM_BELL_PARTIALS],
            target_partial_amplitude: [0; NUM_BELL_PARTIALS],
            add_previous_sample: 0,
            current_partial: 0,
            lp_noise: [0; 3],
            ffm_modulator_phase: 0,
            ffm_previous_sample: 0,
            pno_amplitude: 0,
            pno_filter_state: [[0; 2]; 3],
            pno_filter_scale: [0; 3],
            pno_filter_coefficient: [0; 3],
            phy_delay_ptr: 0,
            phy_excitation_ptr: 0,
            phy_lp_state: 0,
            phy_filter_state: [0; 2],
            phy_previous_sample: 0,
            grain_phase: [0; 4],
            grain_phase_increment: [0; 4],
            grain_envelope_phase: [0; 4],
            grain_envelope_phase_increment: [0; 4],
            fof_next_saw_sample: 0,
            fof_previous_sample: 0,
            fof_svf_lp: [0; NUM_FORMANTS],
            fof_svf_bp: [0; NUM_FORMANTS],
            toy_held_sample: 0,
            toy_decimation_counter: 0,
            svf_bp: 0,
            svf_lp: 0,
            dmd_symbol_phase: 0,
            dmd_symbol_count: 0,
            dmd_filter_state: 0,
            dmd_data_byte: 0,
            clk_cycle_phase: 0,
            clk_cycle_phase_increment: 0,
            clk_rng_state: 0,
            clk_seed: 0,
            clk_sample: 0,
            hat_phase: [0; 6],
            hat_rng_state: 0,
            plk: [PluckState::default(); 4],
        }
    }
}

/// The digital oscillator. Large (embeds all delay lines); construct once.
pub struct DigitalOscillator {
    phase: u32,
    phase_increment: u32,
    delay: u32,
    parameter: [i16; 2],
    previous_parameter: [i16; 2],
    smoothed_parameter: i32,
    pitch: i16,
    active_voice: u8,
    init: bool,
    strike: bool,
    shape: DigitalModel,
    previous_shape: DigitalModel,
    state: State,
    pulse: [Excitation; 4],
    svf: [Svf; 3],

    // `union delay_lines_` -- widened to named fields (host DSP lib, RAM is free).
    comb: [i16; COMB_DELAY_LENGTH],
    ks: [i16; 1025 * 4],
    bowed_bridge: [i8; WG_BRIDGE_LENGTH],
    bowed_neck: [i8; WG_NECK_LENGTH],
    bore: [i16; WG_BORE_LENGTH],
    fluted_jet: [i8; WG_JET_LENGTH],
    fluted_bore: [i8; WG_FBORE_LENGTH],
}

impl Default for DigitalOscillator {
    fn default() -> Self {
        Self::new()
    }
}

impl DigitalOscillator {
    pub fn new() -> Self {
        let mut o = DigitalOscillator {
            phase: 0,
            phase_increment: 0,
            delay: 0,
            parameter: [0; 2],
            previous_parameter: [0; 2],
            smoothed_parameter: 0,
            pitch: 0,
            active_voice: 0,
            init: true,
            strike: true,
            shape: DigitalModel::TripleRingMod,
            previous_shape: DigitalModel::TripleRingMod,
            state: State::zeroed(),
            pulse: Default::default(),
            svf: Default::default(),
            comb: [0; COMB_DELAY_LENGTH],
            ks: [0; 1025 * 4],
            bowed_bridge: [0; WG_BRIDGE_LENGTH],
            bowed_neck: [0; WG_NECK_LENGTH],
            bore: [0; WG_BORE_LENGTH],
            fluted_jet: [0; WG_JET_LENGTH],
            fluted_bore: [0; WG_FBORE_LENGTH],
        };
        o.init();
        o
    }

    pub fn init(&mut self) {
        self.state = State::zeroed();
        for p in &mut self.pulse {
            p.init();
        }
        for s in &mut self.svf {
            s.init();
        }
        self.phase = 0;
        self.strike = true;
        self.init = true;
    }

    #[inline]
    pub fn set_shape(&mut self, shape: DigitalModel) {
        self.shape = shape;
    }

    #[inline]
    pub fn set_pitch(&mut self, pitch: i16) {
        // Smooth HF noise when the pitch CV is noisy.
        if self.pitch > (90 << 7) && pitch > (90 << 7) {
            self.pitch = ((self.pitch as i32 + pitch as i32) >> 1) as i16;
        } else {
            self.pitch = pitch;
        }
    }

    #[inline]
    pub fn set_parameters(&mut self, parameter_1: i16, parameter_2: i16) {
        self.parameter[0] = parameter_1;
        self.parameter[1] = parameter_2;
    }

    #[inline]
    pub fn phase_increment(&self) -> u32 {
        self.phase_increment
    }

    #[inline]
    pub fn strike(&mut self) {
        self.strike = true;
    }

    /// `Render(sync, buffer, size)`.
    pub fn render(&mut self, sync: &[u8], buffer: &mut [i16], size: usize) {
        // Quantise parameter[1] for the FM family.
        if matches!(
            self.shape,
            DigitalModel::Fm | DigitalModel::FeedbackFm | DigitalModel::ChaoticFeedbackFm
        ) {
            let integral = (self.parameter[1] >> 8) as usize;
            let fractional = (self.parameter[1] & 255) as i32;
            let a = LUT_FM_FREQUENCY_QUANTIZER[integral] as i32;
            let b = LUT_FM_FREQUENCY_QUANTIZER[integral + 1] as i32;
            self.parameter[1] = (a + ((b - a) * fractional >> 8)) as i16;
        }

        if self.shape != self.previous_shape {
            self.init();
            self.previous_shape = self.shape;
            self.init = true;
        }

        self.phase_increment = compute_phase_increment_digital(self.pitch as i32);
        self.delay = compute_delay(self.pitch as i32);

        if self.pitch as i32 > HIGHEST_NOTE_DIGITAL {
            self.pitch = HIGHEST_NOTE_DIGITAL as i16;
        } else if self.pitch < 0 {
            self.pitch = 0;
        }

        match self.shape {
            DigitalModel::TripleRingMod => self.render_triple_ring_mod(sync, buffer, size),
            DigitalModel::SawSwarm => self.render_saw_swarm(sync, buffer, size),
            DigitalModel::Comb => self.render_comb(sync, buffer, size),
            DigitalModel::Toy => self.render_toy(sync, buffer, size),
            DigitalModel::DigitalFilterLp
            | DigitalModel::DigitalFilterPk
            | DigitalModel::DigitalFilterBp
            | DigitalModel::DigitalFilterHp => self.render_digital_filter(sync, buffer, size),
            DigitalModel::Vosim => self.render_vosim(sync, buffer, size),
            DigitalModel::Vowel => self.render_vowel(sync, buffer, size),
            DigitalModel::VowelFof => self.render_vowel_fof(sync, buffer, size),
            DigitalModel::Harmonics => self.render_harmonics(sync, buffer, size),
            DigitalModel::Fm => self.render_fm(sync, buffer, size),
            DigitalModel::FeedbackFm => self.render_feedback_fm(sync, buffer, size),
            DigitalModel::ChaoticFeedbackFm => self.render_chaotic_feedback_fm(sync, buffer, size),
            DigitalModel::Plucked => self.render_plucked(sync, buffer, size),
            DigitalModel::Bowed => self.render_bowed(sync, buffer, size),
            DigitalModel::Blown => self.render_blown(sync, buffer, size),
            DigitalModel::Fluted => self.render_fluted(sync, buffer, size),
            DigitalModel::StruckBell => self.render_struck_bell(sync, buffer, size),
            DigitalModel::StruckDrum => self.render_struck_drum(sync, buffer, size),
            DigitalModel::Kick => self.render_kick(sync, buffer, size),
            DigitalModel::Cymbal => self.render_cymbal(sync, buffer, size),
            DigitalModel::Snare => self.render_snare(sync, buffer, size),
            DigitalModel::Wavetables => self.render_wavetables(sync, buffer, size),
            DigitalModel::WaveMap => self.render_wave_map(sync, buffer, size),
            DigitalModel::WaveLine => self.render_wave_line(sync, buffer, size),
            DigitalModel::WaveParaphonic => self.render_wave_paraphonic(sync, buffer, size),
            DigitalModel::FilteredNoise => self.render_filtered_noise(sync, buffer, size),
            DigitalModel::TwinPeaksNoise => self.render_twin_peaks_noise(sync, buffer, size),
            DigitalModel::ClockedNoise => self.render_clocked_noise(sync, buffer, size),
            DigitalModel::GranularCloud => self.render_granular_cloud(sync, buffer, size),
            DigitalModel::ParticleNoise => self.render_particle_noise(sync, buffer, size),
            DigitalModel::DigitalModulation => self.render_digital_modulation(sync, buffer, size),
            DigitalModel::QuestionMark => self.render_question_mark(sync, buffer, size),
        }
    }

    // ----------------------------------------------------------------------
    // Models
    // ----------------------------------------------------------------------

    fn render_triple_ring_mod(&mut self, sync: &[u8], buffer: &mut [i16], size: usize) {
        let mut phase = self.phase.wrapping_add(1 << 30);
        let increment = self.phase_increment;
        let mut modulator_phase = self.state.formant_phase[0];
        let mut modulator_phase_2 = self.state.formant_phase[1];
        let modulator_phase_increment = compute_phase_increment_digital(
            self.pitch as i32 + ((self.parameter[0] as i32 - 16384) >> 2),
        );
        let modulator_phase_increment_2 = compute_phase_increment_digital(
            self.pitch as i32 + ((self.parameter[1] as i32 - 16384) >> 2),
        );

        for i in 0..size {
            phase = phase.wrapping_add(increment);
            if sync[i] != 0 {
                phase = 0;
                modulator_phase = 0;
                modulator_phase_2 = 0;
            }
            modulator_phase = modulator_phase.wrapping_add(modulator_phase_increment);
            modulator_phase_2 = modulator_phase_2.wrapping_add(modulator_phase_increment_2);
            let mut result = interpolate_824_i16(&WAV_SINE, phase);
            result = ((result as i32 * interpolate_824_i16(&WAV_SINE, modulator_phase) as i32)
                >> 16) as i16;
            result = ((result as i32 * interpolate_824_i16(&WAV_SINE, modulator_phase_2) as i32)
                >> 16) as i16;
            result = interpolate_88_i16(&WS_MODERATE_OVERDRIVE, (result as i32 + 32768) as u16);
            buffer[i] = result;
        }
        self.phase = phase.wrapping_sub(1 << 30);
        self.state.formant_phase[0] = modulator_phase;
        self.state.formant_phase[1] = modulator_phase_2;
    }

    fn render_saw_swarm(&mut self, sync: &[u8], buffer: &mut [i16], size: usize) {
        let mut detune = self.parameter[0] as i32 + 1024;
        detune = (detune * detune) >> 9;
        let mut increments = [0u32; 7];
        for (i, inc) in increments.iter_mut().enumerate() {
            let saw_detune = detune.wrapping_mul(i as i32 - 3);
            let detune_integral = saw_detune >> 16;
            let detune_fractional = saw_detune & 0xffff;
            let increment_a =
                compute_phase_increment_digital(self.pitch as i32 + detune_integral) as i32;
            let increment_b =
                compute_phase_increment_digital(self.pitch as i32 + detune_integral + 1) as i32;
            *inc = (increment_a
                + (((increment_b - increment_a).wrapping_mul(detune_fractional)) >> 16))
                as u32;
        }
        if self.strike {
            for p in self.state.saw_phase.iter_mut() {
                *p = Random::get_word();
            }
            self.strike = false;
        }
        let mut hp_cutoff = self.pitch as i32;
        if self.parameter[1] < 10922 {
            hp_cutoff += ((self.parameter[1] as i32 - 10922) * 24) >> 5;
        } else {
            hp_cutoff += ((self.parameter[1] as i32 - 10922) * 12) >> 5;
        }
        hp_cutoff = hp_cutoff.clamp(0, 32767);

        let f = interpolate_824_u16(&LUT_SVF_CUTOFF, (hp_cutoff << 17) as u32) as i32;
        let damp = LUT_SVF_DAMP[0] as i32;
        let mut bp = self.state.saw_bp;
        let mut lp = self.state.saw_lp;

        for i in 0..size {
            if sync[i] != 0 {
                for p in self.state.saw_phase.iter_mut() {
                    *p = 0;
                }
            }
            self.phase = self.phase.wrapping_add(increments[0]);
            for k in 0..6 {
                self.state.saw_phase[k] = self.state.saw_phase[k].wrapping_add(increments[k + 1]);
            }

            let mut sample = -28672i32;
            sample += (self.phase >> 19) as i32;
            for k in 0..6 {
                sample += (self.state.saw_phase[k] >> 19) as i32;
            }
            sample = interpolate_88_i16(&WS_MODERATE_OVERDRIVE, (sample + 32768) as u16) as i32;

            let notch = sample - (bp * damp >> 15);
            lp += f * bp >> 15;
            lp = clip16_sym(lp);
            let hp = notch - lp;
            bp += f * hp >> 15;

            buffer[i] = clip16_sym(hp) as i16;
        }
        self.state.saw_lp = lp;
        self.state.saw_bp = bp;
    }

    fn render_comb(&mut self, _sync: &[u8], buffer: &mut [i16], size: usize) {
        let pitch = self.pitch as i32 + ((self.parameter[0] as i32 - 16384) >> 1);
        let mut filtered_pitch = self.state.ffm_previous_sample as i32;
        filtered_pitch = (15 * filtered_pitch + pitch) >> 4;
        self.state.ffm_previous_sample = filtered_pitch as i16;

        let mut delay = compute_delay(self.state.ffm_previous_sample as i32);
        if delay > (COMB_DELAY_LENGTH as u32) << 16 {
            delay = (COMB_DELAY_LENGTH as u32) << 16;
        }
        let delay_integral = (delay >> 16) as usize;
        let delay_fractional = (delay & 0xffff) as i32;

        let mut resonance = ((self.parameter[1] as i32) << 1) - 32768;
        resonance = interpolate_88_i16(&WS_MODERATE_OVERDRIVE, (resonance + 32768) as u16) as i32;

        let mut delay_ptr = (self.phase as usize) % COMB_DELAY_LENGTH;
        for i in 0..size {
            let input = buffer[i] as i32;
            let offset = delay_ptr + 2 * COMB_DELAY_LENGTH - delay_integral;
            let a = self.comb[offset % COMB_DELAY_LENGTH] as i32;
            let b = self.comb[(offset - 1) % COMB_DELAY_LENGTH] as i32;
            let delayed_sample = a + (((b - a) * (delay_fractional >> 1)) >> 15);
            let feedback = clip16_sym((delayed_sample * resonance >> 15) + (input >> 1));
            self.comb[delay_ptr] = feedback as i16;
            let out = clip16_sym((input + (delayed_sample << 1)) >> 1);
            buffer[i] = out as i16;
            delay_ptr = (delay_ptr + 1) % COMB_DELAY_LENGTH;
        }
        self.phase = delay_ptr as u32;
    }

    fn render_toy(&mut self, sync: &[u8], buffer: &mut [i16], size: usize) {
        // 4x oversampling.
        self.phase_increment >>= 2;
        let phase_increment = self.phase_increment;
        let mut phase = self.phase;

        let mut decimation_counter = self.state.toy_decimation_counter;
        let decimation_count = 512u16.wrapping_sub((self.parameter[0] as u16) >> 6);
        let mut held_sample = self.state.toy_held_sample;

        for i in 0..size {
            let mut filtered_sample: u32 = 0;
            if sync[i] != 0 {
                phase = 0;
            }
            for tap in 0..4 {
                phase = phase.wrapping_add(phase_increment);
                if decimation_counter >= decimation_count {
                    let x = (self.parameter[1] >> 8) as u8;
                    held_sample = ((((phase >> 24) as u8) ^ (x << 1)) & (!x)).wrapping_add(x >> 1);
                    decimation_counter = 0;
                }
                filtered_sample =
                    filtered_sample.wrapping_add(FIR4_COEFFICIENTS[tap] * held_sample as u32);
                decimation_counter = decimation_counter.wrapping_add(1);
            }
            buffer[i] = ((filtered_sample >> 8).wrapping_sub(FIR4_DC_OFFSET)) as i16;
        }
        self.state.toy_held_sample = held_sample;
        self.state.toy_decimation_counter = decimation_counter;
        self.phase = phase;
    }

    fn render_digital_filter(&mut self, sync: &[u8], buffer: &mut [i16], size: usize) {
        const PHASE_RESET: [u32; 4] = [0, 0x8000_0000, 0x4000_0000, 0x8000_0000];

        // C: `int16_t shifted_pitch = pitch_ + ((parameter_[0] - 2048) >> 1);`
        // -- the sum overflows i16 for a high note + high timbre and wraps
        // negative *before* the upper clamp.
        let mut shifted_pitch =
            (self.pitch as i32 + ((self.parameter[0] as i32 - 2048) >> 1)) as i16 as i32;
        if shifted_pitch > 16383 {
            shifted_pitch = 16383;
        }
        let mut modulator_phase = self.state.modulator_phase;
        let mut square_modulator_phase = self.state.square_modulator_phase;
        let mut square_integrator = self.state.integrator;

        let filter_type = self.shape as u8 - DigitalModel::DigitalFilterLp as u8;

        let mut modulator_phase_increment = self.state.modulator_phase_increment;
        let target_increment = compute_phase_increment_digital(shifted_pitch);
        let modulator_phase_increment_increment = if modulator_phase_increment < target_increment {
            (target_increment - modulator_phase_increment) / size as u32
        } else {
            !((modulator_phase_increment - target_increment) / size as u32)
        };

        for i in 0..size {
            self.phase = self.phase.wrapping_add(self.phase_increment);
            modulator_phase_increment =
                modulator_phase_increment.wrapping_add(modulator_phase_increment_increment);
            modulator_phase = modulator_phase.wrapping_add(modulator_phase_increment);
            let integrator_gain = (modulator_phase_increment >> 14) as u16;

            if sync[i] != 0 {
                self.state.polarity = true;
                self.phase = 0;
                modulator_phase = 0;
                square_modulator_phase = 0;
                square_integrator = 0;
            }

            square_modulator_phase = square_modulator_phase.wrapping_add(modulator_phase_increment);
            if self.phase < self.phase_increment {
                modulator_phase = PHASE_RESET[filter_type as usize];
            }
            if self.phase.wrapping_shl(1) < self.phase_increment.wrapping_shl(1) {
                self.state.polarity = !self.state.polarity;
                square_modulator_phase = PHASE_RESET[((filter_type & 1) + 2) as usize];
            }

            let carrier = interpolate_824_i16(&WAV_SINE, modulator_phase) as i32;
            let square_carrier = interpolate_824_i16(&WAV_SINE, square_modulator_phase) as i32;

            let saw = (!(self.phase >> 16)) as u16;
            let double_saw = (!(self.phase >> 15)) as u16;
            let triangle = ((self.phase >> 15) as u16)
                ^ if self.phase & 0x8000_0000 != 0 {
                    0xffff
                } else {
                    0x0000
                };
            let window = if self.parameter[1] < 16384 {
                saw
            } else {
                triangle
            };

            let mut pulse = (square_carrier * double_saw as i32) >> 16;
            if self.state.polarity {
                pulse = -pulse;
            }
            square_integrator += (pulse * integrator_gain as i32) >> 16;
            square_integrator = clip16_sym(square_integrator);

            let saw_tri_signal: i16;
            let square_signal: i16;
            if filter_type & 2 != 0 {
                saw_tri_signal = ((carrier * window as i32) >> 16) as i16;
                square_signal = pulse as i16;
            } else {
                saw_tri_signal = ((window as i32 * (carrier + 32768) >> 16) - 32768) as i16;
                square_signal = if filter_type == 1 {
                    ((pulse + square_integrator) >> 1) as i16
                } else {
                    square_integrator as i16
                };
            }
            let balance = (if self.parameter[1] < 16384 {
                self.parameter[1] as u16
            } else {
                !(self.parameter[1] as u16)
            }) << 2;
            buffer[i] = mix_i16(saw_tri_signal, square_signal, balance);
        }
        self.state.modulator_phase = modulator_phase;
        self.state.square_modulator_phase = square_modulator_phase;
        self.state.integrator = square_integrator;
        self.state.modulator_phase_increment = modulator_phase_increment;
    }

    fn render_vosim(&mut self, sync: &[u8], buffer: &mut [i16], size: usize) {
        for i in 0..2 {
            self.state.formant_increment[i] =
                compute_phase_increment_digital((self.parameter[i] >> 1) as i32);
        }
        for i in 0..size {
            self.phase = self.phase.wrapping_add(self.phase_increment);
            if sync[i] != 0 {
                self.phase = 0;
            }
            let mut sample = 16384i32 + 8192;
            self.state.formant_phase[0] =
                self.state.formant_phase[0].wrapping_add(self.state.formant_increment[0]);
            sample += (interpolate_824_i16(&WAV_SINE, self.state.formant_phase[0]) as i32) >> 1;
            self.state.formant_phase[1] =
                self.state.formant_phase[1].wrapping_add(self.state.formant_increment[1]);
            sample += (interpolate_824_i16(&WAV_SINE, self.state.formant_phase[1]) as i32) >> 2;
            sample = sample * ((interpolate_824_u16(&LUT_BELL, self.phase) as i32) >> 1) >> 15;
            if self.phase < self.phase_increment {
                self.state.formant_phase[0] = 0;
                self.state.formant_phase[1] = 0;
                sample = 0;
            }
            sample -= 16384 + 8192;
            buffer[i] = sample as i16;
        }
    }

    fn render_vowel(&mut self, _sync: &[u8], buffer: &mut [i16], size: usize) {
        const VOWELS_DATA: [PhonemeDefinition; 9] = [
            PhonemeDefinition {
                f: [27, 40, 89],
                a: [15, 13, 1],
            },
            PhonemeDefinition {
                f: [18, 51, 62],
                a: [13, 12, 6],
            },
            PhonemeDefinition {
                f: [15, 69, 93],
                a: [14, 12, 7],
            },
            PhonemeDefinition {
                f: [10, 84, 110],
                a: [13, 10, 8],
            },
            PhonemeDefinition {
                f: [23, 44, 87],
                a: [15, 12, 1],
            },
            PhonemeDefinition {
                f: [13, 29, 80],
                a: [13, 8, 0],
            },
            PhonemeDefinition {
                f: [6, 46, 81],
                a: [12, 3, 0],
            },
            PhonemeDefinition {
                f: [9, 51, 95],
                a: [15, 3, 0],
            },
            PhonemeDefinition {
                f: [6, 73, 99],
                a: [7, 3, 14],
            },
        ];
        const CONSONANT_DATA: [PhonemeDefinition; 8] = [
            PhonemeDefinition {
                f: [6, 54, 121],
                a: [9, 9, 0],
            },
            PhonemeDefinition {
                f: [18, 50, 51],
                a: [12, 10, 5],
            },
            PhonemeDefinition {
                f: [11, 24, 70],
                a: [13, 8, 0],
            },
            PhonemeDefinition {
                f: [15, 69, 74],
                a: [14, 12, 7],
            },
            PhonemeDefinition {
                f: [16, 37, 111],
                a: [14, 8, 1],
            },
            PhonemeDefinition {
                f: [18, 51, 62],
                a: [14, 12, 6],
            },
            PhonemeDefinition {
                f: [6, 26, 81],
                a: [5, 5, 5],
            },
            PhonemeDefinition {
                f: [6, 73, 99],
                a: [7, 10, 14],
            },
        ];

        let vowel_index = (self.parameter[0] >> 12) as usize;
        let balance = (self.parameter[0] & 0x0fff) as u32;
        let formant_shift = (200 + (self.parameter[1] >> 6)) as u32;
        if self.strike {
            self.strike = false;
            self.state.consonant_frames = 160;
            let index = ((Random::get_sample() as i32 + 1) & 7) as usize;
            for i in 0..3 {
                self.state.formant_increment[i] = (CONSONANT_DATA[index].f[i] as u32)
                    .wrapping_mul(0x1000)
                    .wrapping_mul(formant_shift);
                self.state.formant_amplitude[i] = CONSONANT_DATA[index].a[i] as u32;
            }
            self.state.vow_noise = if index >= 6 { 4095 } else { 0 };
        }

        if self.state.consonant_frames != 0 {
            self.state.consonant_frames -= 1;
        } else {
            for i in 0..3 {
                self.state.formant_increment[i] = ((VOWELS_DATA[vowel_index].f[i] as u32)
                    .wrapping_mul(0x1000 - balance)
                    .wrapping_add(
                        (VOWELS_DATA[vowel_index + 1].f[i] as u32).wrapping_mul(balance),
                    ))
                .wrapping_mul(formant_shift);
                self.state.formant_amplitude[i] = ((VOWELS_DATA[vowel_index].a[i] as u32
                    * (0x1000 - balance))
                    .wrapping_add(VOWELS_DATA[vowel_index + 1].a[i] as u32 * balance))
                    >> 12;
            }
            self.state.vow_noise = 0;
        }
        let noise = self.state.vow_noise as i32;

        for i in 0..size {
            self.phase = self.phase.wrapping_add(self.phase_increment);
            let mut sample: i16 = 0;
            self.state.formant_phase[0] =
                self.state.formant_phase[0].wrapping_add(self.state.formant_increment[0]);
            let mut phaselet = ((self.state.formant_phase[0] >> 24) & 0xf0) as usize;
            sample = sample.wrapping_add(
                WAV_FORMANT_SINE[phaselet | self.state.formant_amplitude[0] as usize],
            );
            self.state.formant_phase[1] =
                self.state.formant_phase[1].wrapping_add(self.state.formant_increment[1]);
            phaselet = ((self.state.formant_phase[1] >> 24) & 0xf0) as usize;
            sample = sample.wrapping_add(
                WAV_FORMANT_SINE[phaselet | self.state.formant_amplitude[1] as usize],
            );
            self.state.formant_phase[2] =
                self.state.formant_phase[2].wrapping_add(self.state.formant_increment[2]);
            phaselet = ((self.state.formant_phase[2] >> 24) & 0xf0) as usize;
            sample = sample.wrapping_add(
                WAV_FORMANT_SQUARE[phaselet | self.state.formant_amplitude[2] as usize],
            );

            sample = sample.wrapping_mul(255 - (self.phase >> 24) as i16);
            let phase_noise = Random::get_sample() as i32 * noise;
            if self.phase.wrapping_add(phase_noise as u32) < self.phase_increment {
                self.state.formant_phase[0] = 0;
                self.state.formant_phase[1] = 0;
                self.state.formant_phase[2] = 0;
                sample = 0;
            }
            sample = interpolate_88_i16(&WS_MODERATE_OVERDRIVE, (sample as i32 + 32768) as u16);
            buffer[i] = sample;
        }
    }

    fn render_vowel_fof(&mut self, _sync: &[u8], buffer: &mut [i16], size: usize) {
        let mut amplitudes = [0i16; NUM_FORMANTS];
        let mut svf_lp = [0i32; NUM_FORMANTS];
        let mut svf_bp = [0i32; NUM_FORMANTS];
        let mut svf_f = [0i32; NUM_FORMANTS];

        for i in 0..NUM_FORMANTS {
            let frequency = interpolate_formant_parameter(
                &FORMANT_F_DATA,
                self.parameter[1],
                self.parameter[0],
                i,
            ) as i32
                + (12 << 7);
            svf_f[i] = interpolate_824_u16(&LUT_SVF_CUTOFF, (frequency << 17) as u32) as i32;
            amplitudes[i] = interpolate_formant_parameter(
                &FORMANT_A_DATA,
                self.parameter[1],
                self.parameter[0],
                i,
            );
            if self.init {
                svf_lp[i] = 0;
                svf_bp[i] = 0;
            } else {
                svf_lp[i] = self.state.fof_svf_lp[i];
                svf_bp[i] = self.state.fof_svf_bp[i];
            }
        }

        if self.init {
            self.init = false;
        }

        let mut phase = self.phase;
        let mut previous_sample = self.state.fof_previous_sample as i32;
        let mut next_saw_sample = self.state.fof_next_saw_sample;
        let increment = self.phase_increment << 1;
        let mut n = size;
        let mut out_idx = 0usize;
        while n != 0 {
            let mut this_saw_sample = next_saw_sample;
            next_saw_sample = 0;
            phase = phase.wrapping_add(increment);
            if phase < increment {
                let mut t = safe_div_u32(phase, increment >> 16);
                if t > 65535 {
                    t = 65535;
                }
                this_saw_sample -= (t.wrapping_mul(t) >> 18) as i32;
                t = 65535 - t;
                next_saw_sample -= -((t.wrapping_mul(t) >> 18) as i32);
            }
            next_saw_sample += (phase >> 17) as i32;
            let input = this_saw_sample;
            let mut out = 0i32;
            for i in 0..5 {
                let notch = input - (svf_bp[i] >> 6);
                svf_lp[i] += svf_f[i] * svf_bp[i] >> 15;
                svf_lp[i] = clip16_sym(svf_lp[i]);
                let hp = notch - svf_lp[i];
                svf_bp[i] += svf_f[i] * hp >> 15;
                svf_bp[i] = clip16_sym(svf_bp[i]);
                out += svf_bp[i] * amplitudes[0] as i32 >> 17;
            }
            out = clip16_sym(out);
            buffer[out_idx] = ((out + previous_sample) >> 1) as i16;
            buffer[out_idx + 1] = out as i16;
            out_idx += 2;
            previous_sample = out;
            n -= 2;
        }
        self.phase = phase;
        self.state.fof_next_saw_sample = next_saw_sample;
        self.state.fof_previous_sample = previous_sample as i16;
        for i in 0..NUM_FORMANTS {
            self.state.fof_svf_lp[i] = svf_lp[i];
            self.state.fof_svf_bp[i] = svf_bp[i];
        }
    }

    fn render_harmonics(&mut self, sync: &[u8], buffer: &mut [i16], size: usize) {
        let mut phase = self.phase;
        let mut previous_sample = self.state.add_previous_sample as i32;
        let phase_increment = self.phase_increment << 1;
        let mut target_amplitude = [0i32; NUM_ADDITIVE_HARMONICS];
        let mut amplitude = [0i32; NUM_ADDITIVE_HARMONICS];

        let peak = (NUM_ADDITIVE_HARMONICS as i32 * self.parameter[0] as i32) >> 7;
        let second_peak = (peak >> 1) + NUM_ADDITIVE_HARMONICS as i32 * 128;
        let second_peak_amount = self.parameter[1] as i32 * self.parameter[1] as i32 >> 15;

        let sqrtsqrt_width = if self.parameter[1] < 16384 {
            self.parameter[1] as i32 >> 6
        } else {
            511 - (self.parameter[1] as i32 >> 6)
        };
        let sqrt_width = sqrtsqrt_width * sqrtsqrt_width >> 10;
        let width = sqrt_width * sqrt_width + 4;
        let mut total = 0i32;
        for i in 0..NUM_ADDITIVE_HARMONICS {
            let x = (i as i32) << 8;
            let mut d = x - peak;
            let mut g = 32768 * 128 / (128 + d * d / width);
            d = x - second_peak;
            g += second_peak_amount * 128 / (128 + d * d / width);
            total += g;
            target_amplitude[i] = g;
        }

        let attenuation = 2147483647 / total;
        for i in 0..NUM_ADDITIVE_HARMONICS {
            if (phase_increment >> 16).wrapping_mul(i as u32 + 1) > 0x4000 {
                target_amplitude[i] = 0;
            } else {
                target_amplitude[i] = target_amplitude[i].wrapping_mul(attenuation) >> 16;
            }
            amplitude[i] = self.state.hrm_amplitude[i];
        }

        let mut n = size;
        let mut si = 0usize;
        let mut oi = 0usize;
        while n != 0 {
            phase = phase.wrapping_add(phase_increment);
            let s0 = sync[si] != 0;
            si += 1;
            let s1 = sync[si] != 0;
            si += 1;
            if s0 || s1 {
                phase = 0;
            }
            let mut out = 0i32;
            for i in 0..NUM_ADDITIVE_HARMONICS {
                out += interpolate_824_i16(&WAV_SINE, phase.wrapping_mul(i as u32 + 1)) as i32
                    * amplitude[i]
                    >> 15;
                amplitude[i] += (target_amplitude[i] - amplitude[i]) >> 8;
            }
            out = clip16_sym(out);
            buffer[oi] = ((out + previous_sample) >> 1) as i16;
            buffer[oi + 1] = out as i16;
            oi += 2;
            previous_sample = out;
            n -= 2;
        }
        self.state.add_previous_sample = previous_sample as i16;
        self.phase = phase;
        for i in 0..NUM_ADDITIVE_HARMONICS {
            self.state.hrm_amplitude[i] = amplitude[i];
        }
    }

    fn render_fm(&mut self, sync: &[u8], buffer: &mut [i16], size: usize) {
        let mut modulator_phase = self.state.modulator_phase;
        let modulator_phase_increment = compute_phase_increment_digital(
            (12 << 7) + self.pitch as i32 + ((self.parameter[1] as i32 - 16384) >> 1),
        ) >> 1;

        let mut p_ramp = ParamRamp::new(
            self.previous_parameter[0] as i32,
            self.parameter[0] as i32,
            size,
        );
        for i in 0..size {
            let parameter_0 = p_ramp.next();
            self.phase = self.phase.wrapping_add(self.phase_increment);
            if sync[i] != 0 {
                self.phase = 0;
                modulator_phase = 0;
            }
            modulator_phase = modulator_phase.wrapping_add(modulator_phase_increment);
            let pm = ((interpolate_824_i16(&WAV_SINE, modulator_phase) as i32)
                .wrapping_mul(parameter_0) as u32)
                << 2;
            buffer[i] = interpolate_824_i16(&WAV_SINE, self.phase.wrapping_add(pm));
        }
        self.previous_parameter[0] = self.parameter[0];
        self.state.modulator_phase = modulator_phase;
    }

    fn render_feedback_fm(&mut self, sync: &[u8], buffer: &mut [i16], size: usize) {
        let mut previous_sample = self.state.ffm_previous_sample;
        let mut modulator_phase = self.state.ffm_modulator_phase;

        let mut attenuation =
            self.pitch as i32 - (72 << 7) + ((self.parameter[1] as i32 - 16384) >> 1);
        attenuation = 32767 - attenuation * 4;
        attenuation = attenuation.clamp(0, 32767);

        let modulator_phase_increment = compute_phase_increment_digital(
            (12 << 7) + self.pitch as i32 + ((self.parameter[1] as i32 - 16384) >> 1),
        ) >> 1;

        let mut p_ramp = ParamRamp::new(
            self.previous_parameter[0] as i32,
            self.parameter[0] as i32,
            size,
        );
        for i in 0..size {
            let parameter_0 = p_ramp.next();
            self.phase = self.phase.wrapping_add(self.phase_increment);
            if sync[i] != 0 {
                self.phase = 0;
                modulator_phase = 0;
            }
            modulator_phase = modulator_phase.wrapping_add(modulator_phase_increment);

            let p = parameter_0 * attenuation >> 15;
            let mut pm = (previous_sample as i32) << 14;
            pm = (interpolate_824_i16(&WAV_SINE, modulator_phase.wrapping_add(pm as u32)) as i32)
                .wrapping_mul(p)
                << 1;
            previous_sample = interpolate_824_i16(&WAV_SINE, self.phase.wrapping_add(pm as u32));
            buffer[i] = previous_sample;
        }
        self.previous_parameter[0] = self.parameter[0];
        self.state.ffm_previous_sample = previous_sample;
        self.state.ffm_modulator_phase = modulator_phase;
    }

    fn render_chaotic_feedback_fm(&mut self, sync: &[u8], buffer: &mut [i16], size: usize) {
        let modulator_phase_increment = compute_phase_increment_digital(
            (12 << 7) + self.pitch as i32 + ((self.parameter[1] as i32 - 16384) >> 1),
        ) >> 1;
        let mut previous_sample = self.state.ffm_previous_sample;
        let mut modulator_phase = self.state.ffm_modulator_phase;

        let mut p_ramp = ParamRamp::new(
            self.previous_parameter[0] as i32,
            self.parameter[0] as i32,
            size,
        );
        for i in 0..size {
            let parameter_0 = p_ramp.next();
            self.phase = self.phase.wrapping_add(self.phase_increment);
            if sync[i] != 0 {
                self.phase = 0;
                modulator_phase = 0;
            }
            let pm = ((interpolate_824_i16(&WAV_SINE, modulator_phase) as i32)
                .wrapping_mul(parameter_0) as u32)
                << 1;
            previous_sample = interpolate_824_i16(&WAV_SINE, self.phase.wrapping_add(pm));
            buffer[i] = previous_sample;
            modulator_phase = modulator_phase.wrapping_add(
                (modulator_phase_increment >> 8)
                    .wrapping_mul((129 + (previous_sample >> 9)) as u32),
            );
        }
        self.previous_parameter[0] = self.parameter[0];
        self.state.ffm_previous_sample = previous_sample;
        self.state.ffm_modulator_phase = modulator_phase;
    }

    fn render_struck_bell(&mut self, _sync: &[u8], buffer: &mut [i16], size: usize) {
        const BELL_PARTIALS: [i16; NUM_BELL_PARTIALS] = [
            -1284, -1283, -184, -183, 385, 1175, 1536, 2233, 2434, 2934, 3110,
        ];
        const BELL_PARTIAL_AMPLITUDES: [i32; NUM_BELL_PARTIALS] = [
            8192, 5488, 8192, 14745, 21872, 13680, 11960, 10895, 10895, 6144, 10895,
        ];
        const BELL_PARTIAL_DECAY_LONG: [u16; NUM_BELL_PARTIALS] = [
            65533, 65533, 65533, 65532, 65531, 65531, 65530, 65529, 65527, 65523, 65519,
        ];
        const BELL_PARTIAL_DECAY_SHORT: [u16; NUM_BELL_PARTIALS] = [
            65308, 65283, 65186, 65123, 64839, 64889, 64632, 64409, 64038, 63302, 62575,
        ];

        let mut first_partial = self.state.current_partial;
        let mut last_partial = (self.state.current_partial + 3).min(NUM_BELL_PARTIALS);
        self.state.current_partial = (first_partial + 3) % NUM_BELL_PARTIALS;

        if self.strike {
            for i in 0..NUM_BELL_PARTIALS {
                self.state.partial_amplitude[i] = BELL_PARTIAL_AMPLITUDES[i];
                self.state.partial_phase[i] = 1 << 30;
            }
            self.strike = false;
            first_partial = 0;
            last_partial = NUM_BELL_PARTIALS;
        }

        for i in first_partial..last_partial {
            let mut partial_pitch = self.pitch as i32 + BELL_PARTIALS[i] as i32;
            if i & 1 != 0 {
                partial_pitch += self.parameter[1] as i32 >> 7;
            } else {
                partial_pitch -= self.parameter[1] as i32 >> 7;
            }
            self.state.partial_phase_increment[i] =
                compute_phase_increment_digital(partial_pitch) << 1;
        }

        if self.parameter[0] < 32000 {
            for i in 0..NUM_BELL_PARTIALS {
                let decay_long = BELL_PARTIAL_DECAY_LONG[i] as i32;
                let decay_short = BELL_PARTIAL_DECAY_SHORT[i] as i32;
                let mut balance = (32767 - self.parameter[0] as i32) >> 8;
                balance = balance * balance >> 7;
                let decay = decay_long - ((decay_long - decay_short) * balance >> 7);
                self.state.partial_amplitude[i] =
                    self.state.partial_amplitude[i].wrapping_mul(decay) >> 16;
            }
        }

        let mut previous_sample = self.state.add_previous_sample as i32;
        let mut n = size;
        let mut oi = 0usize;
        while n != 0 {
            let mut out = 0i32;
            for i in 0..NUM_BELL_PARTIALS {
                self.state.partial_phase[i] =
                    self.state.partial_phase[i].wrapping_add(self.state.partial_phase_increment[i]);
                let partial = interpolate_824_i16(&WAV_SINE, self.state.partial_phase[i]) as i32;
                out += partial * self.state.partial_amplitude[i] >> 17;
            }
            out = clip16_sym(out);
            buffer[oi] = ((out + previous_sample) >> 1) as i16;
            buffer[oi + 1] = out as i16;
            oi += 2;
            n -= 2;
            previous_sample = out;
        }
        self.state.add_previous_sample = previous_sample as i16;
    }

    fn render_struck_drum(&mut self, _sync: &[u8], buffer: &mut [i16], size: usize) {
        const DRUM_PARTIALS: [i16; NUM_DRUM_PARTIALS] = [0, 0, 1041, 1747, 1846, 3072];
        const DRUM_PARTIAL_AMPLITUDE: [i32; NUM_DRUM_PARTIALS] =
            [16986, 2654, 3981, 5308, 3981, 2985];
        const DRUM_PARTIAL_DECAY_LONG: [u16; NUM_DRUM_PARTIALS] =
            [65533, 65531, 65531, 65531, 65531, 65516];
        const DRUM_PARTIAL_DECAY_SHORT: [u16; NUM_DRUM_PARTIALS] =
            [65083, 64715, 64715, 64715, 64715, 62312];

        if self.strike {
            let reset_phase = self.state.partial_amplitude[0] < 1024;
            for i in 0..NUM_DRUM_PARTIALS {
                self.state.target_partial_amplitude[i] = DRUM_PARTIAL_AMPLITUDE[i];
                if reset_phase {
                    self.state.partial_phase[i] = 1 << 30;
                }
            }
            self.strike = false;
        } else if self.parameter[0] < 32000 {
            for i in 0..NUM_DRUM_PARTIALS {
                let decay_long = DRUM_PARTIAL_DECAY_LONG[i] as i32;
                let decay_short = DRUM_PARTIAL_DECAY_SHORT[i] as i32;
                let mut balance = (32767 - self.parameter[0] as i32) >> 8;
                balance = balance * balance >> 7;
                let decay = decay_long - ((decay_long - decay_short) * balance >> 7);
                self.state.target_partial_amplitude[i] =
                    self.state.partial_amplitude[i].wrapping_mul(decay) >> 16;
            }
        }

        for i in 0..NUM_DRUM_PARTIALS {
            let partial_pitch = self.pitch as i32 + DRUM_PARTIALS[i] as i32;
            self.state.partial_phase_increment[i] =
                compute_phase_increment_digital(partial_pitch) << 1;
        }

        let mut previous_sample = self.state.add_previous_sample as i32;
        let mut cutoff = (self.pitch as i32 - 12 * 128) + (self.parameter[1] as i32 >> 2);
        cutoff = cutoff.clamp(0, 32767);
        let f = interpolate_824_u16(&LUT_SVF_CUTOFF, (cutoff << 16) as u32) as i32;
        let mut lp_state_0 = self.state.lp_noise[0];
        let mut lp_state_1 = self.state.lp_noise[1];
        let mut lp_state_2 = self.state.lp_noise[2];
        let harmonics_gain = if self.parameter[1] < 12888 {
            self.parameter[1] as i32 + 4096
        } else {
            16384
        };
        let mut noise_mode_gain = if self.parameter[1] < 16384 {
            0
        } else {
            self.parameter[1] as i32 - 16384
        };
        noise_mode_gain = noise_mode_gain * 12888 >> 14;

        let fade_increment = 65536 / size as i32;
        let mut fade = 0i32;
        let mut n = size;
        let mut oi = 0usize;
        while n != 0 {
            fade += fade_increment;
            let mut harmonics = 0i32;

            let mut noise = Random::get_sample() as i32;
            noise = noise.clamp(-16384, 16384);
            lp_state_0 += (noise - lp_state_0) * f >> 15;
            lp_state_1 += (lp_state_0 - lp_state_1) * f >> 15;
            lp_state_2 += (lp_state_1 - lp_state_2) * f >> 15;

            let mut partials = [0i32; NUM_DRUM_PARTIALS];
            for i in 0..NUM_DRUM_PARTIALS {
                self.state.partial_phase[i] =
                    self.state.partial_phase[i].wrapping_add(self.state.partial_phase_increment[i]);
                let mut partial =
                    interpolate_824_i16(&WAV_SINE, self.state.partial_phase[i]) as i32;
                let amplitude = self.state.partial_amplitude[i]
                    + (((self.state.target_partial_amplitude[i]
                        - self.state.partial_amplitude[i])
                        * fade)
                        >> 15);
                partial = partial * amplitude >> 16;
                harmonics += partial;
                partials[i] = partial;
            }
            let mut sample = partials[0];
            let noise_mode_1 = partials[1] * lp_state_2 >> 8;
            let noise_mode_2 = partials[3] * lp_state_2 >> 9;
            sample += noise_mode_1 * (12288 - noise_mode_gain) >> 14;
            sample += noise_mode_2 * noise_mode_gain >> 14;
            sample += harmonics * harmonics_gain >> 14;
            sample = clip16_sym(sample);
            buffer[oi] = ((sample + previous_sample) >> 1) as i16;
            buffer[oi + 1] = sample as i16;
            oi += 2;
            n -= 2;
            previous_sample = sample;
        }
        self.state.add_previous_sample = previous_sample as i16;
        self.state.lp_noise[0] = lp_state_0;
        self.state.lp_noise[1] = lp_state_1;
        self.state.lp_noise[2] = lp_state_2;
        for i in 0..NUM_BELL_PARTIALS {
            self.state.partial_amplitude[i] = self.state.target_partial_amplitude[i];
        }
    }

    fn render_plucked(&mut self, _sync: &[u8], buffer: &mut [i16], size: usize) {
        self.phase_increment <<= 1;
        if self.strike {
            self.active_voice = self.active_voice.wrapping_add(1);
            if self.active_voice as usize >= NUM_PLUCK_VOICES {
                self.active_voice = 0;
            }
            let av = self.active_voice as usize;
            let mut increment = self.phase_increment as i32;
            self.state.plk[av].shift = 0;
            while increment > (2 << 22) {
                increment >>= 1;
                self.state.plk[av].shift += 1;
            }
            self.state.plk[av].size = 1024 >> self.state.plk[av].shift;
            self.state.plk[av].mask = self.state.plk[av].size - 1;
            self.state.plk[av].write_ptr = 0;
            self.state.plk[av].max_phase_increment = self.phase_increment << 1;
            self.state.plk[av].phase_increment = self.phase_increment;
            let mut width = self.parameter[1] as i32;
            width = (3 * width) >> 1;
            self.state.plk[av].initialization_ptr =
                (self.state.plk[av].size * (8192 + width) as usize) >> 16;
            self.strike = false;
        }

        let av = self.active_voice as usize;
        self.state.plk[av].phase_increment = self
            .phase_increment
            .min(self.state.plk[av].max_phase_increment);

        let update_probability = if self.parameter[0] < 16384 {
            65535u32
        } else {
            131072u32.wrapping_sub((self.parameter[0] as u32 >> 3).wrapping_mul(31))
        };
        let mut loss = 4096i16 - (self.phase_increment >> 14) as i16;
        if loss < 256 {
            loss = 256;
        }
        if self.parameter[0] < 16384 {
            loss = ((loss as i32) * (16384 - self.parameter[0] as i32) >> 14) as i16;
        } else {
            loss = 0;
        }

        let mut previous_sample = self.state.plk[0].previous_sample;

        let mut n = size;
        let mut oi = 0usize;
        while n != 0 {
            let mut sample = 0i32;
            for i in 0..NUM_PLUCK_VOICES {
                let base = i * 1025;
                if self.state.plk[i].initialization_ptr != 0 {
                    self.state.plk[i].initialization_ptr -= 1;
                    let ip = self.state.plk[i].initialization_ptr;
                    let excitation_sample =
                        (self.ks[base + ip] as i32 + 3 * Random::get_sample() as i32) >> 2;
                    self.ks[base + ip] = excitation_sample as i16;
                    sample += excitation_sample;
                } else {
                    self.state.plk[i].phase = self.state.plk[i]
                        .phase
                        .wrapping_add(self.state.plk[i].phase_increment);
                    let read_ptr = (((self.state.plk[i].phase >> (22 + self.state.plk[i].shift))
                        as usize)
                        + 2)
                        & self.state.plk[i].mask;
                    let mut write_ptr = self.state.plk[i].write_ptr;
                    while write_ptr != read_ptr {
                        let next = (write_ptr + 1) & self.state.plk[i].mask;
                        let a = self.ks[base + write_ptr] as i32;
                        let b = self.ks[base + next] as i32;
                        let probability = Random::get_word();
                        if (probability & 0xffff) <= update_probability {
                            let mut sum = a + b;
                            sum = if sum < 0 { -(-sum >> 1) } else { sum >> 1 };
                            if loss != 0 {
                                sum = sum * (32768 - loss as i32) >> 15;
                            }
                            self.ks[base + write_ptr] = sum as i16;
                        }
                        if write_ptr == 0 {
                            self.ks[base + self.state.plk[i].size] = self.ks[base];
                        }
                        write_ptr = next;
                    }
                    self.state.plk[i].write_ptr = write_ptr;
                    sample += interpolate_1022(
                        &self.ks[base..base + 1025],
                        self.state.plk[i].phase >> self.state.plk[i].shift,
                    ) as i32;
                }
            }
            sample = clip16_sym(sample);
            buffer[oi] = ((previous_sample as i32 + sample) >> 1) as i16;
            buffer[oi + 1] = sample as i16;
            oi += 2;
            previous_sample = sample as i16;
            n -= 2;
        }
        self.state.plk[0].previous_sample = previous_sample;
    }

    fn render_bowed(&mut self, _sync: &[u8], buffer: &mut [i16], size: usize) {
        const BRIDGE_LP_GAIN: i32 = 14008;
        const BRIDGE_LP_POLE1: i32 = 18022;
        const BIQUAD_GAIN: i32 = 6553;
        const BIQUAD_POLE1: i32 = 6948;
        const BIQUAD_POLE2: i32 = -2959;

        if self.strike {
            self.bowed_bridge.fill(0);
            self.bowed_neck.fill(0);
            self.state = State::zeroed();
            self.strike = false;
        }
        let parameter_0 = 172 - (self.parameter[0] >> 8);
        let parameter_1 = 6 + (self.parameter[1] >> 9);

        let mut delay_ptr = self.state.phy_delay_ptr;
        let mut excitation_ptr = self.state.phy_excitation_ptr;
        let mut lp_state = self.state.phy_lp_state;
        let mut biquad_y0 = self.state.phy_filter_state[0];
        let mut biquad_y1 = self.state.phy_filter_state[1];

        let mut delay = (self.delay >> 1).wrapping_sub(2 << 16);
        let mut bridge_delay = (delay >> 8).wrapping_mul(parameter_1 as u32);
        while delay.wrapping_sub(bridge_delay) > ((WG_NECK_LENGTH as u32 - 1) << 16)
            || bridge_delay > ((WG_BRIDGE_LENGTH as u32 - 1) << 16)
        {
            delay >>= 1;
            bridge_delay >>= 1;
        }
        let bridge_delay_integral = (bridge_delay >> 16) as u16;
        let bridge_delay_fractional = (bridge_delay & 0xffff) as u16;
        let neck_delay = delay.wrapping_sub(bridge_delay);
        let neck_delay_integral = (neck_delay >> 16) as u16;
        let neck_delay_fractional = (neck_delay & 0xffff) as u16;
        let mut previous_sample = self.state.phy_previous_sample as i32;

        let mut n = size;
        let mut oi = 0usize;
        while n != 0 {
            self.phase = self.phase.wrapping_add(self.phase_increment);

            let bridge_delay_ptr = delay_ptr
                .wrapping_add(2 * WG_BRIDGE_LENGTH as u16)
                .wrapping_sub(bridge_delay_integral);
            let neck_delay_ptr = delay_ptr
                .wrapping_add(2 * WG_NECK_LENGTH as u16)
                .wrapping_sub(neck_delay_integral);
            let bridge_dl_a =
                self.bowed_bridge[(bridge_delay_ptr as usize) % WG_BRIDGE_LENGTH] as i16;
            let bridge_dl_b = self.bowed_bridge
                [(bridge_delay_ptr.wrapping_sub(1) as usize) % WG_BRIDGE_LENGTH]
                as i16;
            let nut_dl_a = self.bowed_neck[(neck_delay_ptr as usize) % WG_NECK_LENGTH] as i16;
            let nut_dl_b =
                self.bowed_neck[(neck_delay_ptr.wrapping_sub(1) as usize) % WG_NECK_LENGTH] as i16;
            let bridge_value =
                (mix_i16(bridge_dl_a, bridge_dl_b, bridge_delay_fractional) as i32) << 8;
            let nut_value = (mix_i16(nut_dl_a, nut_dl_b, neck_delay_fractional) as i32) << 8;
            lp_state = (bridge_value.wrapping_mul(BRIDGE_LP_GAIN)
                + lp_state.wrapping_mul(BRIDGE_LP_POLE1))
                >> 15;
            let bridge_reflection = -lp_state;
            let nut_reflection = -nut_value;
            let string_velocity = bridge_reflection + nut_reflection;
            let mut bow_velocity = LUT_BOWING_ENVELOPE[(excitation_ptr >> 1) as usize] as i32;
            bow_velocity += LUT_BOWING_ENVELOPE[((excitation_ptr + 1) >> 1) as usize] as i32;
            bow_velocity >>= 1;
            let velocity_delta = bow_velocity - string_velocity;

            let mut friction = velocity_delta.wrapping_mul(parameter_0 as i32) >> 5;
            if friction < 0 {
                friction = -friction;
            }
            if friction >= (1 << 17) {
                friction = (1 << 17) - 1;
            }
            friction = LUT_BOWING_FRICTION[(friction >> 9) as usize] as i32;
            let new_velocity = friction.wrapping_mul(velocity_delta) >> 15;
            self.bowed_neck[(delay_ptr as usize) % WG_NECK_LENGTH] =
                ((bridge_reflection + new_velocity) >> 8) as i8;
            self.bowed_bridge[(delay_ptr as usize) % WG_BRIDGE_LENGTH] =
                ((nut_reflection + new_velocity) >> 8) as i8;
            delay_ptr = delay_ptr.wrapping_add(1);

            let mut temp = bridge_value.wrapping_mul(BIQUAD_GAIN) >> 15;
            temp += biquad_y0.wrapping_mul(BIQUAD_POLE1) >> 12;
            temp += biquad_y1.wrapping_mul(BIQUAD_POLE2) >> 12;
            let out = clip16_sym(temp - biquad_y1);
            biquad_y1 = biquad_y0;
            biquad_y0 = temp;

            buffer[oi] = ((out + previous_sample) >> 1) as i16;
            buffer[oi + 1] = out as i16;
            oi += 2;
            previous_sample = out;
            excitation_ptr = excitation_ptr.wrapping_add(1);
            n -= 2;
        }
        if (excitation_ptr >> 1) as usize >= LUT_BOWING_ENVELOPE_SIZE - 32 {
            excitation_ptr = ((LUT_BOWING_ENVELOPE_SIZE - 32) << 1) as u16;
        }
        self.state.phy_delay_ptr = (delay_ptr as usize % WG_NECK_LENGTH) as u16;
        self.state.phy_excitation_ptr = excitation_ptr;
        self.state.phy_lp_state = lp_state;
        self.state.phy_filter_state[0] = biquad_y0;
        self.state.phy_filter_state[1] = biquad_y1;
        self.state.phy_previous_sample = previous_sample as i16;
    }

    fn render_blown(&mut self, _sync: &[u8], buffer: &mut [i16], size: usize) {
        const BREATH_PRESSURE: u16 = 26214;
        const REFLECTION_COEFFICIENT: i32 = -3891;
        const REED_SLOPE: i32 = -1229;
        const REED_OFFSET: i32 = 22938;

        let mut delay_ptr = self.state.phy_delay_ptr;
        let mut lp_state = self.state.phy_lp_state;

        if self.strike {
            self.bore.fill(0);
            self.strike = false;
        }

        let mut delay = (self.delay >> 1).wrapping_sub(1 << 16);
        while delay > ((WG_BORE_LENGTH as u32 - 1) << 16) {
            delay >>= 1;
        }
        let bore_delay_integral = (delay >> 16) as u16;
        let bore_delay_fractional = (delay & 0xffff) as u16;
        let parameter = 28000u16.wrapping_sub((self.parameter[0] as u16) >> 1);
        let mut filter_state = self.state.phy_filter_state[0];
        let mut normalized_pitch =
            (self.pitch as i32 - 8192 + (self.parameter[1] as i32 >> 1)) >> 7;
        normalized_pitch = normalized_pitch.clamp(0, 127);
        let filter_coefficient = LUT_FLUTE_BODY_FILTER[normalized_pitch as usize] as i32;

        for i in 0..size {
            self.phase = self.phase.wrapping_add(self.phase_increment);

            let mut breath_pressure =
                (Random::get_sample() as i32).wrapping_mul(parameter as i32) >> 15;
            breath_pressure = breath_pressure.wrapping_mul(BREATH_PRESSURE as i32) >> 15;
            breath_pressure += BREATH_PRESSURE as i32;

            let bore_delay_ptr = delay_ptr
                .wrapping_add(2 * WG_BORE_LENGTH as u16)
                .wrapping_sub(bore_delay_integral);
            let dl_a = self.bore[(bore_delay_ptr as usize) % WG_BORE_LENGTH] as i16;
            let dl_b = self.bore[(bore_delay_ptr.wrapping_sub(1) as usize) % WG_BORE_LENGTH] as i16;
            let dl_value = mix_i16(dl_a, dl_b, bore_delay_fractional) as i32;

            let mut pressure_delta = (dl_value >> 1) + lp_state;
            lp_state = dl_value >> 1;

            pressure_delta = REFLECTION_COEFFICIENT.wrapping_mul(pressure_delta) >> 12;
            pressure_delta -= breath_pressure;
            let reed = clip16_sym((pressure_delta.wrapping_mul(REED_SLOPE) >> 12) + REED_OFFSET);
            let mut out = pressure_delta.wrapping_mul(reed) >> 15;
            out += breath_pressure;
            out = clip16_sym(out);
            self.bore[(delay_ptr as usize) % WG_BORE_LENGTH] = out as i16;
            delay_ptr = delay_ptr.wrapping_add(1);
            filter_state = (filter_coefficient.wrapping_mul(out)
                + (4096 - filter_coefficient).wrapping_mul(filter_state))
                >> 12;
            buffer[i] = filter_state as i16;
        }
        self.state.phy_filter_state[0] = filter_state;
        self.state.phy_delay_ptr = (delay_ptr as usize % WG_BORE_LENGTH) as u16;
        self.state.phy_lp_state = lp_state;
    }

    fn render_fluted(&mut self, _sync: &[u8], buffer: &mut [i16], size: usize) {
        const DC_BLOCKING_POLE: i32 = 4055; // 0.99 * 4096

        let mut delay_ptr = self.state.phy_delay_ptr;
        let mut excitation_ptr = self.state.phy_excitation_ptr;
        let mut lp_state = self.state.phy_lp_state;
        let mut dc_blocking_x0 = self.state.phy_filter_state[0];
        let mut dc_blocking_y0 = self.state.phy_filter_state[1];

        if self.strike {
            excitation_ptr = 0;
            self.fluted_bore.fill(0);
            self.fluted_jet.fill(0);
            lp_state = 0;
            self.strike = false;
        }

        let mut bore_delay = (self.delay << 1).wrapping_sub(2 << 16);
        let mut jet_delay = (bore_delay >> 8).wrapping_mul(48 + (self.parameter[1] as u32 >> 10));
        bore_delay = bore_delay.wrapping_sub(jet_delay);
        while bore_delay > ((WG_FBORE_LENGTH as u32 - 1) << 16)
            || jet_delay > ((WG_JET_LENGTH as u32 - 1) << 16)
        {
            bore_delay >>= 1;
            jet_delay >>= 1;
        }
        let bore_delay_integral = (bore_delay >> 16) as u16;
        let bore_delay_fractional = (bore_delay & 0xffff) as u16;
        let jet_delay_integral = jet_delay >> 16;
        let jet_delay_fractional = (jet_delay & 0xffff) as u16;

        let breath_intensity = 2100u16.wrapping_sub((self.parameter[0] as u16) >> 4);
        // C indexes `lut_flute_body_filter[pitch_ >> 7]` without clamping, which
        // reads past the 128-entry table for notes above ~127. Clamp instead.
        let filter_coefficient =
            LUT_FLUTE_BODY_FILTER[((self.pitch >> 7) as usize).min(127)] as i32;

        for i in 0..size {
            self.phase = self.phase.wrapping_add(self.phase_increment);

            let bore_delay_ptr = delay_ptr
                .wrapping_add(2 * WG_FBORE_LENGTH as u16)
                .wrapping_sub(bore_delay_integral);
            let jet_delay_ptr = delay_ptr
                .wrapping_add(2 * WG_JET_LENGTH as u16)
                .wrapping_sub(jet_delay_integral as u16);
            let bore_dl_a = self.fluted_bore[(bore_delay_ptr as usize) % WG_FBORE_LENGTH] as i16;
            let bore_dl_b = self.fluted_bore
                [(bore_delay_ptr.wrapping_sub(1) as usize) % WG_FBORE_LENGTH]
                as i16;
            let jet_dl_a = self.fluted_jet[(jet_delay_ptr as usize) % WG_JET_LENGTH] as i16;
            let jet_dl_b =
                self.fluted_jet[(jet_delay_ptr.wrapping_sub(1) as usize) % WG_JET_LENGTH] as i16;
            let bore_value = (mix_i16(bore_dl_a, bore_dl_b, bore_delay_fractional) as i32) << 9;
            let jet_value = (mix_i16(jet_dl_a, jet_dl_b, jet_delay_fractional) as i32) << 9;

            let mut breath_pressure = LUT_BLOWING_ENVELOPE[excitation_ptr as usize] as i32;
            breath_pressure <<= 1;
            let mut random_pressure =
                (Random::get_sample() as i32).wrapping_mul(breath_intensity as i32) >> 12;
            random_pressure = random_pressure.wrapping_mul(breath_pressure) >> 15;
            breath_pressure += random_pressure;

            lp_state = ((-filter_coefficient).wrapping_mul(bore_value)
                + (4096 - filter_coefficient).wrapping_mul(lp_state))
                >> 12;
            let reflection = lp_state;
            dc_blocking_y0 = DC_BLOCKING_POLE.wrapping_mul(dc_blocking_y0) >> 12;
            dc_blocking_y0 += reflection - dc_blocking_x0;
            dc_blocking_x0 = reflection;
            let reflection = dc_blocking_y0;

            let mut pressure_delta = breath_pressure - (reflection >> 1);
            self.fluted_jet[(delay_ptr as usize) % WG_JET_LENGTH] = (pressure_delta >> 9) as i8;

            pressure_delta = jet_value;
            let jet_table_index = pressure_delta.clamp(0, 65535);
            pressure_delta =
                LUT_BLOWING_JET[(jet_table_index >> 8) as usize] as i32 + (reflection >> 1);
            self.fluted_bore[(delay_ptr as usize) % WG_FBORE_LENGTH] = (pressure_delta >> 9) as i8;
            delay_ptr = delay_ptr.wrapping_add(1);

            let out = clip16_sym(bore_value >> 1);
            buffer[i] = out as i16;
            if (size - 1 - i) & 3 != 0 {
                excitation_ptr = excitation_ptr.wrapping_add(1);
            }
        }
        if excitation_ptr as usize >= LUT_BLOWING_ENVELOPE_SIZE - 32 {
            excitation_ptr = (LUT_BLOWING_ENVELOPE_SIZE - 32) as u16;
        }
        self.state.phy_delay_ptr = delay_ptr;
        self.state.phy_excitation_ptr = excitation_ptr;
        self.state.phy_lp_state = lp_state;
        self.state.phy_filter_state[0] = dc_blocking_x0;
        self.state.phy_filter_state[1] = dc_blocking_y0;
    }

    fn render_wavetables(&mut self, sync: &[u8], buffer: &mut [i16], size: usize) {
        if self.parameter[1] > self.previous_parameter[1] + 64
            || self.parameter[1] < self.previous_parameter[1] - 64
        {
            self.previous_parameter[1] = self.parameter[1];
        }

        let mut wavetable_index = (self.previous_parameter[1] as u32) * 20;
        wavetable_index >>= 15;

        let wt = &WAVETABLE_DEFINITIONS[wavetable_index as usize];
        let wave_pointer =
            (((self.parameter[0] as i32) << 1) as u32).wrapping_mul(wt.num_steps as u32);
        let mut wave: [&[u8]; 2] = [&WT_WAVES[..129], &WT_WAVES[..129]];
        for (i, w) in wave.iter_mut().enumerate() {
            let wave_index = wt.wave_index[((wave_pointer >> 16) as usize + i).min(16)] as usize;
            *w = &WT_WAVES[wave_index * 129..wave_index * 129 + 129];
        }

        let phase_increment = self.phase_increment >> 1;
        for i in 0..size {
            self.phase = self.phase.wrapping_add(phase_increment);
            if sync[i] != 0 {
                self.phase = 0;
            }
            let s0 = crossfade_u8(wave[0], wave[1], self.phase >> 1, wave_pointer as u16) >> 1;
            self.phase = self.phase.wrapping_add(phase_increment);
            let s1 = crossfade_u8(wave[0], wave[1], self.phase >> 1, wave_pointer as u16) >> 1;
            buffer[i] = s0.wrapping_add(s1);
        }
    }

    fn render_wave_map(&mut self, sync: &[u8], buffer: &mut [i16], size: usize) {
        let mut p = [0u16; 2];
        let mut wave_xfade = [0u16; 2];
        let mut wave_coordinate = [0u16; 2];
        p[0] = ((self.parameter[0] as u32 * 15) >> 4) as u16;
        p[1] = ((self.parameter[1] as u32 * 15) >> 4) as u16;
        wave_xfade[0] = p[0] << 5;
        wave_xfade[1] = p[1] << 5;
        wave_coordinate[0] = p[0] >> 11;
        wave_coordinate[1] = p[1] >> 11;

        let mut wave: [[&[u8]; 2]; 2] = [[&WT_WAVES[..129]; 2]; 2];
        for i in 0..2 {
            for j in 0..2 {
                let wave_index = ((wave_coordinate[0] as usize + i) * 16
                    + (wave_coordinate[1] as usize + j))
                    .min(WT_MAP.len() - 1);
                let w = WT_MAP[wave_index] as usize;
                wave[i][j] = &WT_WAVES[w * 129..w * 129 + 129];
            }
        }

        let phase_increment = self.phase_increment >> 1;
        for i in 0..size {
            self.phase = self.phase.wrapping_add(phase_increment);
            if sync[i] != 0 {
                self.phase = 0;
            }
            let mut sample = mix_i16(
                crossfade_u8(wave[0][0], wave[0][1], self.phase >> 1, wave_xfade[1]),
                crossfade_u8(wave[1][0], wave[1][1], self.phase >> 1, wave_xfade[1]),
                wave_xfade[0],
            ) >> 1;
            self.phase = self.phase.wrapping_add(phase_increment);
            sample = sample.wrapping_add(
                mix_i16(
                    crossfade_u8(wave[0][0], wave[0][1], self.phase >> 1, wave_xfade[1]),
                    crossfade_u8(wave[1][0], wave[1][1], self.phase >> 1, wave_xfade[1]),
                    wave_xfade[0],
                ) >> 1,
            );
            buffer[i] = sample;
        }
    }

    fn render_wave_line(&mut self, sync: &[u8], buffer: &mut [i16], size: usize) {
        // NOTE: the C indexes `wave_line[(scan >> 10) + 1]`, i.e. `wave_line[64]`
        // -- one past this 64-entry table -- whenever the timbre knob sits near
        // its maximum. That out-of-bounds read is a latent Braids bug; the byte
        // it returns depends on `.rodata` layout (the g++ reference build yields
        // 16). We clamp to the last valid entry instead, so this model deviates
        // from that build by up to ~250 LSB for one render block at max timbre.
        const WAVE_LINE: [u8; 64] = [
            187, 179, 154, 155, 135, 134, 137, 19, 24, 3, 8, 66, 79, 25, 180, 174, 64, 127, 198,
            15, 10, 7, 11, 0, 191, 192, 115, 238, 237, 236, 241, 47, 70, 76, 235, 26, 133, 208, 34,
            175, 183, 146, 147, 148, 150, 151, 152, 153, 117, 138, 32, 33, 35, 125, 199, 201, 30,
            31, 193, 27, 29, 21, 18, 182,
        ];

        self.smoothed_parameter =
            (3 * self.smoothed_parameter + ((self.parameter[0] as i32) << 1)) >> 2;
        let scan = self.smoothed_parameter as u16;
        let w0 = WAVE_LINE[(self.previous_parameter[0] as usize >> 9).min(63)] as usize;
        let w1 = WAVE_LINE[((scan >> 10) as usize).min(63)] as usize;
        let w2 = WAVE_LINE[(((scan >> 10) + 1) as usize).min(63)] as usize;
        let wave_0 = &WT_WAVES[w0 * 129..w0 * 129 + 129];
        let wave_1 = &WT_WAVES[w1 * 129..w1 * 129 + 129];
        let wave_2 = &WT_WAVES[w2 * 129..w2 * 129 + 129];

        let smooth_xfade = scan << 6;
        let mut rough_xfade = 0u16;
        let rough_xfade_increment = (32768 / size) as u16;
        let balance = (self.parameter[1] as u32) << 3;

        let mut phase = self.phase;
        let phase_increment = self.phase_increment >> 1;

        for i in 0..size {
            if sync[i] != 0 {
                phase = 0;
            }
            let mut sample = 0i32;
            let (rough, smooth);
            if self.parameter[1] < 8192 {
                rough = crossfade_u8(wave_0, wave_1, (phase >> 1) & 0xfe00_0000, rough_xfade);
                smooth = crossfade_u8(wave_0, wave_1, phase >> 1, rough_xfade);
            } else if self.parameter[1] < 16384 {
                rough = crossfade_u8(wave_0, wave_1, phase >> 1, rough_xfade);
                smooth = crossfade_u8(wave_1, wave_2, phase >> 1, smooth_xfade);
            } else if self.parameter[1] < 24576 {
                smooth = crossfade_u8(wave_1, wave_2, phase >> 1, smooth_xfade);
                rough = crossfade_u8(wave_1, wave_2, (phase >> 1) & 0xfe00_0000, smooth_xfade);
            } else {
                smooth = crossfade_u8(wave_1, wave_2, (phase >> 1) & 0xfe00_0000, smooth_xfade);
                rough = crossfade_u8(wave_1, wave_2, (phase >> 1) & 0xf800_0000, smooth_xfade);
            }
            let (a, b) = if self.parameter[1] < 16384 {
                (rough, smooth)
            } else {
                (smooth, rough)
            };
            sample += mix_i16(a, b, balance as u16) as i32;
            phase = phase.wrapping_add(phase_increment);
            rough_xfade = rough_xfade.wrapping_add(rough_xfade_increment);

            // Second naive-oversampled tap.
            let (rough2, smooth2);
            if self.parameter[1] < 8192 {
                rough2 = crossfade_u8(wave_0, wave_1, (phase >> 1) & 0xfe00_0000, rough_xfade);
                smooth2 = crossfade_u8(wave_0, wave_1, phase >> 1, rough_xfade);
            } else if self.parameter[1] < 16384 {
                rough2 = crossfade_u8(wave_0, wave_1, phase >> 1, rough_xfade);
                smooth2 = crossfade_u8(wave_1, wave_2, phase >> 1, smooth_xfade);
            } else if self.parameter[1] < 24576 {
                smooth2 = crossfade_u8(wave_1, wave_2, phase >> 1, smooth_xfade);
                rough2 = crossfade_u8(wave_1, wave_2, (phase >> 1) & 0xfe00_0000, smooth_xfade);
            } else {
                smooth2 = crossfade_u8(wave_1, wave_2, (phase >> 1) & 0xfe00_0000, smooth_xfade);
                rough2 = crossfade_u8(wave_1, wave_2, (phase >> 1) & 0xf800_0000, smooth_xfade);
            }
            let (a2, b2) = if self.parameter[1] < 16384 {
                (rough2, smooth2)
            } else {
                (smooth2, rough2)
            };
            sample += mix_i16(a2, b2, balance as u16) as i32;
            phase = phase.wrapping_add(phase_increment);
            rough_xfade = rough_xfade.wrapping_add(rough_xfade_increment);

            buffer[i] = (sample >> 1) as i16;
        }
        self.phase = phase;
        self.previous_parameter[0] = (self.smoothed_parameter >> 1) as i16;
    }

    fn render_wave_paraphonic(&mut self, _sync: &[u8], buffer: &mut [i16], size: usize) {
        const MINI_WAVE_LINE: [u8; 33] = [
            157, 161, 171, 188, 189, 191, 192, 193, 196, 198, 201, 234, 232, 229, 226, 224, 1, 2,
            3, 4, 5, 8, 12, 32, 36, 42, 47, 252, 254, 141, 139, 135, 174,
        ];
        const SEMI: i32 = 128;
        const CHORDS: [[i32; 3]; 17] = [
            [2, 4, 6],
            [16, 32, 48],
            [2 * SEMI, 7 * SEMI, 12 * SEMI],
            [3 * SEMI, 7 * SEMI, 10 * SEMI],
            [3 * SEMI, 7 * SEMI, 12 * SEMI],
            [3 * SEMI, 7 * SEMI, 14 * SEMI],
            [3 * SEMI, 7 * SEMI, 17 * SEMI],
            [7 * SEMI, 12 * SEMI, 19 * SEMI],
            [7 * SEMI, 3 + 12 * SEMI, 5 + 19 * SEMI],
            [4 * SEMI, 7 * SEMI, 17 * SEMI],
            [4 * SEMI, 7 * SEMI, 14 * SEMI],
            [4 * SEMI, 7 * SEMI, 12 * SEMI],
            [4 * SEMI, 7 * SEMI, 11 * SEMI],
            [5 * SEMI, 7 * SEMI, 12 * SEMI],
            [4, 7 * SEMI, 12 * SEMI],
            [4, 4 + 12 * SEMI, 12 * SEMI],
            [4, 4 + 12 * SEMI, 12 * SEMI],
        ];

        if self.strike {
            for i in 0..4 {
                self.state.saw_phase[i] = Random::get_word();
            }
            self.strike = false;
        }

        let phase_increment_0 = self.phase_increment;
        let mut phase_0 = self.state.saw_phase[0];
        let mut phase_1 = self.state.saw_phase[1];
        let mut phase_2 = self.state.saw_phase[2];
        let mut phase_3 = self.state.saw_phase[3];

        let chord_integral = (self.parameter[1] >> 11) as usize;
        // C: `uint16_t chord_fractional = parameter_[1] << 5;` -- wraps in 16 bits.
        let mut chord_fractional = (((self.parameter[1] as u32) << 5) as u16) as u32;
        if chord_fractional < 30720 {
            chord_fractional = 0;
        } else if chord_fractional >= 34816 {
            chord_fractional = 65535;
        } else {
            chord_fractional = (chord_fractional - 30720) * 16;
        }

        let mut phase_increment = [0u32; 3];
        for i in 0..3 {
            // C: `uint16_t detune = detune_1 + ((detune_2 - detune_1) * chord_fractional >> 16);`
            let detune_1 = CHORDS[chord_integral][i];
            let detune_2 = CHORDS[chord_integral + 1][i];
            let detune = (detune_1
                + (((detune_2 - detune_1).wrapping_mul(chord_fractional as i32)) >> 16))
                as u16;
            phase_increment[i] = compute_phase_increment_digital(self.pitch as i32 + detune as i32);
        }

        let w1 = MINI_WAVE_LINE[(self.parameter[0] as usize >> 10).min(32)] as usize;
        let w2 = MINI_WAVE_LINE[((self.parameter[0] as usize >> 10) + 1).min(32)] as usize;
        let wave_1 = &WT_WAVES[w1 * 129..w1 * 129 + 129];
        let wave_2 = &WT_WAVES[w2 * 129..w2 * 129 + 129];
        let wave_xfade = (self.parameter[0] as u32) << 6;

        let mut n = size;
        let mut oi = 0usize;
        while n != 0 {
            let mut sample = 0i32;
            phase_0 = phase_0.wrapping_add(phase_increment_0);
            phase_1 = phase_1.wrapping_add(phase_increment[0]);
            phase_2 = phase_2.wrapping_add(phase_increment[1]);
            phase_3 = phase_3.wrapping_add(phase_increment[2]);
            sample += crossfade_u8(wave_1, wave_2, phase_0 >> 1, wave_xfade as u16) as i32;
            sample += crossfade_u8(wave_1, wave_2, phase_1 >> 1, wave_xfade as u16) as i32;
            sample += crossfade_u8(wave_1, wave_2, phase_2 >> 1, wave_xfade as u16) as i32;
            sample += crossfade_u8(wave_1, wave_2, phase_3 >> 1, wave_xfade as u16) as i32;
            buffer[oi] = (sample >> 2) as i16;

            phase_0 = phase_0.wrapping_add(phase_increment_0);
            phase_1 = phase_1.wrapping_add(phase_increment[0]);
            phase_2 = phase_2.wrapping_add(phase_increment[1]);
            phase_3 = phase_3.wrapping_add(phase_increment[2]);
            sample = 0;
            sample += crossfade_u8(wave_1, wave_2, phase_0 >> 1, wave_xfade as u16) as i32;
            sample += crossfade_u8(wave_1, wave_2, phase_1 >> 1, wave_xfade as u16) as i32;
            sample += crossfade_u8(wave_1, wave_2, phase_2 >> 1, wave_xfade as u16) as i32;
            sample += crossfade_u8(wave_1, wave_2, phase_3 >> 1, wave_xfade as u16) as i32;
            buffer[oi + 1] = (sample >> 2) as i16;
            oi += 2;
            n -= 2;
        }

        self.state.saw_phase[0] = phase_0;
        self.state.saw_phase[1] = phase_1;
        self.state.saw_phase[2] = phase_2;
        self.state.saw_phase[3] = phase_3;
    }

    fn render_filtered_noise(&mut self, _sync: &[u8], buffer: &mut [i16], size: usize) {
        let f = interpolate_824_u16(&LUT_SVF_CUTOFF, (self.pitch as u32) << 17) as i32;
        let damp = interpolate_824_u16(&LUT_SVF_DAMP, (self.parameter[0] as u32) << 17) as i32;
        let scale = interpolate_824_u16(&LUT_SVF_SCALE, (self.parameter[0] as u32) << 17) as i32;
        let mut bp = self.state.svf_bp;
        let mut lp = self.state.svf_lp;
        let (bp_gain, lp_gain, hp_gain);
        if self.parameter[1] < 16384 {
            bp_gain = self.parameter[1] as i32;
            lp_gain = 16384 - bp_gain;
            hp_gain = 0;
        } else {
            bp_gain = 32767 - self.parameter[1] as i32;
            hp_gain = self.parameter[1] as i32 - 16384;
            lp_gain = 0;
        }
        let gain_correction = if f > scale { scale * 32767 / f } else { 32767 };
        for i in 0..size {
            let input = (Random::get_sample() as i32) >> 1;
            let notch = input - (bp * damp >> 15);
            lp += f * bp >> 15;
            lp = clip16_sym(lp);
            let hp = notch - lp;
            bp += f * hp >> 15;

            let mut result = 0i32;
            result += (lp_gain * lp) >> 14;
            result += (bp_gain * bp) >> 14;
            result += (hp_gain * hp) >> 14;
            result = clip16_sym(result);
            result = result * gain_correction >> 15;
            buffer[i] = interpolate_88_i16(&WS_MODERATE_OVERDRIVE, (result + 32768) as u16);
        }
        self.state.svf_lp = lp;
        self.state.svf_bp = bp;
    }

    fn render_twin_peaks_noise(&mut self, _sync: &[u8], buffer: &mut [i16], size: usize) {
        let mut y11 = self.state.pno_filter_state[0][0];
        let mut y12 = self.state.pno_filter_state[0][1];
        let mut y21 = self.state.pno_filter_state[1][0];
        let mut y22 = self.state.pno_filter_state[1][1];
        let q = 65240u32 + (self.parameter[0] as u32 >> 7);
        let q_squared = (q.wrapping_mul(q) >> 17) as i32;
        let p1 = constrain_i32(self.pitch as i32, 0, 16383);
        let mut c1 = interpolate_824_u16(&LUT_RESONATOR_COEFFICIENT, (p1 << 17) as u32) as i32;
        let s1 = interpolate_824_u16(&LUT_RESONATOR_SCALE, (p1 << 17) as u32) as i32;
        let p2 = constrain_i32(
            self.pitch as i32 + ((self.parameter[1] as i32 - 16384) >> 1),
            0,
            16383,
        );
        let mut c2 = interpolate_824_u16(&LUT_RESONATOR_COEFFICIENT, (p2 << 17) as u32) as i32;
        let s2 = interpolate_824_u16(&LUT_RESONATOR_SCALE, (p2 << 17) as u32) as i32;
        // C: `c1 = c1 * q >> 16` in *unsigned* 32-bit (`q` is uint32_t).
        c1 = ((c1 as u32).wrapping_mul(q) >> 16) as i32;
        c2 = ((c2 as u32).wrapping_mul(q) >> 16) as i32;
        let makeup_gain = 8191 - (self.parameter[0] as i32 >> 2);

        let mut n = size;
        let mut oi = 0usize;
        while n != 0 {
            let sample = (Random::get_sample() as i32) >> 1;
            let (mut y10, mut y20);
            if sample > 0 {
                y10 = sample * s1 >> 16;
                y20 = sample * s2 >> 16;
            } else {
                y10 = -((-sample) * s1 >> 16);
                y20 = -((-sample) * s2 >> 16);
            }
            y10 += y11 * c1 >> 15;
            y10 -= y12 * q_squared >> 15;
            y10 = clip16_sym(y10);
            y12 = y11;
            y11 = y10;

            y20 += y21 * c2 >> 15;
            y20 -= y22 * q_squared >> 15;
            y20 = clip16_sym(y20);
            y22 = y21;
            y21 = y20;

            y10 += y20;
            y10 += y10.wrapping_mul(makeup_gain) >> 13;
            y10 = clip16_sym(y10);
            let out = interpolate_88_i16(&WS_MODERATE_OVERDRIVE, (y10 + 32768) as u16);
            buffer[oi] = out;
            buffer[oi + 1] = out;
            oi += 2;
            n -= 2;
        }
        self.state.pno_filter_state[0][0] = y11;
        self.state.pno_filter_state[0][1] = y12;
        self.state.pno_filter_state[1][0] = y21;
        self.state.pno_filter_state[1][1] = y22;
    }

    fn render_clocked_noise(&mut self, sync: &[u8], buffer: &mut [i16], size: usize) {
        if self.parameter[1] > self.previous_parameter[1] + 64
            || self.parameter[1] < self.previous_parameter[1] - 64
        {
            self.previous_parameter[1] = self.parameter[1];
        }
        if self.parameter[0] > self.previous_parameter[0] + 16
            || self.parameter[0] < self.previous_parameter[0] - 16
        {
            self.previous_parameter[0] = self.parameter[0];
        }

        if self.strike {
            self.state.clk_seed = Random::get_word() as i32;
            self.strike = false;
        }

        let mut phase = self.phase;
        let mut phase_increment = self.phase_increment;
        for _ in 0..3 {
            if phase_increment < (1u32 << 31) {
                phase_increment <<= 1;
            }
        }

        self.state.clk_cycle_phase_increment =
            compute_phase_increment_digital(self.previous_parameter[0] as i32 - 16384) << 1;

        let num_steps = {
            let n = 1 + (self.previous_parameter[1] as u32 >> 10);
            if n == 1 {
                2
            } else {
                n
            }
        };
        let quantizer_divider = 65536 / num_steps;
        for i in 0..size {
            phase = phase.wrapping_add(phase_increment);
            if sync[i] != 0 {
                phase = 0;
            }
            if phase < phase_increment {
                self.state.clk_rng_state = self
                    .state
                    .clk_rng_state
                    .wrapping_mul(1_664_525)
                    .wrapping_add(1_013_904_223);
                self.state.clk_cycle_phase = self
                    .state
                    .clk_cycle_phase
                    .wrapping_add(self.state.clk_cycle_phase_increment);
                if self.state.clk_cycle_phase < self.state.clk_cycle_phase_increment {
                    self.state.clk_rng_state = self.state.clk_seed as u32;
                    self.state.clk_cycle_phase = self.state.clk_cycle_phase_increment;
                }
                let mut s = self.state.clk_rng_state as u16;
                s -= s % quantizer_divider as u16;
                s = s.wrapping_add((quantizer_divider >> 1) as u16);
                self.state.clk_sample = s as i16;
                phase = phase_increment;
            }
            buffer[i] = self.state.clk_sample;
        }
        self.phase = phase;
    }

    fn render_granular_cloud(&mut self, _sync: &[u8], buffer: &mut [i16], size: usize) {
        for i in 0..4 {
            if self.state.grain_envelope_phase[i] > (1 << 24)
                || self.state.grain_envelope_phase_increment[i] == 0
            {
                self.state.grain_envelope_phase_increment[i] = 0;
                if (Random::get_word() & 0xffff) < 0x4000 {
                    self.state.grain_envelope_phase_increment[i] =
                        (LUT_GRANULAR_ENVELOPE_RATE[(self.parameter[0] >> 7) as usize] as u32) << 3;
                    self.state.grain_envelope_phase[i] = 0;
                    self.state.grain_phase_increment[i] = self.phase_increment;
                    let pitch_mod =
                        (Random::get_sample() as i32).wrapping_mul(self.parameter[1] as i32) >> 16;
                    let phi = (self.phase_increment >> 8) as i32;
                    if pitch_mod < 0 {
                        self.state.grain_phase_increment[i] = self.state.grain_phase_increment[i]
                            .wrapping_add(phi.wrapping_mul(pitch_mod >> 8) as u32);
                    } else {
                        self.state.grain_phase_increment[i] = self.state.grain_phase_increment[i]
                            .wrapping_add(phi.wrapping_mul(pitch_mod >> 7) as u32);
                    }
                }
            }
        }

        for i in 0..size {
            let mut sample = 0i32;
            for g in 0..4 {
                self.state.grain_phase[g] =
                    self.state.grain_phase[g].wrapping_add(self.state.grain_phase_increment[g]);
                self.state.grain_envelope_phase[g] = self.state.grain_envelope_phase[g]
                    .wrapping_add(self.state.grain_envelope_phase_increment[g]);
                sample += interpolate_824_i16(&WAV_SINE, self.state.grain_phase[g]) as i32
                    * LUT_GRANULAR_ENVELOPE[(self.state.grain_envelope_phase[g] >> 16) as usize]
                        as i32
                    >> 17;
            }
            sample = sample.clamp(-32768, 32767);
            buffer[i] = sample as i16;
        }
    }

    fn render_particle_noise(&mut self, _sync: &[u8], buffer: &mut [i16], size: usize) {
        const PARTICLE_NOISE_DECAY: u32 = 64763;
        // C: `int32_t kResonanceSquared = 32768 * 0.996 * 0.996;` (truncates to 32506)
        const RESONANCE_SQUARED: i32 = 32506;
        // C: `int32_t kResonanceFactor = 32768 * 0.996;` (truncates to 32636)
        const RESONANCE_FACTOR: i32 = 32636;

        let mut amplitude = self.state.pno_amplitude;
        let density = 1024i64 + self.parameter[0] as i64;

        let mut y11 = self.state.pno_filter_state[0][0];
        let mut y12 = self.state.pno_filter_state[0][1];
        let mut s1 = self.state.pno_filter_scale[0];
        let mut c1 = self.state.pno_filter_coefficient[0];
        let mut y21 = self.state.pno_filter_state[1][0];
        let mut y22 = self.state.pno_filter_state[1][1];
        let mut s2 = self.state.pno_filter_scale[1];
        let mut c2 = self.state.pno_filter_coefficient[1];
        let mut y31 = self.state.pno_filter_state[2][0];
        let mut y32 = self.state.pno_filter_state[2][1];
        let mut s3 = self.state.pno_filter_scale[2];
        let mut c3 = self.state.pno_filter_coefficient[2];

        let mut n = size;
        let mut oi = 0usize;
        while n != 0 {
            let noise = Random::get_word();
            if ((noise & 0x7fffff) as i64) < density {
                amplitude = 65535;
                let noise_a = ((noise & 0x0fff) as i32 - 0x800) as i16;
                let noise_b = (((noise >> 15) & 0x1fff) as i32 - 0x1000) as i16;
                let p1 = constrain_i32(
                    self.pitch as i32
                        + (3 * noise_a as i32 * self.parameter[1] as i32 >> 17)
                        + 0x600,
                    0,
                    16383,
                );
                c1 = interpolate_824_u16(&LUT_RESONATOR_COEFFICIENT, (p1 << 17) as u32) as i32;
                s1 = interpolate_824_u16(&LUT_RESONATOR_SCALE, (p1 << 17) as u32) as i32;
                let p2 = constrain_i32(
                    self.pitch as i32 + (noise_a as i32 * self.parameter[1] as i32 >> 15) + 0x980,
                    0,
                    16383,
                );
                c2 = interpolate_824_u16(&LUT_RESONATOR_COEFFICIENT, (p2 << 17) as u32) as i32;
                s2 = interpolate_824_u16(&LUT_RESONATOR_SCALE, (p2 << 17) as u32) as i32;
                let p3 = constrain_i32(
                    self.pitch as i32 + (noise_b as i32 * self.parameter[1] as i32 >> 16) + 0x790,
                    0,
                    16383,
                );
                c3 = interpolate_824_u16(&LUT_RESONATOR_COEFFICIENT, (p3 << 17) as u32) as i32;
                s3 = interpolate_824_u16(&LUT_RESONATOR_SCALE, (p3 << 17) as u32) as i32;
                c1 = c1 * RESONANCE_FACTOR >> 15;
                c2 = c2 * RESONANCE_FACTOR >> 15;
                c3 = c3 * RESONANCE_FACTOR >> 15;
            }
            let sample = ((noise as i16 as i32) * amplitude as i32) >> 16;
            amplitude = ((amplitude as u32 * PARTICLE_NOISE_DECAY) >> 16) as u16;

            let (mut y10, mut y20, mut y30);
            if sample > 0 {
                y10 = sample * s1 >> 16;
                y20 = sample * s2 >> 16;
                y30 = sample * s3 >> 16;
            } else {
                y10 = -((-sample) * s1 >> 16);
                y20 = -((-sample) * s2 >> 16);
                y30 = -((-sample) * s3 >> 16);
            }
            y10 += y11 * c1 >> 15;
            y10 -= y12 * RESONANCE_SQUARED >> 15;
            y10 = clip16_sym(y10);
            y12 = y11;
            y11 = y10;
            y20 += y21 * c2 >> 15;
            y20 -= y22 * RESONANCE_SQUARED >> 15;
            y20 = clip16_sym(y20);
            y22 = y21;
            y21 = y20;
            y30 += y31 * c3 >> 15;
            y30 -= y32 * RESONANCE_SQUARED >> 15;
            y30 = clip16_sym(y30);
            y32 = y31;
            y31 = y30;

            y10 += y20 + y30;
            y10 = clip16_sym(y10);
            buffer[oi] = y10 as i16;
            buffer[oi + 1] = y10 as i16;
            oi += 2;
            n -= 2;
        }

        self.state.pno_amplitude = amplitude;
        self.state.pno_filter_state[0][0] = y11;
        self.state.pno_filter_state[0][1] = y12;
        self.state.pno_filter_scale[0] = s1;
        self.state.pno_filter_coefficient[0] = c1;
        self.state.pno_filter_state[1][0] = y21;
        self.state.pno_filter_state[1][1] = y22;
        self.state.pno_filter_scale[1] = s2;
        self.state.pno_filter_coefficient[1] = c2;
        self.state.pno_filter_state[2][0] = y31;
        self.state.pno_filter_state[2][1] = y32;
        self.state.pno_filter_scale[2] = s3;
        self.state.pno_filter_coefficient[2] = c3;
    }

    fn render_digital_modulation(&mut self, _sync: &[u8], buffer: &mut [i16], size: usize) {
        const CONSTELLATION_Q: [i32; 4] = [23100, -23100, -23100, 23100];
        const CONSTELLATION_I: [i32; 4] = [23100, 23100, -23100, -23100];

        let mut phase = self.phase;
        let increment = self.phase_increment;
        let mut symbol_stream_phase = self.state.dmd_symbol_phase;
        let symbol_stream_phase_increment = compute_phase_increment_digital(
            self.pitch as i32 - 1536 + ((self.parameter[0] as i32 - 32767) >> 3),
        );
        let mut data_byte = self.state.dmd_data_byte;

        if self.strike {
            self.state.dmd_symbol_count = 0;
            self.strike = false;
        }

        for i in 0..size {
            phase = phase.wrapping_add(increment);
            symbol_stream_phase = symbol_stream_phase.wrapping_add(symbol_stream_phase_increment);
            if symbol_stream_phase < symbol_stream_phase_increment {
                self.state.dmd_symbol_count = self.state.dmd_symbol_count.wrapping_add(1);
                if self.state.dmd_symbol_count & 3 == 0 {
                    if self.state.dmd_symbol_count >= (64 + 4 * 256) {
                        self.state.dmd_symbol_count = 0;
                    }
                    if self.state.dmd_symbol_count < 32 {
                        data_byte = 0x00;
                    } else if self.state.dmd_symbol_count < 48 {
                        data_byte = 0x99;
                    } else if self.state.dmd_symbol_count < 64 {
                        data_byte = 0xcc;
                    } else {
                        self.state.dmd_filter_state =
                            (self.state.dmd_filter_state * 3 + self.parameter[1] as i32) >> 2;
                        data_byte = (self.state.dmd_filter_state >> 7) as u8;
                    }
                } else {
                    data_byte >>= 2;
                }
            }
            let iq_i = interpolate_824_i16(&WAV_SINE, phase) as i32;
            let iq_q = interpolate_824_i16(&WAV_SINE, phase.wrapping_add(1 << 30)) as i32;
            buffer[i] = ((CONSTELLATION_Q[(data_byte & 3) as usize] * iq_q >> 15)
                + (CONSTELLATION_I[(data_byte & 3) as usize] * iq_i >> 15))
                as i16;
        }
        self.phase = phase;
        self.state.dmd_symbol_phase = symbol_stream_phase;
        self.state.dmd_data_byte = data_byte;
    }

    fn render_question_mark(&mut self, _sync: &[u8], buffer: &mut [i16], size: usize) {
        if self.strike {
            self.state.clk_rng_state = 0;
            self.state.clk_cycle_phase = 0;
            self.state.clk_sample = 10;
            self.state.clk_cycle_phase_increment = u32::MAX; // -1
            self.state.clk_seed = 32767;
            self.strike = false;
        }

        let mut phase = self.phase;
        let increment = self.phase_increment;
        let dit_duration = 3600u32 + ((32767 - self.parameter[0] as u32) >> 2);
        let noise_threshold = 1024i32 + (self.parameter[1] as i32 >> 3);
        for i in 0..size {
            phase = phase.wrapping_add(increment);
            let mut sample = if self.state.clk_rng_state != 0 {
                (interpolate_824_i16(&WAV_SINE, phase) as i32 * 3) >> 2
            } else {
                0
            };
            self.state.clk_cycle_phase = self.state.clk_cycle_phase.wrapping_add(1);
            if self.state.clk_cycle_phase > dit_duration {
                self.state.clk_sample = self.state.clk_sample.wrapping_sub(1);
                if self.state.clk_sample == 0 {
                    self.state.clk_cycle_phase_increment =
                        self.state.clk_cycle_phase_increment.wrapping_add(1);
                    self.state.clk_rng_state = (self.state.clk_rng_state == 0) as u32;

                    let address = (self.state.clk_cycle_phase_increment >> 2) as usize;
                    let shift = ((self.state.clk_cycle_phase_increment & 0x3) << 1) as u32;
                    self.state.clk_sample =
                        ((2i32 << ((WT_CODE[address] >> shift) & 3)) - 1) as i16;
                    if self.state.clk_sample == 15 {
                        self.state.clk_sample = 100;
                        self.state.clk_rng_state = 0;
                        self.state.clk_cycle_phase_increment = u32::MAX;
                    }
                    phase = 1 << 30;
                }
                self.state.clk_cycle_phase = 0;
            }
            self.state.clk_seed = self
                .state
                .clk_seed
                .wrapping_add((Random::get_sample() as i32) >> 2);
            let mut noise_intensity = self.state.clk_seed >> 8;
            if noise_intensity < 0 {
                noise_intensity = -noise_intensity;
            }
            noise_intensity = noise_intensity.max(noise_threshold).min(16000);
            let mut noise = Random::get_sample() as i32 * noise_intensity >> 15;
            noise = noise * WAV_SINE[((phase >> 22) & 0xff) as usize] as i32 >> 15;
            sample += noise;
            sample = clip16_sym(sample);
            let distorted = sample * sample >> 14;
            sample += distorted * self.parameter[1] as i32 >> 15;
            sample = clip16_sym(sample);
            buffer[i] = sample as i16;
        }
        self.phase = phase;
    }

    fn render_kick(&mut self, _sync: &[u8], buffer: &mut [i16], size: usize) {
        if self.init {
            self.pulse[0].init();
            self.pulse[0].set_delay(0);
            self.pulse[0].set_decay(3340);
            self.pulse[1].init();
            self.pulse[1].set_delay((1.0e-3 * 48000.0) as u32);
            self.pulse[1].set_decay(3072);
            self.pulse[2].init();
            self.pulse[2].set_delay((4.0e-3 * 48000.0) as u32);
            self.pulse[2].set_decay(4093);
            self.svf[0].init();
            self.svf[0].set_punch(32768);
            self.svf[0].set_mode(SvfMode::Bp);
            self.init = false;
        }

        if self.strike {
            self.strike = false;
            self.pulse[0].trigger((12.0 * 32768.0 * 0.7) as i32);
            self.pulse[1].trigger((-19662.0 * 0.7) as i32);
            self.pulse[2].trigger(18000);
            self.svf[0].set_punch(24000);
        }

        let decay = self.parameter[0] as u32;
        let mut scaled = 65535u32.wrapping_sub(decay << 1);
        let squared = scaled.wrapping_mul(scaled) >> 16;
        scaled = squared.wrapping_mul(scaled) >> 18;
        self.svf[0].set_resonance((32768 - 128 - scaled as i32) as i16);

        let mut coefficient = self.parameter[1] as u32;
        coefficient = coefficient.wrapping_mul(coefficient) >> 15;
        coefficient = coefficient.wrapping_mul(coefficient) >> 15;
        let lp_coefficient = 128 + (coefficient >> 1) as i32 * 3;
        let mut lp_state = self.state.svf_lp;

        let mut n = size;
        let mut oi = 0usize;
        while n != 0 {
            let mut excitation = 0i32;
            excitation += self.pulse[0].process();
            excitation += if !self.pulse[1].done() { 16384 } else { 0 };
            excitation += self.pulse[1].process();
            self.pulse[2].process();
            self.svf[0].set_frequency(self.pitch + if self.pulse[2].done() { 0 } else { 17 << 7 });

            for _ in 0..2 {
                let resonator_output = (excitation >> 4) + self.svf[0].process(excitation);
                lp_state += (resonator_output - lp_state) * lp_coefficient >> 15;
                lp_state = clip16_sym(lp_state);
                buffer[oi] = lp_state as i16;
                oi += 1;
            }
            n -= 2;
        }
        self.state.svf_lp = lp_state;
    }

    fn render_snare(&mut self, _sync: &[u8], buffer: &mut [i16], size: usize) {
        if self.init {
            self.pulse[0].init();
            self.pulse[0].set_delay(0);
            self.pulse[0].set_decay(1536);
            self.pulse[1].init();
            self.pulse[1].set_delay((1e-3 * 48000.0) as u32);
            self.pulse[1].set_decay(3072);
            self.pulse[2].init();
            self.pulse[2].set_delay((1e-3 * 48000.0) as u32);
            self.pulse[2].set_decay(1200);
            self.pulse[3].init();
            self.pulse[3].set_delay(0);
            self.svf[0].init();
            self.svf[1].init();
            self.svf[2].init();
            self.svf[2].set_resonance(2000);
            self.svf[2].set_mode(SvfMode::Bp);
            self.init = false;
        }

        if self.strike {
            let mut decay = 49152 - self.pitch as i32;
            decay += if self.parameter[1] < 16384 {
                0
            } else {
                self.parameter[1] as i32 - 16384
            };
            if decay > 65535 {
                decay = 65535;
            }
            self.svf[0].set_resonance((29000 + (decay >> 5)) as i16);
            self.svf[1].set_resonance((26500 + (decay >> 5)) as i16);
            self.pulse[3].set_decay((4092 + (decay >> 14)) as u32);

            self.pulse[0].trigger(15 * 32768);
            self.pulse[1].trigger(-32768);
            self.pulse[2].trigger(13107);
            let snappy = (self.parameter[1] as i32).min(14336);
            self.pulse[3].trigger(512 + (snappy << 1));
            self.strike = false;
        }

        self.svf[0].set_frequency(self.pitch + (12 << 7));
        self.svf[1].set_frequency(self.pitch + (24 << 7));
        self.svf[2].set_frequency(self.pitch + (60 << 7));

        let g_1 = 22000 - (self.parameter[0] as i32 >> 1);
        let g_2 = 22000 + (self.parameter[0] as i32 >> 1);

        let mut n = size;
        let mut oi = 0usize;
        while n != 0 {
            let mut excitation_1 = 0i32;
            excitation_1 += self.pulse[0].process();
            excitation_1 += self.pulse[1].process();
            excitation_1 += if !self.pulse[1].done() { 2621 } else { 0 };

            let mut excitation_2 = 0i32;
            excitation_2 += self.pulse[2].process();
            excitation_2 += if !self.pulse[2].done() { 13107 } else { 0 };

            let noise_sample =
                (Random::get_sample() as i32).wrapping_mul(self.pulse[3].process()) >> 15;

            let mut sd = 0i32;
            sd += (self.svf[0].process(excitation_1) + (excitation_1 >> 4)) * g_1 >> 15;
            sd += (self.svf[1].process(excitation_2) + (excitation_2 >> 4)) * g_2 >> 15;
            sd += self.svf[2].process(noise_sample);
            sd = clip16_sym(sd);

            buffer[oi] = sd as i16;
            buffer[oi + 1] = sd as i16;
            oi += 2;
            n -= 2;
        }
    }

    fn render_cymbal(&mut self, _sync: &[u8], buffer: &mut [i16], size: usize) {
        if self.init {
            self.svf[0].init();
            self.svf[0].set_mode(SvfMode::Bp);
            self.svf[0].set_resonance(12000);
            self.svf[1].init();
            self.svf[1].set_mode(SvfMode::Hp);
            self.svf[1].set_resonance(2000);
            self.init = false;
        }

        let mut increments = [0u32; 7];
        let note = (40 << 7) + (self.pitch as i32 >> 1);
        increments[0] = compute_phase_increment_digital(note);
        let root = increments[0] >> 10;
        increments[1] = root.wrapping_mul(24273) >> 4;
        increments[2] = root.wrapping_mul(12561) >> 4;
        increments[3] = root.wrapping_mul(18417) >> 4;
        increments[4] = root.wrapping_mul(22452) >> 4;
        increments[5] = root.wrapping_mul(31858) >> 4;
        increments[6] = increments[0].wrapping_mul(24);

        let xfade = self.parameter[1] as i32;
        self.svf[0].set_frequency(self.parameter[0] >> 1);
        self.svf[1].set_frequency(self.parameter[0] >> 1);

        for i in 0..size {
            self.phase = self.phase.wrapping_add(increments[6]);
            if self.phase < increments[6] {
                self.state.hat_rng_state = self
                    .state
                    .hat_rng_state
                    .wrapping_mul(1_664_525)
                    .wrapping_add(1_013_904_223);
            }
            for k in 0..6 {
                self.state.hat_phase[k] = self.state.hat_phase[k].wrapping_add(increments[k]);
            }
            let mut hat_noise = 0i32;
            for k in 0..6 {
                hat_noise += (self.state.hat_phase[k] >> 31) as i32;
            }
            hat_noise -= 3;
            hat_noise *= 5461;
            hat_noise = self.svf[0].process(hat_noise);
            hat_noise = clip16_sym(hat_noise);

            let mut noise = (self.state.hat_rng_state >> 16) as i32 - 32768;
            noise = self.svf[1].process(noise >> 1);
            noise = clip16_sym(noise);

            buffer[i] = (hat_noise + ((noise - hat_noise) * xfade >> 15)) as i16;
        }
    }
}

// ------------------------------------------------------------------------
// Static tables local to `digital_oscillator.cc`
// ------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct PhonemeDefinition {
    f: [u8; 3],
    a: [u8; 3],
}

struct WavetableDefinition {
    num_steps: u8,
    wave_index: [u8; 17],
}

#[rustfmt::skip]
static WAVETABLE_DEFINITIONS: [WavetableDefinition; 20] = [
    WavetableDefinition { num_steps: 16, wave_index: [0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,15] },
    WavetableDefinition { num_steps: 16, wave_index: [16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31,31] },
    WavetableDefinition { num_steps: 16, wave_index: [32,33,34,35,36,37,38,39,40,41,42,43,44,45,46,47,47] },
    WavetableDefinition { num_steps: 16, wave_index: [48,49,50,51,52,53,54,55,56,57,58,59,60,61,62,63,63] },
    WavetableDefinition { num_steps: 16, wave_index: [64,65,66,67,68,68,69,70,71,72,73,73,74,75,75,76,76] },
    WavetableDefinition { num_steps: 16, wave_index: [77,78,79,80,81,82,83,84,85,86,87,88,89,90,91,92,92] },
    WavetableDefinition { num_steps: 16, wave_index: [93,94,95,96,97,98,99,100,101,102,103,104,105,106,107,108,108] },
    WavetableDefinition { num_steps: 16, wave_index: [109,110,111,112,113,114,115,116,117,118,119,120,121,122,123,124,124] },
    WavetableDefinition { num_steps: 16, wave_index: [125,126,127,128,129,130,131,132,132,132,132,132,132,132,132,132,132] },
    WavetableDefinition { num_steps: 16, wave_index: [133,134,135,136,137,138,139,140,141,142,143,144,144,144,145,145,145] },
    WavetableDefinition { num_steps: 16, wave_index: [146,147,148,149,150,151,151,151,152,152,152,152,153,153,153,153,153] },
    WavetableDefinition { num_steps: 8,  wave_index: [154,154,154,154,154,154,155,156,156,0,0,0,0,0,0,0,0] },
    WavetableDefinition { num_steps: 16, wave_index: [176,157,158,159,160,161,162,163,164,165,166,167,168,169,170,171,171] },
    WavetableDefinition { num_steps: 16, wave_index: [172,173,174,175,176,177,178,179,180,181,182,183,184,185,186,187,187] },
    WavetableDefinition { num_steps: 16, wave_index: [176,188,189,190,191,192,193,194,195,196,197,198,199,200,201,202,202] },
    WavetableDefinition { num_steps: 16, wave_index: [203,205,204,205,212,206,207,208,208,209,210,210,211,211,212,212,212] },
    WavetableDefinition { num_steps: 8,  wave_index: [213,213,213,214,215,216,217,218,219,0,0,0,0,0,0,0,0] },
    WavetableDefinition { num_steps: 16, wave_index: [220,221,222,223,224,225,226,227,228,229,230,231,232,233,234,235,235] },
    WavetableDefinition { num_steps: 16, wave_index: [236,237,238,239,240,241,242,243,244,245,246,247,248,249,250,251,251] },
    WavetableDefinition { num_steps: 4,  wave_index: [252,253,254,255,254,0,0,0,0,0,0,0,0,0,0,0,0] },
];

#[rustfmt::skip]
static FORMANT_F_DATA: [[[i16; NUM_FORMANTS]; NUM_FORMANTS]; NUM_FORMANTS] = [
    [
        [9519, 10738, 12448, 12636, 12892],
        [8620, 11720, 12591, 12932, 13158],
        [7579, 11891, 12768, 13122, 13323],
        [8620, 10013, 12591, 12768, 13010],
        [8324, 9519, 12591, 12831, 13048],
    ],
    [
        [9696, 10821, 12810, 13010, 13263],
        [8620, 11827, 12768, 13228, 13477],
        [7908, 12038, 12932, 13263, 13452],
        [8620, 10156, 12768, 12932, 13085],
        [8324, 9519, 12852, 13010, 13296],
    ],
    [
        [9730, 10902, 12892, 13085, 13330],
        [8832, 11953, 12852, 13085, 13296],
        [7749, 12014, 13010, 13330, 13483],
        [8781, 10211, 12852, 13085, 13296],
        [8448, 9627, 12892, 13085, 13363],
    ],
    [
        [10156, 10960, 12932, 13427, 14195],
        [8620, 11692, 12852, 13296, 14195],
        [8324, 11827, 12852, 13550, 14195],
        [8881, 10156, 12956, 13427, 14195],
        [8160, 9860, 12708, 13427, 14195],
    ],
    [
        [10156, 10960, 13010, 13667, 14195],
        [8324, 12187, 12932, 13489, 14195],
        [7749, 12337, 13048, 13667, 14195],
        [8881, 10156, 12956, 13609, 14195],
        [8160, 9860, 12852, 13609, 14195],
    ],
];

#[rustfmt::skip]
static FORMANT_A_DATA: [[[i16; NUM_FORMANTS]; NUM_FORMANTS]; NUM_FORMANTS] = [
    [
        [16384, 7318, 5813, 5813, 1638],
        [16384, 4115, 5813, 4115, 2062],
        [16384, 518, 2596, 1301, 652],
        [16384, 4617, 1460, 1638, 163],
        [16384, 1638, 411, 652, 259],
    ],
    [
        [16384, 8211, 7318, 6522, 1301],
        [16384, 3269, 4115, 3269, 1638],
        [16384, 2913, 2062, 1638, 518],
        [16384, 5181, 4115, 4115, 821],
        [16384, 1638, 2314, 3269, 821],
    ],
    [
        [16384, 8211, 1159, 1033, 206],
        [16384, 3269, 2062, 1638, 1638],
        [16384, 1033, 1033, 259, 259],
        [16384, 5181, 821, 1301, 326],
        [16384, 1638, 1159, 518, 326],
    ],
    [
        [16384, 10337, 1638, 259, 16],
        [16384, 1033, 518, 291, 16],
        [16384, 1638, 518, 259, 16],
        [16384, 5813, 2596, 652, 29],
        [16384, 4115, 518, 163, 10],
    ],
    [
        [16384, 8211, 411, 1638, 51],
        [16384, 1638, 2913, 163, 25],
        [16384, 4115, 821, 821, 103],
        [16384, 4617, 1301, 1301, 51],
        [16384, 2596, 291, 163, 16],
    ],
];

/// `DigitalOscillator::InterpolateFormantParameter`.
fn interpolate_formant_parameter(
    table: &[[[i16; NUM_FORMANTS]; NUM_FORMANTS]; NUM_FORMANTS],
    x: i16,
    y: i16,
    formant: usize,
) -> i16 {
    let x_index = (x >> 13) as usize;
    let x_mix = (x << 3) as u16 as i32;
    let y_index = (y >> 13) as usize;
    let y_mix = (y << 3) as u16 as i32;
    let mut a = table[x_index][y_index][formant] as i32;
    let b = table[x_index + 1][y_index][formant] as i32;
    let mut c = table[x_index][y_index + 1][formant] as i32;
    let d = table[x_index + 1][y_index + 1][formant] as i32;
    a += (b - a) * x_mix >> 16;
    c += (d - c) * x_mix >> 16;
    (a + ((c - a) * y_mix >> 16)) as i16
}
