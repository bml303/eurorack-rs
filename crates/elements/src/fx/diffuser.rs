//! `elements/dsp/fx/diffuser.h` -- a mono 4-stage all-pass chain applied to the
//! "blow" excitation before it enters the resonator, to smear transients.

use super::fx_engine::{Format32, FxEngine, TAIL, bases};

const LENGTHS: [usize; 4] = [126, 180, 269, 444];
const BASES: [usize; 4] = bases(LENGTHS);
const KAP: f32 = 0.625;

/// `elements::Diffuser`.
pub struct Diffuser {
    engine: FxEngine<Format32, 1024>,
}

impl Default for Diffuser {
    fn default() -> Self {
        Self::new()
    }
}

impl Diffuser {
    pub fn new() -> Self {
        Self {
            engine: FxEngine::new(),
        }
    }

    /// `Init` -- the C hands over an external `float[1024]`; here the engine owns
    /// it. Clears the buffer.
    pub fn init(&mut self) {
        self.engine.clear();
    }

    /// `Process` -- in place, mono.
    pub fn process(&mut self, in_out: &mut [f32]) {
        for s in in_out.iter_mut() {
            let mut c = self.engine.start();
            c.read(*s);
            for line in 0..4 {
                c.read_line(BASES[line], LENGTHS[line], TAIL, KAP);
                c.write_all_pass(BASES[line], LENGTHS[line], 0, -KAP);
            }
            c.write_out_scaled(s, 0.0);
        }
    }
}
