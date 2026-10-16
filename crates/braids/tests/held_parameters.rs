//! Every shape held at a grid of parameters and pitches, with and without
//! sync, must render without panicking. The workspace's dev profile keeps
//! overflow checks on for this crate, so this also finds C overflow (UB,
//! wrapping on the Cortex-M3) that the port has not made explicit with
//! `wrapping_*`: crates using this one may build with overflow checks.

use braids::{MacroOscillator, MacroOscillatorShape};

#[test]
fn every_shape_at_held_parameters() {
    let values: Vec<i16> = (0..=8).map(|i| (i * 4096).min(32767) as i16).collect();
    let mut sync = [0u8; 24];
    sync[5] = 1;
    sync[17] = 1;
    for shape in MacroOscillatorShape::ALL {
        for &p1 in &values {
            for &p2 in &values {
                for note in (0..=138).step_by(12) {
                    let mut osc = MacroOscillator::new();
                    osc.set_shape(shape);
                    osc.set_parameters(p1, p2);
                    osc.set_pitch((note << 7) as i16);
                    for block in 0..40 {
                        if block % 20 == 0 {
                            osc.strike();
                        }
                        let mut out = [0i16; 24];
                        let s = if block % 2 == 0 { sync } else { [0; 24] };
                        osc.render(&s, &mut out, 24);
                    }
                }
            }
        }
    }
}
