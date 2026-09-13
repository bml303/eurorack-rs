//! `tides/generator.{h,cc}` -- the Tides tidal modulator core: a variable-slope
//! oscillator/envelope with a slope/shape crossfade and an optional PLL sync to
//! an external clock, followed by a two-pole lowpass + wavefolder.
//!
//! `Generator::Process()` in the C dispatches to `ProcessWavetable` when the
//! `WAVETABLE_HACK` macro is defined -- it never is in the shipped firmware
//! (`// #define WAVETABLE_HACK` is commented out in `generator.h`), so that
//! path, the `mode_`-as-wavetable-bank-index reuse, and the unused `x_`/`y_`/
//! `z_`/`buffer_`/`prescaler_` fields it alone touches are dead code and are
//! not ported. The hardware interrupt double-buffer (`Process(control)` single
//! -sample ring buffer, `writable_block()`) is a latency-hiding wrapper around
//! `Process(size)`; this port exposes that block renderer directly as
//! [`Generator::render`].

use stmlib::pattern_predictor::PatternPredictor;
use stmlib::{constrain, crossfade_115, interpolate_1022};

use crate::resources::{LUT_CUTOFF, LUT_INCREMENTS, WAVEFORM_TABLE, WAV_BIPOLAR_FOLD, WAV_UNIPOLAR_FOLD};

const K_OCTAVE: i32 = 12 * 128;
const K_SYNC_COUNTER_MAX_TIME: u32 = 8 * 48000;

