//! `clouds/dsp/looping_sample_player.h` -- the looping delay / tape mode: a
//! Hermite-interpolated read head chasing a target delay, with a
//! cross-faded loop when frozen.

use stmlib::units::semitones_to_ratio;

use crate::audio_buffer::AudioBuffer;
use crate::parameters::Parameters;

/// `kCrossfadeDuration`.
const CROSSFADE_DURATION: f32 = 64.0;

/// `LoopingSamplePlayer`.
pub struct LoopingSamplePlayer {
    phase: f32,
    current_delay: f32,

    loop_point: f32,
    loop_duration: f32,
    tail_start: f32,
    tail_duration: f32,
    loop_reset: f32,

    synchronized: bool,

    num_channels: i32,
    tap_delay: i32,
    tap_delay_counter: i32,
}

impl LoopingSamplePlayer {
    pub fn new() -> Self {
        Self {
            phase: 0.0,
            current_delay: 0.0,
            loop_point: 0.0,
            loop_duration: 0.0,
            tail_start: 0.0,
            tail_duration: 1.0,
            loop_reset: 0.0,
            synchronized: false,
            num_channels: 1,
            tap_delay: 0,
            tap_delay_counter: 0,
        }
    }

    /// `Init`.
    pub fn init(&mut self, num_channels: i32) {
        self.num_channels = num_channels;
        self.phase = 0.0;
        self.current_delay = 0.0;
        self.loop_point = 0.0;
        self.loop_duration = 0.0;
        self.tap_delay = 0;
        self.tap_delay_counter = 0;
        self.synchronized = false;
        self.tail_duration = 1.0;
    }

    #[inline]
    pub fn synchronized(&self) -> bool {
        self.synchronized
    }

    /// `Play`.
    pub fn play(
        &mut self,
        buffer: &[AudioBuffer],
        parameters: &Parameters,
        out: &mut [f32],
        block_size: usize,
    ) {
        let max_delay = buffer[0].size() - CROSSFADE_DURATION as i32;
        self.tap_delay_counter += block_size as i32;
        if self.tap_delay_counter > max_delay {
            self.tap_delay = 0;
            self.tap_delay_counter = 0;
            self.synchronized = false;
        }
        if parameters.trigger {
            self.tap_delay = self.tap_delay_counter;
            self.tap_delay_counter = 0;
            self.synchronized = self.tap_delay > 128;
            self.loop_reset = self.phase;
            self.phase = 0.0;
        }

        let bufsize = buffer[0].size();

        if !parameters.freeze {
            // `size` counts down 31, 30, ..., 0 -- the samples still to come.
            for remaining in (0..block_size as i32).rev() {
                let mut target_delay = parameters.position * max_delay as f32;
                if self.synchronized {
                    target_delay = self.tap_delay as f32;
                }
                let error = target_delay - self.current_delay;
                let delay = self.current_delay + 0.00005 * error;
                self.current_delay = delay;
                let mut delay_int = (buffer[0].head() - 4 - remaining + bufsize) << 12;
                delay_int -= (delay * 4096.0) as i32;

                let out_idx = (block_size as i32 - 1 - remaining) as usize * 2;
                let integral = delay_int >> 12;
                let frac = ((delay_int << 4) & 0xffff) as u16;
                let l = buffer[0].read_hermite(integral, frac);
                if self.num_channels == 1 {
                    out[out_idx] = l;
                    out[out_idx + 1] = l;
                } else {
                    let r = buffer[1].read_hermite(integral, frac);
                    out[out_idx] = l;
                    out[out_idx + 1] = r;
                }
            }
            self.phase = 0.0;
        } else {
            let mut loop_point = parameters.position * max_delay as f32 * 15.0 / 16.0;
            loop_point += CROSSFADE_DURATION;
            let d = parameters.size;
            let mut loop_duration = (0.01 + 0.99 * d * d * d) * max_delay as f32;
            if self.synchronized {
                loop_duration = self.tap_delay as f32;
            }
            if loop_point + loop_duration >= max_delay as f32 {
                loop_point = max_delay as f32 - loop_duration;
            }
            let phase_increment = if self.synchronized {
                1.0
            } else {
                semitones_to_ratio(parameters.pitch)
            };

            let mut out_idx = 0usize;
            for _ in 0..block_size {
                if self.phase >= self.loop_duration || self.phase == 0.0 {
                    if self.phase >= self.loop_duration {
                        self.loop_reset = self.loop_duration;
                    }
                    if self.loop_reset >= self.loop_duration {
                        self.loop_reset = self.loop_duration;
                    }
                    self.tail_start = self.loop_duration - self.loop_reset + self.loop_point;
                    self.phase = 0.0;
                    self.tail_duration =
                        CROSSFADE_DURATION.min(CROSSFADE_DURATION * phase_increment);
                    self.loop_point = loop_point;
                    self.loop_duration = loop_duration;
                }
                self.phase += phase_increment;

                let mut gain = 1.0f32;
                if self.tail_duration != 0.0 {
                    gain = (self.phase / self.tail_duration).clamp(0.0, 1.0);
                }
                let delay_int = (buffer[0].head() - 4 + bufsize) << 12;
                let position =
                    delay_int - ((self.loop_duration - self.phase + self.loop_point) * 4096.0) as i32;
                let integral = position >> 12;
                let frac = ((position << 4) & 0xffff) as u16;
                let l = buffer[0].read_hermite(integral, frac);
                if self.num_channels == 1 {
                    out[out_idx] = l * gain;
                    out[out_idx + 1] = l * gain;
                } else {
                    let r = buffer[1].read_hermite(integral, frac);
                    out[out_idx] = l * gain;
                    out[out_idx + 1] = r * gain;
                }

                if gain != 1.0 {
                    let gain = 1.0 - gain;
                    let position =
                        delay_int - ((-self.phase + self.tail_start) * 4096.0) as i32;
                    let integral = position >> 12;
                    let frac = ((position << 4) & 0xffff) as u16;
                    let l = buffer[0].read_hermite(integral, frac);
                    if self.num_channels == 1 {
                        out[out_idx] += l * gain;
                        out[out_idx + 1] += l * gain;
                    } else {
                        let r = buffer[1].read_hermite(integral, frac);
                        out[out_idx] += l * gain;
                        out[out_idx + 1] += r * gain;
                    }
                }
                out_idx += 2;
            }
        }
    }
}

impl Default for LoopingSamplePlayer {
    fn default() -> Self {
        Self::new()
    }
}
