//! `plaits/dsp/fx/diffuser.h` -- a granular diffuser: 4 chained allpasses (the
//! last LFO-modulated), an LFO-modulated delay with a one-pole damping filter
//! in its feedback path, then 2 more allpasses.

use super::fx_engine::{FxBuffer, Tap, TAIL};

const SIZE: usize = 8192;

// `E::DelayLine<Memory, N>` offsets, computed by hand from
// `Reserve<126, Reserve<180, Reserve<269, Reserve<444, Reserve<1653,
// Reserve<2010, Reserve<3411>>>>>>>` (each tap's `base` is the previous tap's
// `base + length + 1`).
const AP1: Tap = Tap { base: 0, length: 126 };
const AP2: Tap = Tap { base: 127, length: 180 };
const AP3: Tap = Tap { base: 308, length: 269 };
const AP4: Tap = Tap { base: 578, length: 444 };
const DAPA: Tap = Tap { base: 1023, length: 1653 };
const DAPB: Tap = Tap { base: 2677, length: 2010 };
const DEL: Tap = Tap { base: 4688, length: 3411 };

pub struct Diffuser {
    engine: FxBuffer<SIZE>,
    lp_decay: f32,
}

impl Default for Diffuser {
    fn default() -> Self {
        Self {
            engine: FxBuffer::default(),
            lp_decay: 0.0,
        }
    }
}

impl Diffuser {
    pub fn init(&mut self) {
        self.engine.set_lfo_frequency(0, 0.3 / 48_000.0);
        self.lp_decay = 0.0;
    }

    pub fn reset(&mut self) {
        self.engine.clear();
    }

    pub fn process(&mut self, amount: f32, rt: f32, in_out: &mut [f32]) {
        const KAP: f32 = 0.625;
        const KLP: f32 = 0.75;
        let mut lp = self.lp_decay;

        for s in in_out.iter_mut() {
            let mut c = self.engine.start();
            c.read_value(*s, 1.0);
            c.read(AP1, TAIL, KAP);
            c.write_allpass(AP1, 0, -KAP);
            c.read(AP2, TAIL, KAP);
            c.write_allpass(AP2, 0, -KAP);
            c.read(AP3, TAIL, KAP);
            c.write_allpass(AP3, 0, -KAP);
            c.interpolate_lfo(AP4, 400.0, 0, 43.0, KAP);
            c.write_allpass(AP4, 0, -KAP);
            c.interpolate_lfo(DEL, 3070.0, 0, 340.0, rt);
            c.lp(&mut lp, KLP);
            c.read(DAPA, TAIL, -KAP);
            c.write_allpass(DAPA, 0, KAP);
            c.read(DAPB, TAIL, KAP);
            c.write_allpass(DAPB, 0, -KAP);
            c.write(DEL, 0, 2.0);
            let mut wet = 0.0;
            c.write_out(&mut wet, 0.0);
            *s += amount * (wet - *s);
        }
        self.lp_decay = lp;
    }
}
