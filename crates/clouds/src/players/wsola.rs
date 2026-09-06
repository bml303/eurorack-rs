//! `clouds/dsp/wsola_sample_player.h` -- WSOLA (waveform-similarity
//! overlap-add) time-stretch / pitch-shift. Two [`Window`]s leap-frog through
//! the recording buffer; each new window is aligned to the previous audio by
//! the sign-bit [`Correlator`], which is owned by the processor and threaded
//! in as `&mut`.

use stmlib::constrain;
use stmlib::units::semitones_to_ratio;

use crate::audio_buffer::AudioBuffer;
use crate::correlator::Correlator;
use crate::parameters::Parameters;
use crate::window::Window;

/// `kMaxWSOLASize`.
const MAX_WSOLA_SIZE: i32 = 4096;

/// `WSOLASamplePlayer`.
pub struct WSOLASamplePlayer {
    windows: [Window; 2],

    window_size: i32,
    num_channels: i32,

    pitch: f32,
    smoothed_pitch: f32,
    position: f32,
    size_factor: f32,

    next_pitch_ratio: f32,
    correlator_loaded: bool,
    search_source: i32,
    search_target: i32,

    env_phase: f32,
    env_phase_increment: f32,
    elapsed: i32,
}

impl WSOLASamplePlayer {
    pub fn new() -> Self {
        Self {
            windows: [Window::new(); 2],
            window_size: MAX_WSOLA_SIZE / 2,
            num_channels: 1,
            pitch: 0.0,
            smoothed_pitch: 0.0,
            position: 0.0,
            size_factor: 0.0,
            next_pitch_ratio: 1.0,
            correlator_loaded: true,
            search_source: 0,
            search_target: 0,
            env_phase: 0.0,
            env_phase_increment: 0.5,
            elapsed: 0,
        }
    }

    /// `Init`.
    pub fn init(&mut self, num_channels: i32) {
        self.num_channels = num_channels;
        self.pitch = 0.0;
        self.position = 0.0;
        self.smoothed_pitch = 0.0;
        self.windows[0].init();
        self.windows[1].init();
        self.next_pitch_ratio = 1.0;
        self.correlator_loaded = true;
        self.search_source = 0;
        self.search_target = 0;
        self.window_size = MAX_WSOLA_SIZE / 2;
        self.env_phase = 0.0;
        self.env_phase_increment = 0.5;
        self.elapsed = 0;
    }

    /// `Play`.
    pub fn play(
        &mut self,
        correlator: &mut Correlator,
        buffer: &[AudioBuffer],
        parameters: &Parameters,
        out: &mut [f32],
        block_size: usize,
    ) {
        self.elapsed += 1;
        if parameters.trigger {
            self.env_phase = 0.0;
            self.env_phase_increment = 1.0 / self.elapsed as f32;
            self.env_phase_increment = constrain(self.env_phase_increment, 0.0001, 0.1);
            self.elapsed = 0;
        }
        self.env_phase += self.env_phase_increment;
        if self.env_phase >= 1.0 {
            self.env_phase = 1.0;
        }
        self.position = parameters.position;
        self.position += (1.0 - self.env_phase) * (1.0 - self.position);

        self.pitch = parameters.pitch;
        self.size_factor = parameters.size;

        if self.windows[0].done() && self.windows[1].done() {
            self.windows[1].mark_as_regenerated();
            self.schedule_aligned_window(correlator, buffer, 0);
        }

        let mut out_idx = 0usize;
        for _ in 0..block_size {
            out[out_idx] = 0.0;
            out[out_idx + 1] = 0.0;
            for i in 0..2 {
                self.windows[i].overlap_add(
                    buffer,
                    &mut out[out_idx..out_idx + 2],
                    self.num_channels,
                );
            }
            for i in 0..2 {
                if self.windows[i].needs_regeneration() {
                    self.windows[i].mark_as_regenerated();
                    self.schedule_aligned_window(correlator, buffer, 1 - i);
                    self.windows[1 - i].overlap_add(
                        buffer,
                        &mut out[out_idx..out_idx + 2],
                        self.num_channels,
                    );
                }
            }
            out_idx += 2;
        }
    }

