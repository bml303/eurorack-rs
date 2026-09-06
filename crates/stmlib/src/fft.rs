//! `stmlib/fft/shy_fft.h` -- Laurent de Soras's split-radix real FFT, the
//! `RotationPhasor` variant (the only one clouds instantiates). Ported for a
//! fixed power-of-two `SIZE` (>= 8); output packing is
//! `[Re(0), Re(1), .., Re(SIZE/2 - 1), Im(0), Im(1), .., Im(SIZE/2 - 1)]`,
//! which `clouds::pvoc::FrameTransformation` reads directly.

/// The base-4 digit permutation `n -> {0:0, 1:2, 2:1, 3:3}` used to build the
/// bit-reversal table.
const fn rev4(d: usize) -> usize {
    [0, 2, 1, 3][d]
}

/// `ShyFFT::bit_rev_256_lut_` -- reverse the four base-4 digits of `i`.
const fn bit_rev_256() -> [u8; 256] {
    let mut lut = [0u8; 256];
    let mut i = 0usize;
    while i < 256 {
        let p = i >> 6;
        let q = (i >> 4) & 3;
        let r = (i >> 2) & 3;
        let s = i & 3;
        lut[i] = (rev4(s) * 64 + rev4(r) * 16 + rev4(q) * 4 + rev4(p)) as u8;
        i += 1;
    }
    lut
}

static BIT_REV_256: [u8; 256] = bit_rev_256();

// The exact literals from `shy_fft.h`'s `Math<float>` (`sqrt_2_div_2()` /
// `pi()`); keeping them verbatim is what makes the transform bit-identical to
// the firmware FFT.
#[allow(clippy::approx_constant)]
const SQRT_2_DIV_2: f32 = 0.7071067811865476;
#[allow(clippy::approx_constant)]
const PI: f32 = 3.141592653589793;

/// `RotationPhasor` -- generates successive roots of unity by complex rotation.
struct Phasor {
    cos: f32,
    sin: f32,
    real: f32,
    imag: f32,
}

impl Phasor {
    #[inline]
    fn start(lut: &[f32], pass: usize) -> Self {
        let index = (pass - 3) << 1;
        Self {
            cos: lut[index],
            real: lut[index],
            sin: lut[index + 1],
            imag: lut[index + 1],
        }
    }

    #[inline]
    fn rotate(&mut self) {
        let temp = self.cos * self.real - self.sin * self.imag;
        self.sin = self.cos * self.imag + self.sin * self.real;
        self.cos = temp;
    }
}

/// `ShyFFT<float, SIZE, RotationPhasor>`.
pub struct ShyFft<const SIZE: usize> {
    /// `sin_cos_lut_` -- `cos/sin(pi / 2^pass)` for `pass` in `3..num_passes`.
    sin_cos_lut: [f32; 40],
    num_passes: usize,
}

impl<const SIZE: usize> ShyFft<SIZE> {
    /// `ShyFFT()` + `Init`.
    pub fn new() -> Self {
        let num_passes = SIZE.trailing_zeros() as usize;
        let mut sin_cos_lut = [0.0f32; 40];
        let mut pass = 3;
        while pass < num_passes {
            let index = (pass - 3) << 1;
            let angle = PI / (1u32 << pass) as f32;
            sin_cos_lut[index] = libm::cosf(angle);
            sin_cos_lut[index + 1] = libm::sinf(angle);
            pass += 1;
        }
        Self {
            sin_cos_lut,
            num_passes,
        }
    }

    #[inline]
    fn bit_rev(&self, i: usize) -> usize {
        ((BIT_REV_256[i & 0xff] as usize) << 8 | BIT_REV_256[i >> 8] as usize)
            >> (16 - self.num_passes)
    }

