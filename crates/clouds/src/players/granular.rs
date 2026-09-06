//! `clouds/dsp/granular_sample_player.h` -- schedules and overlap-adds a pool
//! of [`Grain`]s to make the granular cloud.

use stmlib::fdsp::{crossfade, one_pole, slope};
use stmlib::rsqrt::fast_rsqrt_carmack;
use stmlib::units::semitones_to_ratio;
use stmlib::Random;

use crate::audio_buffer::AudioBuffer;
use crate::dsp::interpolate;
use crate::frame::MAX_BLOCK_SIZE;
use crate::grain::{Grain, GrainQuality};
use crate::parameters::Parameters;
use crate::resources::{LUT_GRAIN_SIZE, LUT_SIN};

/// `kMaxNumGrains`.
const MAX_NUM_GRAINS: usize = 64;

/// `GranularSamplePlayer`.
pub struct GranularSamplePlayer {
    max_num_grains: i32,
    num_midfi_grains: i32,
    num_channels: i32,

    num_grains: f32,
    gain_normalization: f32,
    grain_size_hint: f32,
    grain_rate_phasor: f32,

    grains: [Grain; MAX_NUM_GRAINS],
    available_grains: [i32; MAX_NUM_GRAINS],
}

impl GranularSamplePlayer {
    pub fn new() -> Self {
        Self {
            max_num_grains: 0,
            num_midfi_grains: 0,
            num_channels: 1,
            num_grains: 0.0,
            gain_normalization: 1.0,
            grain_size_hint: 1024.0,
            grain_rate_phasor: 0.0,
            grains: [Grain::new(); MAX_NUM_GRAINS],
            available_grains: [0; MAX_NUM_GRAINS],
        }
    }

    /// `Init`.
    pub fn init(&mut self, num_channels: i32, max_num_grains: i32) {
        self.max_num_grains = max_num_grains;
        self.num_midfi_grains = 3 * max_num_grains / 4;
        self.gain_normalization = 1.0;
        for g in self.grains.iter_mut() {
            g.init();
        }
        self.num_grains = 0.0;
        self.num_channels = num_channels;
        self.grain_size_hint = 1024.0;
    }

    fn fill_available_grains_list(&mut self) -> i32 {
        let mut n = 0i32;
        for i in 0..self.max_num_grains {
            if !self.grains[i as usize].active() {
                self.available_grains[n as usize] = i;
                n += 1;
            }
        }
        n
    }

    /// `Play`.
    pub fn play(
        &mut self,
        buffer: &[AudioBuffer],
        parameters: &Parameters,
        out: &mut [f32],
        size: usize,
    ) {
        let mut overlap = parameters.granular.overlap;
        overlap = overlap * overlap * overlap;
        let target_num_grains = self.max_num_grains as f32 * overlap;
        let mut p = target_num_grains / self.grain_size_hint;
        let space_between_grains = self.grain_size_hint / target_num_grains;
        if parameters.granular.use_deterministic_seed {
            p = -1.0;
        } else {
            self.grain_rate_phasor = -1000.0;
        }

        let mut num_available_grains = self.fill_available_grains_list();

        let mut seed_trigger = parameters.trigger;
        for t in 0..size {
            self.grain_rate_phasor += 1.0;
            let seed_probabilistic =
                Random::get_float() < p && target_num_grains > self.num_grains;
            let seed_deterministic = self.grain_rate_phasor >= space_between_grains;
            let seed = seed_probabilistic || seed_deterministic || seed_trigger;
            if num_available_grains != 0 && seed {
                num_available_grains -= 1;
                let index = self.available_grains[num_available_grains as usize];
                let quality = if num_available_grains < self.num_midfi_grains {
                    GrainQuality::Medium
                } else {
                    GrainQuality::High
                };
                let buffer_size = buffer[0].size();
                let buffer_head = buffer[0].head() - size as i32 + t as i32;
                self.schedule_grain(
                    index as usize,
                    parameters,
                    t as i32,
                    buffer_size,
                    buffer_head,
                    quality,
                );
                self.grain_rate_phasor = 0.0;
                seed_trigger = false;
            }
        }

        for s in out[..size * 2].iter_mut() {
            *s = 0.0;
        }
        // `envelope_buffer_` in the C -- pure per-grain scratch, no state
        // carried between grains or blocks.
        let mut envelope = [0.0f32; MAX_BLOCK_SIZE];
        for i in 0..self.max_num_grains as usize {
            let quality = self.grains[i].recommended_quality();
            self.grains[i].overlap_add(
                buffer,
                out,
                &mut envelope,
                size,
                self.num_channels,
                quality,
            );
        }

        let active_grains = self.max_num_grains - num_available_grains;
        slope(&mut self.num_grains, active_grains as f32, 0.9, 0.2);

        let mut gain_normalization = if self.num_grains > 2.0 {
            fast_rsqrt_carmack(self.num_grains - 1.0)
        } else {
            1.0
        };
        let mut window_gain = 1.0 + 2.0 * parameters.granular.window_shape;
        window_gain = window_gain.clamp(1.0, 2.0);
        gain_normalization *= crossfade(1.0, window_gain, parameters.granular.overlap);

        let mut out_idx = 0usize;
        for _ in 0..size {
            one_pole(&mut self.gain_normalization, gain_normalization, 0.01);
            out[out_idx] *= self.gain_normalization;
            out[out_idx + 1] *= self.gain_normalization;
            out_idx += 2;
        }
    }

    fn schedule_grain(
        &mut self,
        index: usize,
        parameters: &Parameters,
        pre_delay: i32,
        buffer_size: i32,
        buffer_head: i32,
        quality: GrainQuality,
    ) {
        let position = parameters.position;
        let pitch = parameters.pitch;
        let window_shape = parameters.granular.window_shape;
        let mut grain_size = interpolate(&LUT_GRAIN_SIZE, parameters.size, 256.0);
        let pitch_ratio = semitones_to_ratio(pitch);
        let inv_pitch_ratio = semitones_to_ratio(-pitch);
        let pan = 0.5 + parameters.stereo_spread * (Random::get_float() - 0.5);
        let (gain_l, gain_r);
        if self.num_channels == 1 {
            gain_l = interpolate(&LUT_SIN, pan, 256.0);
            gain_r = interpolate(&LUT_SIN[256..], pan, 256.0);
        } else if pan < 0.5 {
            gain_l = 1.0;
            gain_r = 2.0 * pan;
        } else {
            gain_r = 1.0;
            gain_l = 2.0 * (1.0 - pan);
        }

        if pitch_ratio > 1.0 {
            grain_size = grain_size.min(buffer_size as f32 * 0.25 * inv_pitch_ratio);
        }

        let eaten_by_play_head = grain_size * pitch_ratio;
        let eaten_by_recording_head = grain_size;

        let mut available = 0.0f32;
        available += buffer_size as f32;
        available -= eaten_by_play_head;
        available -= eaten_by_recording_head;

        let width = (grain_size as i32) & !1;
        let start = buffer_head - (position * available + eaten_by_play_head) as i32;
        self.grains[index].start(
            pre_delay,
            buffer_size,
            start,
            width,
            (pitch_ratio * 65536.0) as u32 as i32,
            window_shape,
            gain_l,
            gain_r,
            quality,
        );
        one_pole(&mut self.grain_size_hint, grain_size, 0.1);
    }
}

impl Default for GranularSamplePlayer {
    fn default() -> Self {
        Self::new()
    }
}
