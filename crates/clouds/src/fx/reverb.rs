//! `clouds/dsp/fx/reverb.h` -- the Griesinger/Dattorro reverb (4 input
//! all-passes, then a 2x 2AP+delay loop), 12-bit ring buffer, modulated taps.

use crate::frame::FloatFrame;

use super::fx_engine::{bases, Format12, FxEngine, TAIL};

const LENGTHS: [usize; 10] = [113, 162, 241, 399, 1653, 2038, 3411, 1913, 1663, 4782];
const BASES: [usize; 10] = bases(LENGTHS);

const AP1: usize = 0;
const AP2: usize = 1;
const AP3: usize = 2;
const AP4: usize = 3;
const DAP1A: usize = 4;
const DAP1B: usize = 5;
const DEL1: usize = 6;
const DAP2A: usize = 7;
const DAP2B: usize = 8;
const DEL2: usize = 9;

/// `Reverb`.
pub struct Reverb {
    engine: FxEngine<Format12, 16384>,
    amount: f32,
    input_gain: f32,
    reverb_time: f32,
    diffusion: f32,
    lp: f32,
    lp_decay_1: f32,
    lp_decay_2: f32,
}

impl Reverb {
    pub fn new() -> Self {
        Self {
            engine: FxEngine::new(),
            amount: 0.0,
            input_gain: 0.0,
            reverb_time: 0.0,
            diffusion: 0.625,
            lp: 0.7,
            lp_decay_1: 0.0,
            lp_decay_2: 0.0,
        }
    }

    /// `Init`.
    pub fn init(&mut self) {
        self.engine.clear();
        self.engine.set_lfo_frequency(0, 0.5 / 32000.0);
        self.engine.set_lfo_frequency(1, 0.3 / 32000.0);
        self.lp = 0.7;
        self.diffusion = 0.625;
    }

    #[inline]
    pub fn set_amount(&mut self, amount: f32) {
        self.amount = amount;
    }
    #[inline]
    pub fn set_input_gain(&mut self, input_gain: f32) {
        self.input_gain = input_gain;
    }
    #[inline]
    pub fn set_time(&mut self, reverb_time: f32) {
        self.reverb_time = reverb_time;
    }
    #[inline]
    pub fn set_diffusion(&mut self, diffusion: f32) {
        self.diffusion = diffusion;
    }
    #[inline]
    pub fn set_lp(&mut self, lp: f32) {
        self.lp = lp;
    }

    /// `Process`.
    pub fn process(&mut self, in_out: &mut [FloatFrame]) {
        let kap = self.diffusion;
        let klp = self.lp;
        let krt = self.reverb_time;
        let amount = self.amount;
        let gain = self.input_gain;

        let mut lp_1 = self.lp_decay_1;
        let mut lp_2 = self.lp_decay_2;

        for frame in in_out.iter_mut() {
            let mut wet = 0.0f32;
            let mut apout = 0.0f32;
            let mut c = self.engine.start();

            // Smear AP1 inside the loop.
            c.interpolate_lfo(BASES[AP1], 10.0, 0, 60.0, 1.0);
            c.write_line(BASES[AP1], LENGTHS[AP1], 100, 0.0);

            c.read_scaled(frame.l + frame.r, gain);

            // Diffuse through 4 all-passes.
            c.read_line(BASES[AP1], LENGTHS[AP1], TAIL, kap);
            c.write_all_pass(BASES[AP1], LENGTHS[AP1], 0, -kap);
            c.read_line(BASES[AP2], LENGTHS[AP2], TAIL, kap);
            c.write_all_pass(BASES[AP2], LENGTHS[AP2], 0, -kap);
            c.read_line(BASES[AP3], LENGTHS[AP3], TAIL, kap);
            c.write_all_pass(BASES[AP3], LENGTHS[AP3], 0, -kap);
            c.read_line(BASES[AP4], LENGTHS[AP4], TAIL, kap);
            c.write_all_pass(BASES[AP4], LENGTHS[AP4], 0, -kap);
            c.write_out(&mut apout);

            // Main reverb loop.
            c.load(apout);
            c.interpolate_lfo(BASES[DEL2], 4680.0, 1, 100.0, krt);
            c.lp(&mut lp_1, klp);
            c.read_line(BASES[DAP1A], LENGTHS[DAP1A], TAIL, -kap);
            c.write_all_pass(BASES[DAP1A], LENGTHS[DAP1A], 0, kap);
            c.read_line(BASES[DAP1B], LENGTHS[DAP1B], TAIL, kap);
            c.write_all_pass(BASES[DAP1B], LENGTHS[DAP1B], 0, -kap);
            c.write_line(BASES[DEL1], LENGTHS[DEL1], 0, 2.0);
            c.write_out_scaled(&mut wet, 0.0);

            frame.l += (wet - frame.l) * amount;

            c.load(apout);
            c.read_line(BASES[DEL1], LENGTHS[DEL1], TAIL, krt);
            c.lp(&mut lp_2, klp);
            c.read_line(BASES[DAP2A], LENGTHS[DAP2A], TAIL, kap);
            c.write_all_pass(BASES[DAP2A], LENGTHS[DAP2A], 0, -kap);
            c.read_line(BASES[DAP2B], LENGTHS[DAP2B], TAIL, -kap);
            c.write_all_pass(BASES[DAP2B], LENGTHS[DAP2B], 0, kap);
            c.write_line(BASES[DEL2], LENGTHS[DEL2], 0, 2.0);
            c.write_out_scaled(&mut wet, 0.0);

            frame.r += (wet - frame.r) * amount;
        }

        self.lp_decay_1 = lp_1;
        self.lp_decay_2 = lp_2;
    }
}

impl Default for Reverb {
    fn default() -> Self {
        Self::new()
    }
}
