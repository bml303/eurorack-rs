//! `elements/dsp/ominous_voice.{h,cc}` -- "Ominous", the hidden easter-egg
//! voice: a dark 2x2-operator FM synth (Braids' FM / FBFM modes), 8x
//! oversampled, downsampled with an IIR + a 101-tap FIR, then band-pass
//! filtered and spatialised into the stereo field.

use stmlib::fdsp::clip16;
use stmlib::filter::{FilterMode, FrequencyApproximation, NaiveSvf, Svf};

use crate::dsp::MAX_BLOCK_SIZE;
use crate::multistage_envelope::{
    FLAG_FALLING_EDGE, FLAG_GATE, FLAG_RISING_EDGE, MultistageEnvelope,
};
use crate::part::Patch;
use crate::resources::{
    LUT_DETUNE_QUANTIZER, LUT_FM_FREQUENCY_QUANTIZER, LUT_MIDI_TO_F_HIGH, LUT_MIDI_TO_F_LOW,
    LUT_MIDI_TO_INCREMENT_HIGH, LUT_SINE,
};

const OVERSAMPLING_DOWN_MIDI: f32 = -36.0;
const OVERSAMPLING_UP: usize = 8;
const NUM_OSCILLATORS: usize = 2;

const FIR_SIZE: usize = 101;
const FIR_BUFFER_SIZE: usize = 128;

// scipy.signal.remez(101, [0, 0.3 / 8, 0.495 / 8, 0.5], [1, 0]);
#[rustfmt::skip]
const DOWNSAMPLING_FILTER: [f32; FIR_SIZE] = [
    -0.001859272945,  0.001184937535,  0.001212413444,  0.001369688661,
     0.001555406705,  0.001685761819,  0.001692922383,  0.001526182555,
     0.001157229282,  0.000582212588, -0.000172916131, -0.001054896973,
    -0.001981709311, -0.002852566234, -0.003555372122, -0.003977161477,
    -0.004020648315, -0.003616652394, -0.002735861951, -0.001399720214,
     0.000314317050,  0.002272758644,  0.004295073578,  0.006166173855,
     0.007654982354,  0.008534833653,  0.008614353501,  0.007761247708,
     0.005919024408,  0.003133736224, -0.000445934810, -0.004563290490,
    -0.008867277113, -0.012932060625, -0.016287075604, -0.018456326393,
    -0.018994127671, -0.017542469537, -0.013838153737, -0.007789574712,
     0.000541523640,  0.010920252821,  0.022936167260,  0.036030557729,
     0.049530599001,  0.062696232995,  0.074769247024,  0.085030908865,
     0.092854033134,  0.097753623479,  0.099422113893,  0.097753623479,
     0.092854033134,  0.085030908865,  0.074769247024,  0.062696232995,
     0.049530599001,  0.036030557729,  0.022936167260,  0.010920252821,
     0.000541523640, -0.007789574712, -0.013838153737, -0.017542469537,
    -0.018994127671, -0.018456326393, -0.016287075604, -0.012932060625,
    -0.008867277113, -0.004563290490, -0.000445934810,  0.003133736224,
     0.005919024408,  0.007761247708,  0.008614353501,  0.008534833653,
     0.007654982354,  0.006166173855,  0.004295073578,  0.002272758644,
     0.000314317050, -0.001399720214, -0.002735861951, -0.003616652394,
    -0.004020648315, -0.003977161477, -0.003555372122, -0.002852566234,
    -0.001981709311, -0.001054896973, -0.000172916131,  0.000582212588,
     0.001157229282,  0.001526182555,  0.001692922383,  0.001685761819,
     0.001555406705,  0.001369688661,  0.001212413444,  0.001184937535,
    -0.001859272945,
];

/// Positive-wrapping table lookup (`InterpolateWrap` in the C, which relies on
/// negative-index UB for `x < 0`; here it folds into `[0, 1)` instead).
#[inline]
fn interpolate_wrap(table: &[f32], index: f32, size: f32) -> f32 {
    let mut frac = index - (index as i32) as f32;
    if frac < 0.0 {
        frac += 1.0;
    }
    let scaled = frac * size;
    let integral = scaled as usize;
    let fractional = scaled - integral as f32;
    let a = table[integral];
    let b = table[integral + 1];
    a + (b - a) * fractional
}

#[inline]
fn f32_to_u32_wrap(x: f32) -> u32 {
    (x as i64) as u32
}

