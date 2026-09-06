//! `clouds/dsp/window.h` -- the grain variant used by the WSOLA stretch
//! player: a triangular-enveloped Hermite read, overlap-added one sample at a
//! time, that signals when it is half-way through so the player can schedule
//! its replacement.

use crate::audio_buffer::AudioBuffer;

/// `Window`.
#[derive(Debug, Clone, Copy)]
pub struct Window {
    first_sample: i32,
    phase: i32,
    phase_increment: i32,
    envelope_phase_increment: f32,

    done: bool,
    half: bool,
    regenerated: bool,
}

impl Window {
    pub const fn new() -> Self {
        Self {
            first_sample: 0,
            phase: 0,
            phase_increment: 0,
            envelope_phase_increment: 0.0,
            done: true,
            half: false,
            regenerated: false,
        }
    }

    /// `Init`.
    pub fn init(&mut self) {
        self.done = true;
        self.regenerated = false;
        self.half = false;
    }

    /// `Start`.
    ///
    /// Restores `done_ = false`, which upstream Clouds lost in March 2023:
    /// `Start()` originally had the assignment *twice*, and two near-
    /// simultaneous "remove duplicate assignment" commits
    /// (`fbb53ba` + `0e3756f`, merged in `d1d8839`) between them deleted both
    /// copies. Without it a freshly `Start`ed window stays `done()` forever
    /// and Stretch mode is silent -- `clouds/test/clouds_test.cc` never
    /// exercises Stretch, so the regression went unnoticed. Reinstated here so
    /// the mode works; the C reference used for verification carries the same
    /// one-line fix (see `PORTING.md`).
    pub fn start(&mut self, buffer_size: i32, start: i32, width: i32, phase_increment: i32) {
        self.first_sample = (start + buffer_size).rem_euclid(buffer_size);
        self.phase_increment = phase_increment;
        self.phase = 0;
        self.done = false;
        self.regenerated = false;
        self.envelope_phase_increment = 2.0 / width as f32;
    }

    #[inline]
    pub fn done(&self) -> bool {
        self.done
    }

    #[inline]
    pub fn needs_regeneration(&self) -> bool {
        self.half && !self.regenerated
    }

    #[inline]
    pub fn mark_as_regenerated(&mut self) {
        self.regenerated = true;
    }

    /// `OverlapAdd` -- accumulate one sample into `samples` (`[l, r]`).
    pub fn overlap_add(&mut self, buffer: &[AudioBuffer], samples: &mut [f32], channels: i32) {
        if self.done {
            return;
        }
        let phase_integral = self.phase >> 16;
        let phase_fractional = (self.phase & 0xffff) as u16;
        let sample_index = self.first_sample + phase_integral;

        let envelope_phase = phase_integral as f32 * self.envelope_phase_increment;
        self.done = envelope_phase >= 2.0;
        self.half = envelope_phase >= 1.0;
        let gain = if envelope_phase >= 1.0 {
            2.0 - envelope_phase
        } else {
            envelope_phase
        };

        let l = buffer[0].read_hermite(sample_index, phase_fractional) * gain;
        if channels == 1 {
            samples[0] += l;
            samples[1] += l;
        } else {
            let r = buffer[1].read_hermite(sample_index, phase_fractional) * gain;
            samples[0] += l;
            samples[1] += r;
        }
        self.phase += self.phase_increment;
    }
}

impl Default for Window {
    fn default() -> Self {
        Self::new()
    }
}
