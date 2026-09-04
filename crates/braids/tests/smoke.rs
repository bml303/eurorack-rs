//! Every model must render a finite block for a plausible range of inputs
//! without panicking (index-out-of-bounds, divide-by-zero, overflow).

use braids::{DigitalModel, MacroOscillator, MacroOscillatorShape};

#[test]
fn every_macro_shape_renders() {
    let sync = [0u8; 24];
    for shape in MacroOscillatorShape::ALL {
        let mut osc = MacroOscillator::new();
        osc.set_shape(shape);
        for step in 0..400i32 {
            let p1 = ((step * 163) & 0x7fff) as i16;
            let p2 = ((step * 617) & 0x7fff) as i16;
            osc.set_parameters(p1, p2);
            osc.set_pitch(((24 << 7) + step * 37).min(140 << 7) as i16);
            if step % 64 == 0 {
                osc.strike();
            }
            let mut block = [0i16; 24];
            osc.render(&sync, &mut block, 24);
        }
    }
}

#[test]
fn digital_model_indices_match_macro_tail() {
    // macro shape N (>= TripleRingMod) must map to digital model N - 13.
    let base = MacroOscillatorShape::TripleRingMod as u8;
    assert_eq!(
        DigitalModel::from_u8(MacroOscillatorShape::QuestionMark as u8 - base),
        Some(DigitalModel::QuestionMark)
    );
    assert_eq!(
        DigitalModel::from_u8(MacroOscillatorShape::Plucked as u8 - base),
        Some(DigitalModel::Plucked)
    );
    assert_eq!(DigitalModel::COUNT, 35);
    assert_eq!(MacroOscillatorShape::COUNT, 48);
}

#[test]
fn sync_input_is_handled() {
    let mut osc = MacroOscillator::new();
    osc.set_shape(MacroOscillatorShape::SawSync);
    osc.set_pitch(50 << 7);
    osc.set_parameters(20000, 12000);
    let mut sync = [0u8; 24];
    for k in 0..24 {
        sync[k] = if k % 5 == 0 { 1 } else { 0 };
    }
    let mut block = [0i16; 24];
    for _ in 0..50 {
        osc.render(&sync, &mut block, 24);
    }
}