/// `stmlib::Interpolate` with the C's "one past the end * 0" clamped in bounds
/// (see the note in `resonator.rs`).
#[inline]
fn interpolate(table: &[f32], index: f32, size: f32) -> f32 {
    let scaled = index.clamp(0.0, 1.0) * size;
    let last = table.len() - 1;
    let integral = (scaled as usize).min(last);
    let fractional = scaled - integral as f32;
    let a = table[integral];
    let b = table[(integral + 1).min(last)];
    a + (b - a) * fractional
}

// -- FIR downsampler ---------------------------------------------------------

#[derive(Debug, Clone)]
struct FirDownsampler {
    ptr: usize,
    buffer: [f32; FIR_BUFFER_SIZE * 2],
}

impl FirDownsampler {
    fn new() -> Self {
        Self {
            ptr: 0,
            buffer: [0.0; FIR_BUFFER_SIZE * 2],
        }
    }

    fn init(&mut self) {
        self.buffer.fill(0.0);
        self.ptr = 0;
    }

    /// `Process(in, out, size)` -- `size` a multiple of `OVERSAMPLING_UP`;
    /// writes `size / OVERSAMPLING_UP` output samples.
    fn process(&mut self, input: &[f32], out: &mut [f32], size: usize) {
        let mut in_idx = 0;
        let mut out_idx = 0;
        let mut remaining = size;
        while remaining != 0 {
            for _ in 0..OVERSAMPLING_UP {
                self.buffer[self.ptr] = input[in_idx];
                self.buffer[self.ptr + FIR_BUFFER_SIZE] = input[in_idx];
                in_idx += 1;
                self.ptr = (self.ptr + (FIR_BUFFER_SIZE - 1)) & (FIR_BUFFER_SIZE - 1);
                remaining -= 1;
            }
            let mut s = 0.0f32;
            for i in 0..FIR_SIZE {
                s += self.buffer[self.ptr + i + 1] * DOWNSAMPLING_FILTER[i];
            }
            out[out_idx] = s;
            out_idx += 1;
        }
    }
}

// -- Spatializer -----------------------------------------------------------

#[derive(Debug, Clone)]
struct Spatializer {
    behind: [f32; MAX_BLOCK_SIZE],
    left: f32,
    right: f32,
    angle: f32,
    distance: f32,
    fixed_position: f32,
    behind_filter: NaiveSvf,
}

impl Spatializer {
    fn new() -> Self {
        Self {
            behind: [0.0; MAX_BLOCK_SIZE],
            left: 0.0,
            right: 0.0,
            angle: 0.0,
            distance: 0.0,
            fixed_position: 0.0,
            behind_filter: NaiveSvf::default(),
        }
    }

    fn init(&mut self, fixed_position: f32) {
        self.angle = 0.0;
        self.fixed_position = fixed_position;
        self.left = 0.0;
        self.right = 0.0;
        self.distance = 0.0;
        self.behind_filter.init();
        self.behind_filter
            .set_f_q(0.05, 1.0, FrequencyApproximation::Exact);
    }

    #[inline]
    fn rotate(&mut self, rotation_speed: f32) {
        self.angle += rotation_speed;
        if self.angle >= 1.0 {
            self.angle -= 1.0;
        }
        if self.angle < 0.0 {
            self.angle += 1.0;
        }
    }

    #[inline]
    fn set_distance(&mut self, distance: f32) {
        self.distance = distance;
    }

    fn process(&mut self, source: &[f32], center: &mut [f32], sides: &mut [f32], size: usize) {
        self.behind_filter.process_block(
            FilterMode::LowPass,
            &source[..size],
            &mut self.behind[..size],
        );

        let angle = self.angle;
        let mut x = self.distance * interpolate_wrap(&LUT_SINE, angle, 4096.0);
        let y = self.distance * interpolate_wrap(&LUT_SINE, angle + 0.25, 4096.0);
        let backfront = (1.0 + y) * 0.5 * self.distance;
        x += self.fixed_position * (1.0 - self.distance);

        let target_left = interpolate_wrap(&LUT_SINE, (1.0 + x) * 0.125, 4096.0);
        let target_right = interpolate_wrap(&LUT_SINE, (3.0 + x) * 0.125, 4096.0);

        let step = 1.0 / size as f32;
        let left_increment = (target_left - self.left) * step;
        let right_increment = (target_right - self.right) * step;

        for i in 0..size {
            self.left += left_increment;
            self.right += right_increment;
            let y = source[i] + backfront * (self.behind[i] - source[i]);
            let l = self.left * y;
            let r = self.right * y;
            center[i] += (l + r) * 0.5;
            sides[i] += (l - r) * 0.5 / 0.7;
        }
    }
}