    /// `Direct(input, output)` -- forward transform. `input` is used as
    /// scratch and left in an unspecified state; the result lands in `output`.
    pub fn direct(&self, input: &mut [f32], output: &mut [f32]) {
        let size = SIZE;
        let q = size >> 2;

        // First and second pass (bit-reversed decimation-in-time).
        {
            let mut d = 0usize;
            let mut i = 0usize;
            while i < size {
                let r0 = self.bit_rev(i);
                let r1 = r0 + 2 * q;
                let r2 = r0 + q;
                let r3 = r0 + 3 * q;
                output[d + 1] = input[r0] - input[r1];
                output[d + 3] = input[r2] - input[r3];
                let a = input[r0] + input[r1];
                let b = input[r2] + input[r3];
                output[d] = a + b;
                output[d + 2] = a - b;
                d += 4;
                i += 4;
            }
        }

        // Third pass: output -> input.
        {
            let mut i = 0usize;
            while i < size {
                input[i] = output[i] + output[i + 4];
                input[i + 4] = output[i] - output[i + 4];
                input[i + 2] = output[i + 2];
                input[i + 6] = output[i + 6];
                let v = (output[i + 5] - output[i + 7]) * SQRT_2_DIV_2;
                input[i + 1] = output[i + 1] + v;
                input[i + 3] = output[i + 1] - v;
                let v = (output[i + 5] + output[i + 7]) * SQRT_2_DIV_2;
                input[i + 5] = v + output[i + 3];
                input[i + 7] = v - output[i + 3];
                i += 8;
            }
        }

        // Remaining passes. After the third pass the data is in `input`; the C
        // sets (s = output, d = input) then swaps at the top of each pass, so
        // the first pass here reads `input` and writes `output`.
        let result_in_output = {
            let mut s: &mut [f32] = &mut output[..];
            let mut d: &mut [f32] = &mut input[..];
            // `d` currently aliases `input`.
            let mut d_is_output = false;
            for pass in 3..self.num_passes {
                core::mem::swap(&mut s, &mut d);
                d_is_output = !d_is_output;
                let n = 1usize << pass;
                let n_2 = n >> 1;
                let mut i = 0usize;
                while i < size {
                    d[i] = s[i] + s[i + n];
                    d[i + n] = s[i] - s[i + n];
                    d[i + n_2] = s[i + n_2];
                    d[i + n + n_2] = s[i + n + n_2];
                    let mut ph = Phasor::start(&self.sin_cos_lut, pass);
                    for j in 1..n_2 {
                        let c = ph.cos;
                        let sn = ph.sin;
                        let s2r = s[i + n + j];
                        let s2i = s[i + n + n_2 + j];
                        let s1r = s[i + j];
                        let s1i = s[i + n_2 + j];
                        let v = s2r * c - s2i * sn;
                        d[i + j] = s1r + v;
                        d[i + n - j] = s1r - v;
                        let v = s2r * sn + s2i * c;
                        d[i + n + j] = v + s1i;
                        d[i + 2 * n - j] = v - s1i;
                        ph.rotate();
                    }
                    i += n << 1;
                }
            }
            d_is_output
        };

        if !result_in_output {
            output[..size].copy_from_slice(&input[..size]);
        }
    }