const WAV_INVERSE_TAN_AUDIO: u16 = 0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeneratorRange {
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeneratorMode {
    Ad,
    Looping,
    Ar,
}

pub const CONTROL_FREEZE: u8 = 1;
pub const CONTROL_GATE: u8 = 2;
pub const CONTROL_CLOCK: u8 = 4;
pub const CONTROL_CLOCK_RISING: u8 = 8;
pub const CONTROL_GATE_RISING: u8 = 16;
pub const CONTROL_GATE_FALLING: u8 = 32;

pub const FLAG_END_OF_ATTACK: u8 = 1;
pub const FLAG_END_OF_RELEASE: u8 = 2;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GeneratorSample {
    pub unipolar: u16,
    pub bipolar: i16,
    pub flags: u8,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct FrequencyRatio {
    pub p: u32,
    pub q: u32,
}

#[rustfmt::skip]
const FREQUENCY_RATIOS: [FrequencyRatio; 12] = [
    FrequencyRatio { p: 1, q: 1 },
    FrequencyRatio { p: 5, q: 4 },
    FrequencyRatio { p: 4, q: 3 },
    FrequencyRatio { p: 3, q: 2 },
    FrequencyRatio { p: 5, q: 3 },
    FrequencyRatio { p: 2, q: 1 },
    FrequencyRatio { p: 3, q: 1 },
    FrequencyRatio { p: 4, q: 1 },
    FrequencyRatio { p: 6, q: 1 },
    FrequencyRatio { p: 8, q: 1 },
    FrequencyRatio { p: 12, q: 1 },
    FrequencyRatio { p: 16, q: 1 },
];

/// `x >> n` reproducing the C shift on the reference (g++/x86) build: the PLL
/// tracking math computes shift counts derived from a phase error that, for
/// pathological (never normally reached) states, can exceed 31. That's
/// undefined behaviour in the C; x86 (and the firmware's own reference
/// toolchain) masks the count to its low 5 bits, so this matches that rather
/// than Rust's panic-on-overflow shift.
#[inline]
fn shr_i32_masked(value: i32, shift: i32) -> i32 {
    value >> ((shift as u32) & 31)
}

pub struct Generator {
    mode: GeneratorMode,
    range: GeneratorRange,
    previous_sample: GeneratorSample,

    clock_divider: u32,

    pitch: i16,
    previous_pitch: i16,
    shape: i16,
    slope: i16,
    smoothed_slope: i32,
    smoothness: i16,
    attenuation: i16,

    phase: u32,
    phase_increment: u32,
    wrap: bool,

    sync: bool,
    frequency_ratio: FrequencyRatio,

    sync_counter: u32,
    sync_edges_counter: u32,
    local_osc_phase: u32,
    local_osc_phase_increment: u32,
    target_phase_increment: u32,
    eor_counter: u32,

    pattern_predictor: PatternPredictor<32, 9>,

    uni_lp_state: [i64; 2],
    bi_lp_state: [i64; 2],

    running: bool,

    next_sample: i32,
    slope_up: bool,
    mid_point: u32,
}

impl Default for Generator {
    fn default() -> Self {
        Self::new()
    }
}

impl Generator {
    pub fn new() -> Self {
        let mut g = Generator {
            mode: GeneratorMode::Ad,
            range: GeneratorRange::High,
            previous_sample: GeneratorSample::default(),
            clock_divider: 1,
            pitch: 0,
            previous_pitch: 0,
            shape: 0,
            slope: 0,
            smoothed_slope: 0,
            smoothness: 0,
            attenuation: 0,
            phase: 0,
            phase_increment: 0,
            wrap: false,
            sync: false,
            frequency_ratio: FrequencyRatio::default(),
            sync_counter: 0,
            sync_edges_counter: 0,
            local_osc_phase: 0,
            local_osc_phase_increment: 0,
            target_phase_increment: 0,
            eor_counter: 0,
            pattern_predictor: PatternPredictor::new(),
            uni_lp_state: [0; 2],
            bi_lp_state: [0; 2],
            running: false,
            next_sample: 0,
            slope_up: false,
            mid_point: 0,
        };
        g.init();
        g
    }

    pub fn init(&mut self) {
        self.mode = GeneratorMode::Looping;
        self.range = GeneratorRange::High;
        self.clock_divider = 1;
        self.phase = 0;
        self.set_pitch(60 << 7);
        self.pattern_predictor.init();

        self.previous_sample = GeneratorSample::default();

        self.shape = 0;
        self.slope = 0;
        self.smoothed_slope = 0;
        self.smoothness = 0;

        self.running = false;

        self.clear_filter_state();

        self.sync_counter = K_SYNC_COUNTER_MAX_TIME;
        self.frequency_ratio = FrequencyRatio { p: 1, q: 1 };
        self.sync = false;
        self.phase_increment = 9_448_928;
        self.local_osc_phase_increment = self.phase_increment;
        self.target_phase_increment = self.phase_increment;
    }

    #[inline]
    pub fn set_range(&mut self, range: GeneratorRange) {
        self.clear_filter_state();
        self.range = range;
        self.clock_divider = if self.range == GeneratorRange::Low { 4 } else { 1 };
    }

    #[inline]
    pub fn set_mode(&mut self, mode: GeneratorMode) {
        self.mode = mode;
        if self.mode == GeneratorMode::Looping {
            self.running = true;
        }
    }

    #[inline]
    pub fn set_pitch(&mut self, pitch: i16) {
        if self.sync {
            self.compute_frequency_ratio(pitch);
        }
        let mut pitch = pitch.wrapping_add((12i16 << 7).wrapping_sub((60i16 << 7).wrapping_mul(self.range as i16)));
        if self.range == GeneratorRange::Low {
            pitch = pitch.wrapping_sub(12 << 7);
        }
        self.pitch = pitch;
    }

    #[inline]
    pub fn set_shape(&mut self, shape: i16) {
        self.shape = shape;
    }

    #[inline]
    pub fn set_slope(&mut self, slope: i16) {
        self.slope = slope;
    }

    #[inline]
    pub fn set_smoothness(&mut self, smoothness: i16) {
        self.smoothness = smoothness;
    }

    #[inline]
    pub fn set_frequency_ratio(&mut self, ratio: FrequencyRatio) {
        self.frequency_ratio = ratio;
    }

    #[inline]
    pub fn set_sync(&mut self, sync: bool) {
        if !self.sync && sync {
            self.pattern_predictor.init();
        }
        self.sync = sync;
        self.sync_edges_counter = 0;
    }

    #[inline]
    pub fn mode(&self) -> GeneratorMode {
        self.mode
    }

    #[inline]
    pub fn range(&self) -> GeneratorRange {
        self.range
    }

    #[inline]
    pub fn sync(&self) -> bool {
        self.sync
    }

    #[inline]
    pub fn clock_divider(&self) -> u32 {
        self.clock_divider
    }

    #[inline]
    fn clear_filter_state(&mut self) {
        self.uni_lp_state = [0; 2];
        self.bi_lp_state = [0; 2];
    }

    /// `Generator::Process(const uint8_t*, GeneratorSample*, size_t)` -- render
    /// one block. `control` and `out` must have the same length.
    pub fn render(&mut self, control: &[u8], out: &mut [GeneratorSample]) {
        assert_eq!(control.len(), out.len());
        if self.range == GeneratorRange::High {
            self.process_audio_rate(control, out);
        } else {
            self.process_control_rate(control, out);
        }
        self.process_filter_wavefolder(out);
    }

    fn compute_frequency_ratio(&mut self, pitch: i16) {
        let delta = self.previous_pitch.wrapping_sub(pitch);
        // Hysteresis for preventing glitchy transitions.
        if delta < 96 && delta > -96 {
            return;
        }
        self.previous_pitch = pitch;
        // Corresponds to a 0V CV after calibration. `pitch` stays an int16_t
        // through this subtraction in the C, narrowing before the next line's
        // multiply promotes it back to int -- matched here explicitly since,
        // unlike the pure add/sub chains elsewhere, the division right after
        // means truncating early vs. late isn't equivalent.
        let mut pitch = pitch.wrapping_sub(36 << 7);
        // The range of the control panel knob is 4 octaves.
        pitch = ((pitch as i32 * 12) / (48 << 7)) as i16;
        let mut swap = false;
        if pitch < 0 {
            pitch = -pitch;
            swap = true;
        }
        let mut idx = pitch;
        if idx as usize >= FREQUENCY_RATIOS.len() {
            idx = FREQUENCY_RATIOS.len() as i16 - 1;
        }
        let table_entry = FREQUENCY_RATIOS[idx as usize];
        self.frequency_ratio = table_entry;
        if swap {
            self.frequency_ratio.q = table_entry.p;
            self.frequency_ratio.p = table_entry.q;
        }
    }

    fn compute_phase_increment(&self, pitch: i16) -> u32 {
        let mut pitch = pitch as i32;
        let mut num_shifts: i32 = 0;
        while pitch < 0 {
            pitch += K_OCTAVE;
            num_shifts -= 1;
        }
        while pitch >= K_OCTAVE {
            pitch -= K_OCTAVE;
            num_shifts += 1;
        }
        let idx = (pitch >> 4) as usize;
        let a = LUT_INCREMENTS[idx];
        let b = LUT_INCREMENTS[idx + 1];
        let mut phase_increment =
            a.wrapping_add(((b.wrapping_sub(a) as i32).wrapping_mul(pitch & 0xf) >> 4) as u32);
        // Compensate for downsampling.
        phase_increment = phase_increment.wrapping_mul(self.clock_divider);
        if num_shifts >= 0 {
            phase_increment.wrapping_shl(num_shifts as u32)
        } else {
            phase_increment.wrapping_shr((-num_shifts) as u32)
        }
    }

    fn compute_pitch(&self, phase_increment: u32) -> i16 {
        let first = LUT_INCREMENTS[0];
        let last = LUT_INCREMENTS[LUT_INCREMENTS.len() - 2];
        let mut pitch: i32 = 0;

        let mut phase_increment = if phase_increment == 0 { 1 } else { phase_increment };
        phase_increment /= self.clock_divider;
        while phase_increment > last {
            phase_increment >>= 1;
            pitch += K_OCTAVE;
        }
        while phase_increment < first {
            phase_increment <<= 1;
            pitch -= K_OCTAVE;
        }
        let idx = LUT_INCREMENTS.partition_point(|&v| v < phase_increment);
        pitch += (idx as i32) << 4;
        pitch as i16
    }

    fn compute_cutoff_frequency(&self, pitch: i16, smoothness: i16) -> i32 {
        let mut shifts = self.clock_divider;
        let mut pitch = pitch as i32;
        while shifts > 1 {
            shifts >>= 1;
            pitch += K_OCTAVE;
        }
        let mut frequency: i32;
        if smoothness > 0 {
            frequency = 256 << 7;
        } else if smoothness > -16384 {
            let start = pitch + (36 << 7);
            let end = 256 << 7;
            frequency = start + (((end - start) * (smoothness as i32 + 16384)) >> 14);
        } else {
            let start = pitch - (36 << 7);
            let end = pitch + (36 << 7);
            frequency = start + (((end - start) * (smoothness as i32 + 32768)) >> 14);
        }
        frequency += 32768;
        if frequency < 0 {
            frequency = 0;
        }
        frequency
    }

    // `1 * (...)` and the two-step `< 0` / `> 32767` clamp below are literal
    // transcriptions of the C's coefficient table and its `CONSTRAIN`-style
    // (not panic-on-inverted-bounds) clamp -- kept so this function reads as
    // a direct line-for-line match against `Generator::ComputeAntialiasAttenuation`.
    #[allow(clippy::identity_op, clippy::manual_clamp)]
    fn compute_antialias_attenuation(&self, pitch: i16, slope: i16, shape: i16, smoothness: i16) -> i32 {
        let mut pitch = pitch as i32 + 12 * 128;
        if pitch < 0 {
            pitch = 0;
        }
        let slope = if slope < 0 { !slope } else { slope } as i32;
        let shape = if shape < 0 { !shape } else { shape } as i32;
        let smoothness = if smoothness < 0 { 0 } else { smoothness } as i32;

        let mut p: i32 = 252059;
        p += (-76 * smoothness) >> 5;
        p += (-30 * shape) >> 5;
        p += (-102 * slope) >> 5;
        p += (-664 * pitch) >> 5;
        p += (31 * ((smoothness * shape) >> 16)) >> 5;
        p += (12 * ((smoothness * slope) >> 16)) >> 5;
        p += (14 * ((shape * slope) >> 16)) >> 5;
        p += (219 * ((pitch * smoothness) >> 16)) >> 5;
        p += (50 * ((pitch * shape) >> 16)) >> 5;
        p += (425 * ((pitch * slope) >> 16)) >> 5;
        p += (13 * ((smoothness * smoothness) >> 16)) >> 5;
        p += (1 * ((shape * shape) >> 16)) >> 5;
        p += (-11 * ((slope * slope) >> 16)) >> 5;
        p += (776 * ((pitch * pitch) >> 16)) >> 5;
        if p < 0 {
            p = 0;
        }
        if p > 32767 {
            p = 32767;
        }
        p
    }

    #[inline]
    fn next_integrated_blep_sample(&self, t: u32) -> i32 {
        let t = t.min(65535);
        let t1 = (t >> 1) as i32;
        let t2 = t1.wrapping_mul(t1) >> 16;
        let t4 = t2.wrapping_mul(t2) >> 16;
        12288 - t1 + ((3 * t2) >> 1) - t4
    }

    #[inline]
    fn this_integrated_blep_sample(&self, t: u32) -> i32 {
        let t = t.min(65535);
        let t = 65535 - t;
        let t1 = (t >> 1) as i32;
        let t2 = t1.wrapping_mul(t1) >> 16;
        let t4 = t2.wrapping_mul(t2) >> 16;
        12288 - t1 + ((3 * t2) >> 1) - t4
    }

    fn process_filter_wavefolder(&mut self, out: &mut [GeneratorSample]) {
        let frequency = self.compute_cutoff_frequency(self.pitch, self.smoothness);
        // `frequency` saturates at exactly 65536 (`256 << 7` then `+= 32768`)
        // whenever `smoothness >= 0`, which makes the C's `lut_cutoff[(frequency
        // >> 7) + 1]` a one-past-the-end read (`frequency >> 7 == 512 ==
        // LUT_CUTOFF.len() - 1`) -- undefined behaviour there, but harmless
        // since the fractional part (`frequency & 0x7f`) is always 0 at that
        // saturation point, zeroing out `f_b`'s contribution below. Clamp the
        // index instead of reproducing the OOB read.
        let idx = (frequency >> 7) as usize;
        let f_a = (LUT_CUTOFF[idx.min(LUT_CUTOFF.len() - 1)] >> 16) as i32;
        let f_b = (LUT_CUTOFF[(idx + 1).min(LUT_CUTOFF.len() - 1)] >> 16) as i32;
        let f = f_a + (((f_b - f_a) * (frequency & 0x7f)) >> 7);
        let mut wf_gain: i32 = 2048;
        let mut wf_balance: i32 = 0;
        if self.smoothness > 0 {
            let attenuated_smoothness =
                ((self.smoothness as i32 * self.attenuation as i32) >> 15) as i16;
            wf_gain += (attenuated_smoothness as i32 * (32767 - 1024)) >> 14;
            wf_balance = attenuated_smoothness as i32;
        }

        let mut uni_lp_state_0 = self.uni_lp_state[0] as i32;
        let mut uni_lp_state_1 = self.uni_lp_state[1] as i32;
        let mut bi_lp_state_0 = self.bi_lp_state[0] as i32;
        let mut bi_lp_state_1 = self.bi_lp_state[1] as i32;

        for sample in out.iter_mut() {
            // Run through LPF.
            bi_lp_state_0 += (f * (sample.bipolar as i32 - bi_lp_state_0)) >> 15;
            bi_lp_state_1 += (f * (bi_lp_state_0 - bi_lp_state_1)) >> 15;

            // Fold.
            let original = bi_lp_state_1;
            let phase = (original.wrapping_mul(wf_gain) as u32).wrapping_add(1u32 << 31);
            let folded = interpolate_1022(&WAV_BIPOLAR_FOLD, phase) as i32;
            sample.bipolar = (original + (((folded - original) * wf_balance) >> 15)) as i16;

            // Run through LPF.
            uni_lp_state_0 += (f * (sample.unipolar as i32 - uni_lp_state_0)) >> 15;
            uni_lp_state_1 += (f * (uni_lp_state_0 - uni_lp_state_1)) >> 15;

            // Fold.
            let original = uni_lp_state_1 << 1;
            let phase = original.wrapping_mul(wf_gain) as u32;
            let folded = (interpolate_1022(&WAV_UNIPOLAR_FOLD, phase) as i32) << 1;
            sample.unipolar = (original + (((folded - original) * wf_balance) >> 15)) as u16;
        }

        self.uni_lp_state[0] = uni_lp_state_0 as i64;
        self.uni_lp_state[1] = uni_lp_state_1 as i64;
        self.bi_lp_state[0] = bi_lp_state_0 as i64;
        self.bi_lp_state[1] = bi_lp_state_1 as i64;
    }

    fn process_audio_rate(&mut self, input: &[u8], out: &mut [GeneratorSample]) {
        let mut sample = self.previous_sample;

        if self.sync {
            self.pitch = self.compute_pitch(self.phase_increment);
            self.pitch = constrain(self.pitch, 0, 120 << 7);
        } else {
            self.pitch = constrain(self.pitch, 0, 120 << 7);
            self.phase_increment = self.compute_phase_increment(self.pitch);
            self.local_osc_phase_increment = self.phase_increment;
            self.target_phase_increment = self.phase_increment;
        }

        self.attenuation = self.compute_antialias_attenuation(
            self.pitch,
            self.slope,
            self.shape,
            self.smoothness,
        ) as i16;

        let shape_val = (((self.shape as i32 * self.attenuation as i32) >> 15) + 32768) as u16;
        let wave_index = (WAV_INVERSE_TAN_AUDIO + (shape_val >> 14)) as usize;
        let shape_1 = WAVEFORM_TABLE[wave_index];
        let shape_2 = WAVEFORM_TABLE[wave_index + 1];
        let shape_xfade = shape_val << 2;

        let mut end_of_attack = (self.slope as i32 + 32768) as u32;
        end_of_attack <<= 16;

        // Load state into locals -- mirrors the C's register-caching comment.
        let mut phase = self.phase;
        let mut phase_increment = self.phase_increment;
        let mut wrap = self.wrap;

        // Enforce that the EOA pulse is at least 1 sample wide.
        if end_of_attack >= phase_increment {
            end_of_attack -= phase_increment;
        }
        if end_of_attack < phase_increment {
            end_of_attack = phase_increment;
        }

        let mut mid_point = self.mid_point;
        let mut next_sample = self.next_sample;

        for i in 0..input.len() {
            self.sync_counter = self.sync_counter.wrapping_add(1);
            let control = input[i];

            // When freeze is high, discard any start/reset command.
            if control & CONTROL_FREEZE == 0 {
                if control & CONTROL_GATE_RISING != 0 {
                    phase = 0;
                    self.running = true;
                } else if self.mode != GeneratorMode::Looping && wrap {
                    phase = 0;
                    self.running = false;
                }
            }

            if self.sync {
                if control & CONTROL_CLOCK_RISING != 0 {
                    self.sync_edges_counter = self.sync_edges_counter.wrapping_add(1);
                    if self.sync_edges_counter >= self.frequency_ratio.q {
                        self.sync_edges_counter = 0;
                        if self.sync_counter < K_SYNC_COUNTER_MAX_TIME && self.sync_counter != 0 {
                            let mut increment = (self.frequency_ratio.p as u64)
                                .wrapping_mul(0xffff_ffffu64 / self.sync_counter as u64);
                            if increment > 0x2000_0000 {
                                increment = 0x2000_0000;
                            }
                            self.target_phase_increment = increment as u32;
                            self.local_osc_phase = 0;
                        }
                        self.sync_counter = 0;
                    }
                }
                // Fast tracking of the local oscillator to the external oscillator.
                let diff = self.target_phase_increment.wrapping_sub(self.local_osc_phase_increment);
                self.local_osc_phase_increment = self
                    .local_osc_phase_increment
                    .wrapping_add((shr_i32_masked(diff as i32, 8)) as u32);
                self.local_osc_phase = self.local_osc_phase.wrapping_add(self.local_osc_phase_increment);

                // Slow phase realignment between the master oscillator and the
                // local oscillator.
                let phase_error = self.local_osc_phase.wrapping_sub(phase) as i32;
                phase_increment = self
                    .local_osc_phase_increment
                    .wrapping_add(shr_i32_masked(phase_error, 13) as u32);
            }

            if control & CONTROL_FREEZE != 0 {
                out[i] = sample;
                continue;
            }

            let sustained =
                self.mode == GeneratorMode::Ar && phase >= (1u32 << 31) && control & CONTROL_GATE != 0;

            if sustained {
                phase = 1u32 << 31;
            }

            mid_point = (mid_point >> 5).wrapping_mul(31);
            mid_point = mid_point.wrapping_add(end_of_attack >> 5);
            let min_mid_point = 2u32.wrapping_mul(phase_increment);
            let max_mid_point = 0xffff_ffffu32 - min_mid_point;
            mid_point = constrain(mid_point, min_mid_point, max_mid_point);
            mid_point = constrain(mid_point, 0x1_0000, 0xffff_0000);

            let slope_up = (0xffff_ffffu32 / (mid_point >> 16)) as i32;
            let slope_down = (0xffff_ffffu32 / (!mid_point >> 16)) as i32;

            let mut this_sample = next_sample;
            next_sample = 0;
            // Process reset discontinuity.
            //
            // `discontinuity * (phase_increment >> 18)` is `int32_t * uint32_t`
            // in the C: usual arithmetic conversions make the product -- and
            // therefore the `>> 14` right of it -- unsigned (a *logical* shift),
            // even though `discontinuity` is itself signed. Reproduce that by
            // multiplying/shifting in `u32` and only reinterpreting the bits as
            // `i32` once the result is stored back into `discontinuity`, rather
            // than casting to `i32` first (which would make the shift
            // sign-extend instead).
            if phase < phase_increment {
                self.slope_up = true;
                let t = phase / (phase_increment >> 16);
                let discontinuity_sum = slope_up.wrapping_add(slope_down) as u32;
                let discontinuity = (discontinuity_sum.wrapping_mul(phase_increment >> 18) >> 14) as i32;
                this_sample = this_sample
                    .wrapping_add(self.this_integrated_blep_sample(t).wrapping_mul(discontinuity) >> 16);
                next_sample = next_sample
                    .wrapping_add(self.next_integrated_blep_sample(t).wrapping_mul(discontinuity) >> 16);
            } else if self.slope_up ^ (phase < mid_point) {
                // Process transition discontinuity.
                self.slope_up = phase < mid_point;
                let t = phase.wrapping_sub(mid_point) / (phase_increment >> 16);
                let discontinuity_sum = slope_up.wrapping_add(slope_down) as u32;
                let discontinuity = (discontinuity_sum.wrapping_mul(phase_increment >> 18) >> 14) as i32;
                this_sample = this_sample
                    .wrapping_sub(self.this_integrated_blep_sample(t).wrapping_mul(discontinuity) >> 16);
                next_sample = next_sample
                    .wrapping_sub(self.next_integrated_blep_sample(t).wrapping_mul(discontinuity) >> 16);
            }

            // Same unsigned-shift subtlety as above: `(phase >> 16) * slope_up`
            // and `((phase - mid_point) >> 16) * slope_down` are `uint32_t *
            // int32_t` in the C, so their product -- and the final `>> 16` --
            // are unsigned/logical.
            let contribution: u32 = if self.slope_up {
                (phase >> 16).wrapping_mul(slope_up as u32) >> 16
            } else {
                65535u32.wrapping_sub(
                    (phase.wrapping_sub(mid_point) >> 16).wrapping_mul(slope_down as u32) >> 16,
                )
            };
            next_sample = next_sample.wrapping_add(contribution as i32);
            this_sample = constrain(this_sample, 0, 65535);

            sample.bipolar = crossfade_115(shape_1, shape_2, this_sample as u16, shape_xfade);
            sample.unipolar =
                crossfade_115(shape_1, shape_2, ((this_sample >> 1) + 32768) as u16, shape_xfade) as u16;
            sample.flags = 0;
            let looped = self.mode == GeneratorMode::Looping && wrap;
            if phase >= end_of_attack || !self.running {
                sample.flags |= FLAG_END_OF_ATTACK;
            }
            if !self.running || looped {
                self.eor_counter = if phase_increment < 44_739_242 { 48 } else { 1 };
            }
            if self.eor_counter != 0 {
                sample.flags |= FLAG_END_OF_RELEASE;
                self.eor_counter -= 1;
            }
            out[i] = sample;
            if self.running && !sustained {
                phase = phase.wrapping_add(phase_increment);
                wrap = phase < phase_increment;
            }
            if !self.running && !sustained {
                sample.bipolar = 0;
                sample.unipolar = 0;
            }
        }

        self.previous_sample = sample;
        self.phase = phase;
        self.phase_increment = phase_increment;
        self.wrap = wrap;
        self.next_sample = next_sample;
        self.mid_point = mid_point;
    }

    fn process_control_rate(&mut self, input: &[u8], out: &mut [GeneratorSample]) {
        if self.sync {
            self.pitch = self.compute_pitch(self.phase_increment);
        } else {
            self.phase_increment = self.compute_phase_increment(self.pitch);
            self.local_osc_phase_increment = self.phase_increment;
            self.target_phase_increment = self.phase_increment;
        }

        self.attenuation = 32767;

        let mut sample = self.previous_sample;

        let mut shape = (self.shape as i32 + 32768) as u16;
        shape = (shape >> 2).wrapping_mul(3);
        const WAV_REVERSED_CONTROL: u16 = 5;
        let wave_index = (WAV_REVERSED_CONTROL + (shape >> 13)) as usize;
        let shape_1 = WAVEFORM_TABLE[wave_index];
        let shape_2 = WAVEFORM_TABLE[wave_index + 1];
        let shape_xfade = shape << 3;

        let mut phase = self.phase;
        let mut phase_increment = self.phase_increment;
        let mut wrap = self.wrap;
        let mut smoothed_slope = self.smoothed_slope;
        let mut previous_smoothed_slope: i32 = 0x7fff_ffff;
        let mut end_of_attack: u32 = 1u32 << 31;
        const K_SLOPE_BITS: u32 = 12;
        let mut attack_factor: u32 = 1 << K_SLOPE_BITS;
        let mut decay_factor: u32 = 1 << K_SLOPE_BITS;

        for i in 0..input.len() {
            self.sync_counter = self.sync_counter.wrapping_add(1);
            // Low-pass filter the slope parameter.
            smoothed_slope += (self.slope as i32 - smoothed_slope) >> 4;

            let control = input[i];

            // When freeze is high, discard any start/reset command.
            if control & CONTROL_FREEZE == 0 {
                if control & CONTROL_GATE_RISING != 0 {
                    phase = 0;
                    self.running = true;
                } else if self.mode != GeneratorMode::Looping && wrap {
                    self.running = false;
                    phase = 0;
                }
            }

            if (control & CONTROL_CLOCK_RISING != 0) && self.sync && self.sync_counter != 0 {
                if self.sync_counter >= K_SYNC_COUNTER_MAX_TIME {
                    phase = 0;
                } else {
                    let predicted_period = if self.sync_counter < 480 {
                        self.sync_counter
                    } else {
                        self.pattern_predictor.predict(self.sync_counter as i32)
                    };
                    let mut increment = (self.frequency_ratio.p as u64).wrapping_mul(
                        0xffff_ffffu64 / (predicted_period as u64 * self.frequency_ratio.q as u64),
                    );
                    if increment > 0x2000_0000 {
                        increment = 0x2000_0000;
                    }
                    phase_increment = increment as u32;
                }
                self.sync_counter = 0;
            }

            if control & CONTROL_FREEZE != 0 {
                out[i] = sample;
                continue;
            }

            // Recompute the waveshaping parameters only when the slope has changed.
            if smoothed_slope != previous_smoothed_slope {
                let slope_offset =
                    stmlib::interpolate_88_u16(&crate::resources::LUT_SLOPE_COMPRESSION, (smoothed_slope + 32768) as u16)
                        as u32;
                if slope_offset <= 1 {
                    decay_factor = 32768 << K_SLOPE_BITS;
                    attack_factor = 1 << (K_SLOPE_BITS - 1);
                } else {
                    decay_factor = (32768u32 << K_SLOPE_BITS) / slope_offset;
                    attack_factor = (32768u32 << K_SLOPE_BITS) / (65536 - slope_offset);
                }
                previous_smoothed_slope = smoothed_slope;
                end_of_attack = slope_offset << 16;
            }

            let mut skewed_phase = if phase <= end_of_attack {
                (phase >> K_SLOPE_BITS).wrapping_mul(decay_factor)
            } else {
                ((phase - end_of_attack) >> K_SLOPE_BITS)
                    .wrapping_mul(attack_factor)
                    .wrapping_add(1u32 << 31)
            };

            let sustained =
                self.mode == GeneratorMode::Ar && phase >= end_of_attack && control & CONTROL_GATE != 0;

            if sustained {
                skewed_phase = 1u32 << 31;
                phase = end_of_attack + 1;
            }

            sample.unipolar = crossfade_115(shape_1, shape_2, (skewed_phase >> 16) as u16, shape_xfade) as u16;

            sample.bipolar = crossfade_115(shape_1, shape_2, (skewed_phase >> 15) as u16, shape_xfade);
            if skewed_phase >= (1u32 << 31) {
                sample.bipolar = -sample.bipolar;
            }

            let mut adjusted_end_of_attack = end_of_attack;
            if adjusted_end_of_attack >= phase_increment {
                adjusted_end_of_attack -= phase_increment;
            }
            if adjusted_end_of_attack < phase_increment {
                adjusted_end_of_attack = phase_increment;
            }

            sample.flags = 0;
            let looped = self.mode == GeneratorMode::Looping && wrap;
            if phase >= adjusted_end_of_attack || !self.running || sustained {
                sample.flags |= FLAG_END_OF_ATTACK;
            }
            if !self.running || looped {
                self.eor_counter = if phase_increment < 44_739_242 { 48 } else { 1 };
            }
            if self.eor_counter != 0 {
                sample.flags |= FLAG_END_OF_RELEASE;
                self.eor_counter -= 1;
            }
            // Two special cases for the "pure decay" scenario: END_OF_ATTACK is
            // always true except at the initial trigger.
            if end_of_attack == 0 {
                sample.flags |= FLAG_END_OF_ATTACK;
            }
            let triggered = control & CONTROL_GATE_RISING != 0;
            if (sustained || end_of_attack == 0) && (triggered || looped) {
                sample.flags &= !FLAG_END_OF_ATTACK;
            }

            out[i] = sample;
            if self.running && !sustained {
                phase = phase.wrapping_add(phase_increment);
                wrap = phase < phase_increment;
            } else {
                wrap = false;
            }
        }

        self.previous_sample = sample;
        self.phase = phase;
        self.phase_increment = phase_increment;
        self.wrap = wrap;
        self.smoothed_slope = smoothed_slope;
    }
}