// -- FM operator ---------------------------------------------------------

#[derive(Debug, Clone, Default)]
struct FmOscillator {
    fm_amount: f32,
    previous_sample: f32,
    phase_carrier: u32,
    phase_mod: u32,
}

impl FmOscillator {
    fn init(&mut self) {
        self.fm_amount = 0.0;
        self.previous_sample = 0.0;
        self.phase_carrier = 0;
        self.phase_mod = 0;
    }

    #[inline]
    fn midi_to_increment(midi_pitch: f32) -> u32 {
        let pitch = (midi_pitch * 256.0) as i32;
        let pitch = 32768 + clip16(pitch - 20480);
        let increment = LUT_MIDI_TO_INCREMENT_HIGH[(pitch >> 8) as usize]
            * LUT_MIDI_TO_F_LOW[(pitch & 0xff) as usize];
        increment as u32
    }

    #[inline]
    fn sine_fm(phase: u32, fm: f32) -> f32 {
        let phase = phase.wrapping_add(f32_to_u32_wrap(fm * 2_147_483_648.0));
        let integral = (phase >> 20) as usize;
        let fractional = (phase << 12) as f32 / 4_294_967_296.0;
        let a = LUT_SINE[integral];
        let b = LUT_SINE[integral + 1];
        a + (b - a) * fractional
    }

    fn process(
        &mut self,
        frequency: f32,
        ratio: f32,
        feedback_amount: f32,
        target_fm_amount: f32,
        external_fm: &[f32],
        destination: &mut [f32],
        size: usize,
    ) {
        let ratio = interpolate(&LUT_FM_FREQUENCY_QUANTIZER, ratio, 128.0);

        let inc_carrier = Self::midi_to_increment(frequency);
        let inc_mod = Self::midi_to_increment(frequency + ratio);

        let mut phase_carrier = self.phase_carrier;
        let mut phase_mod = self.phase_mod;

        let step = 1.0 / size as f32;
        let mut fm_amount = self.fm_amount;
        let fm_amount_increment = (target_fm_amount - fm_amount) * step;
        let mut previous_sample = self.previous_sample;

        // Reduce FM depth when frequency or feedback are high, to limit aliasing.
        let brightness = frequency + ratio * 0.75 - 60.0 + feedback_amount * 24.0;
        let mut amount_attenuation = if brightness <= 0.0 {
            1.0
        } else {
            1.0 - brightness * brightness * 0.0015
        };
        if amount_attenuation < 0.0 {
            amount_attenuation = 0.0;
        }

        for i in 0..size {
            fm_amount += fm_amount_increment;
            phase_carrier = phase_carrier.wrapping_add(inc_carrier);
            phase_mod = phase_mod.wrapping_add(inc_mod);
            let m = Self::sine_fm(phase_mod, feedback_amount * previous_sample);
            previous_sample = Self::sine_fm(
                phase_carrier,
                amount_attenuation * (m * fm_amount + external_fm[i]),
            );
            destination[i] = previous_sample;
        }

        self.phase_carrier = phase_carrier;
        self.phase_mod = phase_mod;
        self.fm_amount = fm_amount;
        self.previous_sample = previous_sample;
    }
}

// -- OminousVoice --------------------------------------------------------

/// `elements::OminousVoice`.
pub struct OminousVoice {
    external_fm_oversampled: [f32; OVERSAMPLING_UP * MAX_BLOCK_SIZE],
    osc_oversampled: [f32; OVERSAMPLING_UP * MAX_BLOCK_SIZE],
    osc: [f32; MAX_BLOCK_SIZE],

    previous_gate: bool,
    envelope: MultistageEnvelope,

    level_state: f32,
    damping: f32,
    feedback: f32,

    osc_level: [f32; NUM_OSCILLATORS],
    external_fm_state: [f32; NUM_OSCILLATORS],

    oscillator: [FmOscillator; NUM_OSCILLATORS],
    iir_downsampler: [NaiveSvf; NUM_OSCILLATORS],
    fir_downsampler: [FirDownsampler; NUM_OSCILLATORS],
    filter: [Svf; NUM_OSCILLATORS],
    spatializer: [Spatializer; NUM_OSCILLATORS],
}

