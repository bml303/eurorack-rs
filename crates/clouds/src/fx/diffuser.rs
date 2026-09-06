//! `clouds/dsp/fx/diffuser.h` -- a stereo pair of 4-stage all-pass chains,
//! dry/wet blended by `amount`. Post-processing for the granular / stretch /
//! looping modes.

use crate::frame::FloatFrame;

use super::fx_engine::{bases, Context, Format32, FxEngine, TAIL};

const LENGTHS: [usize; 8] = [126, 180, 269, 444, 151, 205, 245, 405];
const BASES: [usize; 8] = bases(LENGTHS);
const KAP: f32 = 0.625;

/// `Diffuser`.
pub struct Diffuser {
    engine: FxEngine<Format32, 2048>,
    amount: f32,
}

impl Diffuser {
    pub fn new() -> Self {
        Self {
            engine: FxEngine::new(),
            amount: 0.0,
        }
    }

    /// `Init` -- the C hands over an external `float[2048]`; here the engine
    /// owns it. Kept for parity with the firmware's `Prepare` sequence.
    pub fn init(&mut self) {
        self.engine.clear();
    }

    /// `set_amount`.
    #[inline]
    pub fn set_amount(&mut self, amount: f32) {
        self.amount = amount;
    }

    #[inline]
    fn all_pass(c: &mut Context<'_, Format32, 2048>, line: usize) {
        c.read_line(BASES[line], LENGTHS[line], TAIL, KAP);
        c.write_all_pass(BASES[line], LENGTHS[line], 0, -KAP);
    }

    /// `Process`.
    pub fn process(&mut self, in_out: &mut [FloatFrame]) {
        for frame in in_out.iter_mut() {
            // One Start per frame -- both channels share the context (and the
            // write pointer), exactly as in the C.
            let mut c = self.engine.start();

            let mut wet = 0.0f32;
            c.read(frame.l);
            Self::all_pass(&mut c, 0);
            Self::all_pass(&mut c, 1);
            Self::all_pass(&mut c, 2);
            Self::all_pass(&mut c, 3);
            c.write_out_scaled(&mut wet, 0.0);
            frame.l += self.amount * (wet - frame.l);

            c.read(frame.r);
            Self::all_pass(&mut c, 4);
            Self::all_pass(&mut c, 5);
            Self::all_pass(&mut c, 6);
            Self::all_pass(&mut c, 7);
            c.write_out_scaled(&mut wet, 0.0);
            frame.r += self.amount * (wet - frame.r);
        }
    }
}

impl Default for Diffuser {
    fn default() -> Self {
        Self::new()
    }
}
