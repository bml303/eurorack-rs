//! `clouds/dsp/grain.h` -- one grain of the granular cloud: an enveloped,
//! pitch-shifted read from the recording buffer, overlap-added into the
//! output. The C templates on channel count / quality / resolution; here
//! those are runtime arguments.

use crate::audio_buffer::{AudioBuffer, InterpolationMethod};
use crate::dsp::interpolate;
use crate::resources::LUT_WINDOW;

/// `GrainQuality` -- also selects the buffer interpolation method.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum GrainQuality {
    /// ZOH reads.
    Low,
    /// Linear reads.
    Medium,
    /// Hermite reads.
    High,
}

impl GrainQuality {
    #[inline]
    fn interpolation(self) -> InterpolationMethod {
        match self {
            GrainQuality::Low => InterpolationMethod::Zoh,
            GrainQuality::Medium => InterpolationMethod::Linear,
            GrainQuality::High => InterpolationMethod::Hermite,
        }
    }
}

/// `Grain`.
#[derive(Debug, Clone, Copy)]
pub struct Grain {
    first_sample: i32,
    width: i32,
    phase: i32,
    phase_increment: i32,
    pre_delay: i32,

    envelope_smoothness: f32,
    envelope_slope: f32,
    envelope_phase: f32,
    envelope_phase_increment: f32,

    gain_l: f32,
    gain_r: f32,

    active: bool,
    recommended_quality: GrainQuality,
}

impl Grain {
    pub const fn new() -> Self {
        Self {
            first_sample: 0,
            width: 0,
            phase: 0,
            phase_increment: 0,
            pre_delay: 0,
            envelope_smoothness: 0.0,
            envelope_slope: 0.0,
            envelope_phase: 2.0,
            envelope_phase_increment: 0.0,
            gain_l: 0.0,
            gain_r: 0.0,
            active: false,
            recommended_quality: GrainQuality::Low,
        }
    }

    /// `Init`.
    pub fn init(&mut self) {
        self.active = false;
        self.envelope_phase = 2.0;
    }

    #[inline]
    pub fn active(&self) -> bool {
        self.active
    }

    #[inline]
    pub fn recommended_quality(&self) -> GrainQuality {
        self.recommended_quality
    }

    /// `Start`.
    pub fn start(
        &mut self,
        pre_delay: i32,
        buffer_size: i32,
        start: i32,
        width: i32,
        phase_increment: i32,
        window_shape: f32,
        gain_l: f32,
        gain_r: f32,
        recommended_quality: GrainQuality,
    ) {
        self.pre_delay = pre_delay;
        self.width = width;
        self.first_sample = (start + buffer_size).rem_euclid(buffer_size);
        self.phase_increment = phase_increment;
        self.phase = 0;
        self.envelope_phase = 0.0;
        self.envelope_phase_increment = 2.0 / width as f32;
        if window_shape >= 0.5 {
            self.envelope_smoothness = (window_shape - 0.5) * 2.0;
            self.envelope_slope = 0.0;
        } else {
            self.envelope_smoothness = 0.0;
            self.envelope_slope = 0.5 / (window_shape + 0.01);
        }
        self.active = true;
        self.gain_l = gain_l;
        self.gain_r = gain_r;
        self.recommended_quality = recommended_quality;
    }

    /// `RenderEnvelope` -- pre-compute the grain envelope into `envelope`.
    /// Writes `-1.0` at the sample where the grain expires (and stops there).
    fn render_envelope(&mut self, envelope: &mut [f32], size: usize, quality: GrainQuality) {
        let increment = self.envelope_phase_increment;
        let smoothness = self.envelope_smoothness;
        let slope = self.envelope_slope;
        let use_lut = smoothness != 0.0;

        let mut phase = self.envelope_phase;
        let mut i = 0usize;
        for _ in 0..size {
            let mut gain = phase;
            gain = if gain >= 1.0 { 2.0 - gain } else { gain };
            if use_lut {
                if quality == GrainQuality::High {
                    let window = interpolate(&LUT_WINDOW, gain, 4096.0);
                    gain += smoothness * (window - gain);
                }
            } else if quality >= GrainQuality::Medium {
                gain *= slope;
                if gain >= 1.0 {
                    gain = 1.0;
                }
            }
            phase += increment;
            if phase >= 2.0 {
                envelope[i] = -1.0;
                break;
            }
            envelope[i] = gain;
            i += 1;
        }
        self.envelope_phase = phase;
    }

    /// `OverlapAdd` -- accumulate the grain into `destination` (interleaved
    /// stereo, `2 * size` floats). `envelope` is a scratch buffer of at least
    /// `size` floats, reused between grains.
    pub fn overlap_add(
        &mut self,
        buffer: &[AudioBuffer],
        destination: &mut [f32],
        envelope: &mut [f32],
        mut size: usize,
        num_channels: i32,
        quality: GrainQuality,
    ) {
        if !self.active {
            return;
        }

        // The pre-delay lets a grain start partway through a block.
        let mut dst = 0usize;
        while self.pre_delay != 0 && size != 0 {
            dst += 2;
            size -= 1;
            self.pre_delay -= 1;
        }

        self.render_envelope(envelope, size, quality);

        let method = quality.interpolation();
        let phase_increment = self.phase_increment;
        let first_sample = self.first_sample;
        let gain_l = self.gain_l;
        let gain_r = self.gain_r;
        let mut phase = self.phase;
        let mut e = 0usize;
        for _ in 0..size {
            let sample_index = first_sample + (phase >> 16);
            let gain = envelope[e];
            e += 1;
            if gain == -1.0 {
                self.active = false;
                break;
            }
            let frac = (phase & 65535) as u16;
            let l = buffer[0].read(method, sample_index, frac) * gain;
            if num_channels == 1 {
                destination[dst] += l * gain_l;
                destination[dst + 1] += l * gain_r;
            } else {
                let r = buffer[1].read(method, sample_index, frac) * gain;
                destination[dst] += l * gain_l + r * (1.0 - gain_r);
                destination[dst + 1] += r * gain_r + l * (1.0 - gain_l);
            }
            dst += 2;
            phase += phase_increment;
        }
        self.phase = phase;
    }
}

impl Default for Grain {
    fn default() -> Self {
        Self::new()
    }
}
