//! `clouds/dsp/pvoc/stft.{h,cc}` -- short-time Fourier transform with
//! overlap-add. `Process` streams samples in/out of a circular analysis /
//! synthesis buffer; `Buffer` (driven from `GranularProcessor::prepare`)
//! does one hop's worth of FFT -> spectral modification -> IFFT -> overlap-add.

use alloc::boxed::Box;
use alloc::vec;

use stmlib::fdsp::clip16;
use stmlib::fft::ShyFft;

use crate::parameters::Parameters;
use crate::resources::LUT_SINE_WINDOW_4096;

use super::frame_transformation::FrameTransformation;

/// `kMaxFftSize`.
pub const MAX_FFT_SIZE: usize = 4096;
/// The FFT type clouds instantiates.
pub type Fft = ShyFft<MAX_FFT_SIZE>;

/// `STFT`.
pub struct Stft {
    fft_size: usize,
    hop_size: usize,
    buffer_size: usize,

    window_stride: usize,

    analysis: Box<[i16]>,
    synthesis: Box<[i16]>,

    buffer_ptr: usize,
    process_ptr: usize,
    block_size: usize,

    ready: usize,
    done: usize,

    parameters: Option<Parameters>,
}

impl Stft {
    pub fn new() -> Self {
        Self {
            fft_size: 0,
            hop_size: 0,
            buffer_size: 0,
            window_stride: 1,
            analysis: Box::from([]),
            synthesis: Box::from([]),
            buffer_ptr: 0,
            process_ptr: 0,
            block_size: 0,
            ready: 0,
            done: 0,
            parameters: None,
        }
    }

    /// `Init`.
    pub fn init(&mut self, fft_size: usize, hop_size: usize) {
        self.fft_size = fft_size;
        self.hop_size = hop_size;
        self.buffer_size = fft_size + hop_size;
        // window_stride_ = LUT_SINE_WINDOW_4096_SIZE / fft_size
        self.window_stride = LUT_SINE_WINDOW_4096.len() / fft_size;
        self.analysis = vec![0i16; self.buffer_size].into_boxed_slice();
        self.synthesis = vec![0i16; self.buffer_size].into_boxed_slice();
        self.parameters = None;
        self.reset();
    }

    /// `Reset`.
    pub fn reset(&mut self) {
        self.buffer_ptr = 0;
        self.process_ptr = (2 * self.hop_size) % self.buffer_size;
        self.block_size = 0;
        for a in self.analysis.iter_mut() {
            *a = 0;
        }
        for s in self.synthesis.iter_mut() {
            *s = 0;
        }
        self.ready = 0;
        self.done = 0;
    }

    /// `Process` -- stream `size` strided samples through the circular buffer.
    ///
    /// The C indexes `analysis_[buffer_ptr_ + i]` without wrapping inside the
    /// inner loop, relying on 32-sample blocks (`buffer_size` is a multiple of
    /// 32) so the sum never crosses the end. This wraps the index, which is
    /// identical for 32-sample blocks and simply stays correct otherwise.
    pub fn process(
        &mut self,
        parameters: &Parameters,
        input: &[f32],
        in_offset: usize,
        output: &mut [f32],
        out_offset: usize,
        size: usize,
        stride: usize,
    ) {
        self.parameters = Some(*parameters);
        let mut remaining = size;
        let mut in_idx = in_offset;
        let mut out_idx = out_offset;
        while remaining > 0 {
            let processed = remaining.min(self.hop_size - self.block_size);
            for i in 0..processed {
                let idx = (self.buffer_ptr + i) % self.buffer_size;
                let sample = (input[in_idx] * 32768.0) as i32;
                self.analysis[idx] = clip16(sample) as i16;
                output[out_idx] = self.synthesis[idx] as f32 / 16384.0;
                in_idx += stride;
                out_idx += stride;
            }
            self.block_size += processed;
            remaining -= processed;
            self.buffer_ptr += processed;
            if self.buffer_ptr >= self.buffer_size {
                self.buffer_ptr -= self.buffer_size;
            }
            if self.block_size >= self.hop_size {
                self.block_size -= self.hop_size;
                self.ready += 1;
            }
        }
    }

    /// `Buffer` -- process one pending hop. `fft_in` / `fft_out` are the
    /// shared scratch buffers (each `fft_size` long); the C aliases
    /// `fft_in == ifft_in` and `fft_out == ifft_out`.
    pub fn buffer(
        &mut self,
        fft: &Fft,
        modifier: &mut FrameTransformation,
        fft_in: &mut [f32],
        fft_out: &mut [f32],
    ) {
        if self.ready == self.done {
            return;
        }
        let parameters = match self.parameters {
            Some(p) => p,
            None => return,
        };

        // Windowed copy of the analysis block into the FFT input.
        let mut source_ptr = self.process_ptr;
        for i in 0..self.fft_size {
            fft_in[i] = LUT_SINE_WINDOW_4096[i * self.window_stride] * self.analysis[source_ptr] as f32;
            source_ptr += 1;
            if source_ptr >= self.buffer_size {
                source_ptr -= self.buffer_size;
            }
        }

        // Forward transform: result in `fft_out`, `fft_in` becomes scratch.
        fft.direct(fft_in, fft_out);

        // Spectral modification: reads `fft_out`, writes `fft_in` (= ifft_in).
        modifier.process(&parameters, fft_out, fft_in);

        // Inverse transform: reads `fft_in` (= ifft_in), result in `fft_out`.
        fft.inverse(fft_in, fft_out);

        // Overlap-add the windowed IFFT output back into the synthesis buffer.
        let inverse_window_size =
            1.0 / ((self.fft_size * self.fft_size / self.hop_size) >> 1) as f32;
        let mut destination_ptr = self.process_ptr;
        for i in 0..self.fft_size {
            let s = fft_out[i] * LUT_SINE_WINDOW_4096[i * self.window_stride] * inverse_window_size;
            let mut x = s as i32;
            if i < self.fft_size - self.hop_size {
                x += self.synthesis[destination_ptr] as i32;
            }
            self.synthesis[destination_ptr] = clip16(x) as i16;
            destination_ptr += 1;
            if destination_ptr >= self.buffer_size {
                destination_ptr -= self.buffer_size;
            }
        }

        self.done += 1;
        self.process_ptr += self.hop_size;
        if self.process_ptr >= self.buffer_size {
            self.process_ptr -= self.buffer_size;
        }
    }
}

impl Default for Stft {
    fn default() -> Self {
        Self::new()
    }
}
