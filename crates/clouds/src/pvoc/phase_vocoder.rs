//! `clouds/dsp/pvoc/phase_vocoder.{h,cc}` -- the spectral playback mode: one
//! [`Stft`] + [`FrameTransformation`] per channel, sharing a single FFT and
//! the FFT scratch buffers.

use alloc::boxed::Box;
use alloc::vec;

use crate::frame::FloatFrame;
use crate::parameters::Parameters;

use super::frame_transformation::{FrameTransformation, HIGH_FREQUENCY_TRUNCATION, MAX_NUM_TEXTURES};
use super::stft::{Fft, Stft, MAX_FFT_SIZE};

/// Firmware slab sizes (`clouds.cc` `block_mem` / `block_ccm`), used to
/// reproduce the `BufferAllocator` texture-count arithmetic.
const LARGE_BUFFER_SIZE: usize = 118784;
const SMALL_BUFFER_SIZE: usize = 65536 - 128;

const HOP_RATIO: usize = 4;

/// `PhaseVocoder`.
pub struct PhaseVocoder {
    fft: Fft,
    stft: [Stft; 2],
    frame_transformation: [FrameTransformation; 2],
    fft_buffer: Box<[f32]>,
    ifft_buffer: Box<[f32]>,
    num_channels: i32,
}

impl PhaseVocoder {
    pub fn new() -> Self {
        Self {
            fft: Fft::new(),
            stft: [Stft::new(), Stft::new()],
            frame_transformation: [FrameTransformation::new(), FrameTransformation::new()],
            fft_buffer: Box::from([]),
            ifft_buffer: Box::from([]),
            num_channels: 1,
        }
    }

    /// `Init` -- `num_channels` determines the buffer partitioning, hence the
    /// number of magnitude textures (7 mono, 3 stereo).
    pub fn init(&mut self, num_channels: i32) {
        self.num_channels = num_channels;
        let fft_size = MAX_FFT_SIZE;
        let hop_size = fft_size / HOP_RATIO;

        self.fft = Fft::new();
        self.fft_buffer = vec![0.0f32; fft_size].into_boxed_slice();
        self.ifft_buffer = vec![0.0f32; fft_size].into_boxed_slice();

        let num_textures = Self::num_textures(num_channels, fft_size);

        for i in 0..num_channels as usize {
            self.stft[i].init(fft_size, hop_size);
            self.frame_transformation[i].init(fft_size as i32, num_textures as i32);
        }
    }

    /// The `BufferAllocator` texture-count computation from `PhaseVocoder::Init`.
    fn num_textures(num_channels: i32, fft_size: usize) -> usize {
        let texture_size = (fft_size >> 1) - HIGH_FREQUENCY_TRUNCATION as usize;
        let fft_bytes = fft_size * 4;
        let ana_syn_bytes = (fft_size + (fft_size >> 1)) * 2 * 2;
        let mut num_textures = MAX_NUM_TEXTURES;
        for ch in 0..num_channels as usize {
            let bs = if ch == 0 {
                if num_channels == 1 {
                    LARGE_BUFFER_SIZE
                } else {
                    SMALL_BUFFER_SIZE
                }
            } else {
                SMALL_BUFFER_SIZE
            };
            let mut used = 0usize;
            if ch == 0 {
                used += fft_bytes; // fft_buffer, from allocator[0]
            }
            if ch == (num_channels - 1) as usize {
                used += fft_bytes; // ifft_buffer, from allocator[num_channels - 1]
            }
            used += ana_syn_bytes; // ana_syn[ch], from allocator[ch]
            let free = bs.saturating_sub(used);
            num_textures = num_textures.min(free / (4 * texture_size));
        }
        num_textures
    }

    /// `Buffer` -- run one pending FFT hop per channel (called from
    /// `GranularProcessor::prepare`).
    pub fn buffer(&mut self) {
        for i in 0..self.num_channels as usize {
            self.stft[i].buffer(
                &self.fft,
                &mut self.frame_transformation[i],
                &mut self.fft_buffer,
                &mut self.ifft_buffer,
            );
        }
    }

    /// `Process` -- stream a block through both channels' STFTs.
    pub fn process(
        &mut self,
        parameters: &Parameters,
        input: &[FloatFrame],
        output: &mut [FloatFrame],
        size: usize,
    ) {
        let mut in_flat = [0.0f32; crate::frame::MAX_BLOCK_SIZE * 2];
        let mut out_flat = [0.0f32; crate::frame::MAX_BLOCK_SIZE * 2];
        for i in 0..size {
            in_flat[2 * i] = input[i].l;
            in_flat[2 * i + 1] = input[i].r;
        }
        for ch in 0..self.num_channels as usize {
            self.stft[ch].process(parameters, &in_flat, ch, &mut out_flat, ch, size, 2);
        }
        for i in 0..size {
            output[i].l = out_flat[2 * i];
            output[i].r = out_flat[2 * i + 1];
        }
    }
}

impl Default for PhaseVocoder {
    fn default() -> Self {
        Self::new()
    }
}
