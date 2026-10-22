//! `Patch::fourth_macro`: Plaits Palette's per-engine macro, which only the
//! six-op FM engine implements. At 0.5 it is the stock sound; off centre
//! it detunes the modulators and darkens or drives the output. The other
//! engines ignore it.

use plaits::{Modulations, Patch, Voice};

const BLOCK_SIZE: usize = 12;
const SIX_OP_A: i32 = 2;
const VIRTUAL_ANALOG: i32 = 0;

/// `blocks` blocks of OUT from `engine` playing a held note with the
/// macro at `fourth_macro`
fn render(engine: i32, fourth_macro: f32, blocks: usize) -> Vec<f32> {
    let mut voice = Voice::new(BLOCK_SIZE);
    voice.init();
    let p = Patch { engine, fourth_macro, ..Default::default() };
    let m = Modulations { trigger: 1.0, trigger_patched: true, ..Default::default() };
    let mut out = Vec::new();
    for _ in 0..blocks {
        let (mut o, mut a) = ([0.0; BLOCK_SIZE], [0.0; BLOCK_SIZE]);
        voice.render(&p, &m, &mut o, &mut a);
        out.extend_from_slice(&o);
    }
    out
}

fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0f32, |peak, s| peak.max(s.abs()))
}

#[test]
fn the_centre_is_the_stock_sound() {
    let stock = render(SIX_OP_A, 0.5, 200);
    assert!(peak(&stock) > 0.01, "the six-op engine plays");
    let default = {
        let mut voice = Voice::new(BLOCK_SIZE);
        voice.init();
        let p = Patch { engine: SIX_OP_A, ..Default::default() };
        let m = Modulations { trigger: 1.0, trigger_patched: true, ..Default::default() };
        let mut out = Vec::new();
        for _ in 0..200 {
            let (mut o, mut a) = ([0.0; BLOCK_SIZE], [0.0; BLOCK_SIZE]);
            voice.render(&p, &m, &mut o, &mut a);
            out.extend_from_slice(&o);
        }
        out
    };
    assert_eq!(stock, default, "Patch::default() is the centre");
}

#[test]
fn below_the_centre_darkens_above_it_drives() {
    let stock = render(SIX_OP_A, 0.5, 200);
    let dark = render(SIX_OP_A, 0.0, 200);
    let driven = render(SIX_OP_A, 1.0, 200);
    assert_ne!(stock, dark);
    assert_ne!(stock, driven);
    // -- a one-pole at coefficient 0.08 takes the edge off: block-to-block
    // -- differences shrink
    let roughness = |s: &[f32]| s.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f32>();
    assert!(roughness(&dark) < roughness(&stock), "the low-pass smooths the output");
    // -- the drive pushes the waveform out towards the soft limit
    assert!(peak(&driven) >= peak(&stock) * 0.99, "the drive doesn't lose level");
}

#[test]
fn the_other_engines_ignore_it() {
    let stock = render(VIRTUAL_ANALOG, 0.5, 50);
    let dark = render(VIRTUAL_ANALOG, 0.0, 50);
    let driven = render(VIRTUAL_ANALOG, 1.0, 50);
    assert_eq!(stock, dark);
    assert_eq!(stock, driven);
}