impl Default for OminousVoice {
    fn default() -> Self {
        Self::new()
    }
}

impl OminousVoice {
    pub fn new() -> Self {
        let mut v = Self {
            external_fm_oversampled: [0.0; OVERSAMPLING_UP * MAX_BLOCK_SIZE],
            osc_oversampled: [0.0; OVERSAMPLING_UP * MAX_BLOCK_SIZE],
            osc: [0.0; MAX_BLOCK_SIZE],
            previous_gate: false,
            envelope: MultistageEnvelope::new(),
            level_state: 0.0,
            damping: 0.0,
            feedback: 0.0,
            osc_level: [0.0; NUM_OSCILLATORS],
            external_fm_state: [0.0; NUM_OSCILLATORS],
            oscillator: [FmOscillator::default(), FmOscillator::default()],
            iir_downsampler: [NaiveSvf::default(), NaiveSvf::default()],
            fir_downsampler: [FirDownsampler::new(), FirDownsampler::new()],
            filter: [Svf::default(), Svf::default()],
            spatializer: [Spatializer::new(), Spatializer::new()],
        };
        v.init();
        v
    }

    /// `Init`.
    pub fn init(&mut self) {
        self.envelope.init();
        self.envelope.set_adsr(0.5, 0.5, 0.5, 0.5);
        self.previous_gate = false;
        self.level_state = 0.0;
        self.damping = 0.0;
        self.feedback = 0.0;

        for i in 0..NUM_OSCILLATORS {
            self.external_fm_state[i] = 0.0;
            self.oscillator[i].init();

            self.fir_downsampler[i].init();
            self.iir_downsampler[i].init();
            self.iir_downsampler[i].set_f_q(
                1.0 / OVERSAMPLING_UP as f32 * 0.8,
                0.5,
                FrequencyApproximation::Exact,
            );

            self.osc_level[i] = 0.0;
            self.filter[i].init();

            self.spatializer[i].init(if i == 0 { -0.7 } else { 0.7 });
        }
    }

    fn gate_flags(&mut self, gate_in: bool) -> u8 {
        let mut flags = 0;
        if gate_in {
            if !self.previous_gate {
                flags |= FLAG_RISING_EDGE;
            }
            flags |= FLAG_GATE;
        } else if self.previous_gate {
            flags = FLAG_FALLING_EDGE;
        }
        self.previous_gate = gate_in;
        flags
    }

    #[inline]
    fn midi_to_frequency(midi_pitch: f32) -> f32 {
        let midi_pitch = if midi_pitch < -12.0 {
            -12.0
        } else {
            midi_pitch
        };
        let pitch = (midi_pitch * 256.0) as i32;
        let pitch = 32768 + clip16(pitch - 20480);
        LUT_MIDI_TO_F_HIGH[(pitch >> 8) as usize] * LUT_MIDI_TO_F_LOW[(pitch & 0xff) as usize]
    }

    fn configure_envelope(&mut self, patch: &Patch) {
        if patch.exciter_envelope_shape < 0.4 {
            let a = 0.0;
            let dr = (patch.exciter_envelope_shape * 0.625 + 0.2) * 1.8;
            self.envelope.set_adsr(a, dr, 0.0, dr);
        } else if patch.exciter_envelope_shape < 0.6 {
            let s = (patch.exciter_envelope_shape - 0.4) * 5.0;
            self.envelope.set_adsr(0.0, 0.80, s, 0.80);
        } else {
            let a = 0.0;
            let dr = ((1.0 - patch.exciter_envelope_shape) * 0.75 + 0.15) * 1.8;
            self.envelope.set_adsr(a, dr, 1.0, dr);
        }
    }

    /// `Upsample<up>` -- linear-ramp interpolating upsampler.
    fn upsample(state: &mut f32, source: &[f32], destination: &mut [f32], source_size: usize) {
        let down = 1.0 / OVERSAMPLING_UP as f32;
        let mut s = *state;
        let mut w = 0;
        for i in 0..source_size {
            let increment = (source[i] - s) * down;
            for _ in 0..OVERSAMPLING_UP {
                destination[w] = s;
                w += 1;
                s += increment;
            }
        }
        *state = s;
    }