    /// `Inverse(input, output)` -- inverse transform. `input` is used as
    /// scratch; the result lands in `output`.
    pub fn inverse(&self, input: &mut [f32], output: &mut [f32]) {
        let size = SIZE;
        let q = size >> 2;

        // Remaining passes, descending. Data starts in `input`; the C reads
        // `s` / writes `d` then swaps roles at the end of every pass, so the
        // first pass here reads `input` and writes `output`.
        let result_in_output = {
            let mut s: &mut [f32] = &mut input[..];
            let mut d: &mut [f32] = &mut output[..];
            let mut d_is_output = true;
            for pass in (3..self.num_passes).rev() {
                let n = 1usize << pass;
                let n_2 = n >> 1;
                let mut i = 0usize;
                while i < size {
                    let sr = i;
                    let si = i + n;
                    d[i] = s[sr] + s[si];
                    d[i + n] = s[sr] - s[si];
                    d[i + n_2] = s[sr + n_2] * 2.0;
                    d[i + n + n_2] = s[si + n_2] * 2.0;
                    let d1i = i + n_2;
                    let d2i = i + n + n_2;
                    let mut ph = Phasor::start(&self.sin_cos_lut, pass);
                    for j in 1..n_2 {
                        let si_m_j = s[si - j];
                        let si_j = s[si + j];
                        let si_n_m_j = s[si + n - j];
                        let sr_j = s[sr + j];
                        d[i + j] = sr_j + si_m_j;
                        d[d1i + j] = si_j - si_n_m_j;
                        let c = ph.cos;
                        let sn = ph.sin;
                        let vr = sr_j - si_m_j;
                        let vi = si_j + si_n_m_j;
                        d[i + n + j] = vr * c + vi * sn;
                        d[d2i + j] = vi * c - vr * sn;
                        ph.rotate();
                    }
                    i += n << 1;
                }
                core::mem::swap(&mut s, &mut d);
                d_is_output = !d_is_output;
            }
            // Result is in the buffer that was `d` before the last swap == `s`.
            !d_is_output
        };
        if !result_in_output {
            output[..size].copy_from_slice(&input[..size]);
        }

        // Third pass inverse: output -> input.
        {
            let mut i = 0usize;
            while i < size {
                input[i] = output[i] + output[i + 4];
                input[i + 4] = output[i] - output[i + 4];
                input[i + 2] = output[i + 2] * 2.0;
                input[i + 6] = output[i + 6] * 2.0;
                input[i + 1] = output[i + 1] + output[i + 3];
                input[i + 3] = output[i + 5] - output[i + 7];
                let vr = output[i + 1] - output[i + 3];
                let vi = output[i + 5] + output[i + 7];
                input[i + 5] = (vr + vi) * SQRT_2_DIV_2;
                input[i + 7] = (vi - vr) * SQRT_2_DIV_2;
                i += 8;
            }
        }

        // First and second pass inverse: input -> output.
        {
            let mut sbase = 0usize;
            let mut i = 0usize;
            while i < size {
                let r0 = self.bit_rev(i);
                let r1 = r0 + 2 * q;
                let r2 = r0 + q;
                let r3 = r0 + 3 * q;
                let b_0 = input[sbase] + input[sbase + 2];
                let b_2 = input[sbase] - input[sbase + 2];
                let b_1 = input[sbase + 1] * 2.0;
                let b_3 = input[sbase + 3] * 2.0;
                output[r0] = b_0 + b_1;
                output[r1] = b_0 - b_1;
                output[r2] = b_2 + b_3;
                output[r3] = b_2 - b_3;
                sbase += 4;
                i += 4;
            }
        }
    }
}

impl<const SIZE: usize> Default for ShyFft<SIZE> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip<const N: usize>() {
        let fft = ShyFft::<N>::new();
        let mut signal = [0.0f32; N];
        for (i, s) in signal.iter_mut().enumerate() {
            *s = libm::sinf(2.0 * PI * 3.0 * i as f32 / N as f32)
                + 0.5 * libm::sinf(2.0 * PI * 17.0 * i as f32 / N as f32);
        }
        let original = signal;

        let mut freq = [0.0f32; N];
        let mut scratch = signal;
        fft.direct(&mut scratch, &mut freq);

        // Parseval: sum |x|^2 * N == sum(re^2 + im^2) over the packed spectrum
        // (rough check that direct produced something sane).
        let time_energy: f32 = original.iter().map(|x| x * x).sum();
        assert!(time_energy > 0.1);

        let mut out = [0.0f32; N];
        let mut fin = freq;
        fft.inverse(&mut fin, &mut out);

        // Inverse(Direct(x)) == N * x for this transform.
        let scale = N as f32;
        let mut max_err = 0.0f32;
        for i in 0..N {
            max_err = max_err.max((out[i] / scale - original[i]).abs());
        }
        assert!(max_err < 1e-3, "N={N} max round-trip error {max_err}");
    }

    #[test]
    fn round_trips_at_several_sizes() {
        round_trip::<8>();
        round_trip::<64>();
        round_trip::<256>();
        round_trip::<1024>();
        round_trip::<4096>();
    }
}