    /// `ReadSignBits<num_channels>` -- pack the sign of `size` interpolated
    /// samples (starting at `source`, stepped by `phase_increment`) into
    /// `destination`, MSB-first, 32 per word. Returns the sample count.
    fn read_sign_bits(
        &self,
        buffer: &[AudioBuffer],
        phase_increment: i32,
        mut source: i32,
        size: i32,
        destination: &mut [u32],
    ) -> i32 {
        let mut phase: i32 = 0;
        let mut bits: u32 = 0;
        let mut bit_counter: u32 = 0;
        let mut num_samples: i32 = 0;
        if source < 0 {
            source += buffer[0].size();
        }
        while (phase >> 16) < size {
            let integral = source + (phase >> 16);
            let fractional = (phase & 0xffff) as u16;
            let mut s = buffer[0].read_linear(integral, fractional);
            if self.num_channels == 2 {
                s += buffer[1].read_linear(integral, fractional);
            }
            bits |= if s > 0.0 { 1 } else { 0 };
            if bit_counter & 0x1f == 0x1f {
                destination[(bit_counter >> 5) as usize] = bits;
                num_samples += 32;
            }
            bit_counter += 1;
            bits <<= 1;
            phase = phase.wrapping_add(phase_increment);
        }
        while bit_counter & 0x1f != 0 {
            if bit_counter & 0x1f == 0x1f {
                destination[(bit_counter >> 5) as usize] = bits;
                num_samples += 32;
            }
            bit_counter += 1;
            bits <<= 1;
        }
        num_samples
    }

    /// `LoadCorrelator` -- called from `GranularProcessor::prepare`.
    pub fn load_correlator(&mut self, correlator: &mut Correlator, buffer: &[AudioBuffer]) {
        if self.correlator_loaded {
            return;
        }
        let mut stride = self.window_size as f32 / 2048.0;
        stride = constrain(stride, 1.0, 2.0);
        stride *= 65536.0;
        let ratio = if self.next_pitch_ratio < 1.25 {
            1.25
        } else {
            self.next_pitch_ratio
        };
        let increment = (stride * ratio) as i32;

        let window_size = self.window_size;
        let search_source = self.search_source;
        let search_target = self.search_target;

        let num_samples = {
            let src = correlator.source_mut();
            self.read_sign_bits(buffer, increment, search_source, window_size, src)
        };
        {
            let dst = correlator.destination_mut();
            self.read_sign_bits(
                buffer,
                increment,
                search_target - window_size,
                window_size * 2,
                dst,
            );
        }
        correlator.start_search(
            num_samples,
            search_target - window_size + (window_size >> 1),
            increment,
        );
        self.correlator_loaded = true;
    }

    /// `ScheduleAlignedWindow` -- restart `windows[window_index]` at the
    /// correlator's best splice point and advance the search geometry.
    fn schedule_aligned_window(
        &mut self,
        correlator: &mut Correlator,
        buffer: &[AudioBuffer],
        window_index: usize,
    ) {
        let next_window_position = correlator.best_match();
        self.correlator_loaded = false;
        self.windows[window_index].start(
            buffer[0].size(),
            next_window_position - (self.window_size >> 1),
            self.window_size,
            (self.next_pitch_ratio * 65536.0) as u32 as i32,
        );

        let mut pitch_error = self.pitch - self.smoothed_pitch;
        let pitch_error_sign = if pitch_error < 0.0 { -1.0 } else { 1.0 };
        pitch_error *= pitch_error_sign;
        if pitch_error >= 12.0 {
            pitch_error = 12.0;
        }
        self.smoothed_pitch += pitch_error * pitch_error_sign;
        let pitch_ratio = semitones_to_ratio(self.smoothed_pitch);
        let inv_pitch_ratio = semitones_to_ratio(-self.smoothed_pitch);
        self.next_pitch_ratio = pitch_ratio;

        let size_factor = semitones_to_ratio((self.size_factor - 1.0) * 60.0);
        let mut new_window_size = (size_factor * MAX_WSOLA_SIZE as f32) as i32;
        if (new_window_size - self.window_size).abs() > 64 {
            let error = (new_window_size - self.window_size) >> 5;
            new_window_size = self.window_size + error;
            self.window_size = new_window_size - (new_window_size % 4);
        }

        let mut limit = buffer[0].size();
        limit -= (2.0 * self.window_size as f32 * inv_pitch_ratio) as i32;
        limit -= 2 * self.window_size;
        if limit < 0 {
            limit = 0;
        }

        let position = self.position;
        let mut target_position = buffer[0].head();
        target_position -= (limit as f32 * position) as i32;
        target_position -= self.window_size;

        self.search_source = next_window_position;
        self.search_target = target_position;
    }
}

impl Default for WSOLASamplePlayer {
    fn default() -> Self {
        Self::new()
    }
}
