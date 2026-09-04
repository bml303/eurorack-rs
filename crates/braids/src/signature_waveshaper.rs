//! `braids/signature_waveshaper.h` -- a waveshaper whose transfer curve is
//! seeded from the MCU serial number, adding per-unit "impurities". Only used by
//! the top-level firmware, not the oscillators; ported for completeness.

use stmlib::fixed::mix_i16;

use crate::resources::WAV_SINE;

#[derive(Debug, Clone)]
pub struct SignatureWaveshaper {
    transfer: [i32; 257],
}

impl Default for SignatureWaveshaper {
    fn default() -> Self {
        let mut s = Self { transfer: [0; 257] };
        s.init(0);
        s
    }
}

impl SignatureWaveshaper {
    pub fn new(seed: u32) -> Self {
        let mut s = Self { transfer: [0; 257] };
        s.init(seed);
        s
    }

    pub fn init(&mut self, mut seed: u32) {
        let skew = (seed & 15) as i32;
        seed >>= 4;
        let sigmoid_strength = (seed & 31) as i32;
        seed >>= 5;
        let mut bumplets_frequency = (seed & 3) as i32;
        seed >>= 2;
        bumplets_frequency += 3;
        let mut bumplets_width = (seed & 7) as i32;
        bumplets_width += 1;
        bumplets_width <<= 7;
        bumplets_width = bumplets_width.wrapping_mul(bumplets_width);

        for i in 0..256i32 {
            let mut x = ((i - 128) << 8) as i16;
            let x_skew = (i * i - 32768) as i16;
            x = mix_i16(x, x_skew, (skew << 11) as u16);

            let sigmoid = ((x as i32).wrapping_mul(8192 + (sigmoid_strength << 10))
                / (8192 + (sigmoid_strength * (x as i32).abs() >> 5)))
                as i16;
            let bumplets = WAV_SINE[((i * bumplets_frequency) & 255) as usize];
            let mut bumplet_gain = ((x as i32).wrapping_mul(x as i32) / bumplets_width + 16) as u16;
            bumplet_gain = (32768u32 * 128 / (128 + bumplet_gain as u32)) as u16;
            self.transfer[i as usize] = mix_i16(sigmoid, bumplets, bumplet_gain) as i32;
        }
        self.transfer[256] = self.transfer[255];
    }

    #[inline]
    pub fn transfer_at(&self, i: u16) -> i32 {
        self.transfer[i as usize]
    }

    #[inline]
    pub fn transform(&self, sample: i16) -> i32 {
        let i = (sample as i32 + 32768) as u16;
        let a = self.transfer[(i >> 8) as usize];
        let b = self.transfer[((i >> 8) + 1) as usize];
        a + ((b - a) * (i & 0xff) as i32 >> 8)
    }
}
