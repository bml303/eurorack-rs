//! `streams/lorenz_generator.{h,cc}` -- integrates the Lorenz chaotic
//! attractor (fixed point, 8.24) at an EXCITE-modulated rate, mapping two
//! of its three axes to VCA/VCF amounts.

use crate::resources::LUT_LORENZ_RATE;

const SIGMA: i64 = 167_772_160; // 10.0 * (1 << 24), truncated.
const RHO: i64 = 469_762_048; // 28.0 * (1 << 24), truncated.
const BETA: i64 = 44_739_242; // 8.0 / 3.0 * (1 << 24), truncated.

#[derive(Debug, Clone, Copy, Default)]
pub struct LorenzGenerator {
    x: i32,
    y: i32,
    z: i32,
    rate: i32,
    vcf_amount: i32,
    vca_amount: i32,
    target_vcf_amount: i32,
    target_vca_amount: i32,

    index: u8,
}

impl LorenzGenerator {
    pub fn init(&mut self) {
        self.x = 1_677_721; // 0.1 * (1 << 24), truncated.
        self.y = 0;
        self.z = 0;
        self.vcf_amount = 0;
        self.vca_amount = 0;
    }

    pub fn set_index(&mut self, index: u8) {
        self.index = index;
    }

    pub fn configure(&mut self, _alternate: bool, parameters: &[i32; 2], _globals: Option<&[i32; 4]>) {
        self.rate = parameters[0] >> 8;
        let mut vcf_amount = 65535 - parameters[1];
        let mut vca_amount = parameters[1];
        if vcf_amount >= 32767 {
            vcf_amount = 32767;
        }
        if vca_amount >= 32767 {
            vca_amount = 32767;
        }
        self.target_vcf_amount = vcf_amount;
        self.target_vca_amount = vca_amount;
    }

    pub fn process(&mut self, _audio: i16, excite: i16, gain: &mut u16, frequency: &mut u16) {
        self.vcf_amount = self.vcf_amount.wrapping_add((self.target_vcf_amount - self.vcf_amount) >> 8);
        self.vca_amount = self.vca_amount.wrapping_add((self.target_vca_amount - self.vca_amount) >> 8);
        let rate = (self.rate + ((excite as i32) >> 8)).clamp(0, 256);
        let dt = LUT_LORENZ_RATE[rate as usize] as i64;

        let inner1 = SIGMA.wrapping_mul((self.y - self.x) as i64) >> 24;
        let outer1 = dt.wrapping_mul(inner1) >> 24;
        let x = ((self.x as i64).wrapping_add(outer1)) as i32;

        let rho_minus_z = RHO.wrapping_sub(self.z as i64);
        let inner2 = ((self.x as i64).wrapping_mul(rho_minus_z) >> 24).wrapping_sub(self.y as i64);
        let outer2 = dt.wrapping_mul(inner2) >> 24;
        let y = ((self.y as i64).wrapping_add(outer2)) as i32;

        let term_a = (self.x as i64).wrapping_mul(self.y as i64) >> 24;
        let term_b = BETA.wrapping_mul(self.z as i64) >> 24;
        let inner3 = term_a.wrapping_sub(term_b);
        let outer3 = dt.wrapping_mul(inner3) >> 24;
        let z = ((self.z as i64).wrapping_add(outer3)) as i32;

        self.x = x;
        self.y = y;
        self.z = z;

        let mut z_scaled = z >> 14;
        let mut x_scaled = (x >> 14).wrapping_add(32768);

        if self.index != 0 {
            // On channel 2, z and y are inverted to get more variety!
            core::mem::swap(&mut z_scaled, &mut x_scaled);
        }

        *gain = (z_scaled.wrapping_mul(self.vca_amount) >> 15) as u16;
        *frequency = 65535i32.wrapping_add((x_scaled - 65535).wrapping_mul(self.vcf_amount) >> 15) as u16;
    }
}