    /// `Process`.
    pub fn process(
        &mut self,
        patch: &Patch,
        frequency: f32,
        strength: f32,
        gate_in: bool,
        blow_in: &[f32],
        strike_in: &[f32],
        raw: &mut [f32],
        center: &mut [f32],
        sides: &mut [f32],
        size: usize,
    ) {
        let flags = self.gate_flags(gate_in);

        // Envelope.
        self.configure_envelope(patch);
        let mut env_level = self.envelope.process(flags);
        env_level += if strength >= 0.5 {
            2.0 * strength - 1.0
        } else {
            0.0
        };
        let level_increment = (env_level - self.level_state) / size as f32;

        self.damping += 0.1 * (patch.resonator_damping - self.damping);
        let filter_env_amount = if self.damping <= 0.9 {
            1.1 * self.damping
        } else {
            0.99
        };
        let vca_env_amount = 1.0 + self.damping * self.damping * self.damping * self.damping * 0.5;

        // Configure the filter.
        let mut cutoff_midi = 12.0;
        cutoff_midi += patch.resonator_brightness * 140.0;
        cutoff_midi += filter_env_amount * env_level * 120.0;
        cutoff_midi += 0.5 * (frequency - 64.0);

        let cutoff = Self::midi_to_frequency(cutoff_midi);
        let q_bump = patch.resonator_geometry - 0.6;
        let q = 1.72 - q_bump * q_bump * 2.0;
        let cutoff_2 = cutoff * (1.0 + patch.resonator_modulation_offset);

        self.filter[0].set_f_q(cutoff, q, FrequencyApproximation::Fast);
        self.filter[1].set_f_q(cutoff_2, q * 1.25, FrequencyApproximation::Fast);

        center[..size].fill(0.0);
        sides[..size].fill(0.0);
        raw[..size].fill(0.0);

        let rotation_speed = [1.0f32, 1.123456];
        self.feedback += 0.01 * (patch.exciter_bow_timbre - self.feedback);
        let frequency = frequency + OVERSAMPLING_DOWN_MIDI;

        for i in 0..NUM_OSCILLATORS {
            let src = if i == 0 { blow_in } else { strike_in };
            let mut fm_state = self.external_fm_state[i];
            Self::upsample(
                &mut fm_state,
                &src[..size],
                &mut self.external_fm_oversampled,
                size,
            );
            self.external_fm_state[i] = fm_state;

            let (detune, ratio, amount, level) = if i == 0 {
                (
                    0.0,
                    patch.exciter_blow_meta,
                    patch.exciter_blow_timbre,
                    patch.exciter_blow_level,
                )
            } else {
                (
                    interpolate(&LUT_DETUNE_QUANTIZER, patch.exciter_bow_level, 64.0),
                    patch.exciter_strike_meta,
                    patch.exciter_strike_timbre,
                    patch.exciter_strike_level,
                )
            };

            let os = size * OVERSAMPLING_UP;
            self.oscillator[i].process(
                frequency + detune,
                ratio,
                self.feedback * (0.25 + 0.15 * patch.exciter_signature),
                (2.0 - patch.exciter_signature * self.feedback) * amount,
                &self.external_fm_oversampled[..os],
                &mut self.osc_oversampled[..os],
                os,
            );

            self.iir_downsampler[i]
                .process_in_place(FilterMode::LowPass, &mut self.osc_oversampled[..os]);
            self.fir_downsampler[i].process(&self.osc_oversampled, &mut self.osc, os);

            // Copy to raw buffer, ramping the per-oscillator level.
            let mut level_state = self.osc_level[i];
            for j in 0..size {
                level_state += 0.01 * (level - level_state);
                self.osc[j] *= level_state;
                raw[j] += self.osc[j];
            }
            self.osc_level[i] = level_state;

            // Apply filter.
            self.filter[i]
                .process_multimode_in_place(&mut self.osc[..size], patch.resonator_geometry);

            // Apply VCA.
            let mut l = self.level_state;
            for j in 0..size {
                let mut gain = l * vca_env_amount;
                if gain >= 1.0 {
                    gain = 1.0;
                }
                self.osc[j] *= gain;
                l += level_increment;
            }

            // Spatialize.
            let f = patch.resonator_position * patch.resonator_position * 0.001;
            let distance = patch.resonator_position;

            self.spatializer[i].rotate(f * rotation_speed[i]);
            self.spatializer[i].set_distance(distance * (2.0 - distance));
            let osc_copy = self.osc;
            self.spatializer[i].process(&osc_copy[..size], center, sides, size);
        }

        self.level_state = env_level;
    }
}
