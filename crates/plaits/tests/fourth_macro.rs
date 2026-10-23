//! `Patch::fourth_macro`: Plaits Palette's per-engine macro, which the
//! six-op FM, wavetable and wave terrain engines implement. At 0.5 it is
//! the stock sound; off centre the six-op engine detunes the modulators
//! and darkens or drives the output, the wavetable engine warps the
//! phase, the wave terrain engine moves the path down or up. The other
//! engines ignore it.

use plaits::{Modulations, Patch, Voice};

const BLOCK_SIZE: usize = 12;
const SIX_OP_A: i32 = 2;
const VIRTUAL_ANALOG: i32 = 0;
const WAVE_TERRAIN: i32 = 5;
const WAVETABLE: i32 = 13;

/// `blocks` blocks of OUT from `engine` playing a held note with the
/// macro at `fourth_macro`
fn render(engine: i32, fourth_macro: f32, blocks: usize) -> Vec<f32> {
    render_patch(Patch { engine, fourth_macro, ..Default::default() }, blocks)
}

fn render_patch(p: Patch, blocks: usize) -> Vec<f32> {
    let mut voice = Voice::new(BLOCK_SIZE);
    voice.init();
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

#[test]
fn the_wavetable_engine_warps_the_phase() {
    // -- mid-table, away from the quantized corners
    let patch = |fourth_macro| Patch {
        engine: WAVETABLE,
        fourth_macro,
        harmonics: 0.3,
        timbre: 0.4,
        morph: 0.6,
        ..Default::default()
    };
    let stock = render_patch(patch(0.5), 200);
    assert!(peak(&stock) > 0.01, "the wavetable engine plays");
    let default = Patch { engine: WAVETABLE, harmonics: 0.3, timbre: 0.4, morph: 0.6, ..Default::default() };
    assert_eq!(stock, render_patch(default, 200), "Patch::default() is the centre");
    for fourth_macro in [0.0, 1.0] {
        let warped = render_patch(patch(fourth_macro), 200);
        assert_ne!(stock, warped, "{fourth_macro} warps");
        // -- the slope division keeps the level in the stock's range
        let (p, s) = (peak(&warped), peak(&stock));
        assert!(p > s * 0.5 && p < s * 2.0, "{fourth_macro}: peak {p} vs {s}");
    }
    assert_ne!(render_patch(patch(0.0), 200), render_patch(patch(1.0), 200), "bipolar");
}

#[test]
fn the_wave_terrain_engine_moves_the_path() {
    let patch = |fourth_macro| Patch { engine: WAVE_TERRAIN, fourth_macro, timbre: 0.5, ..Default::default() };
    let stock = render_patch(patch(0.5), 200);
    assert!(peak(&stock) > 0.01, "the wave terrain engine plays");
    let default = render_patch(Patch { engine: WAVE_TERRAIN, timbre: 0.5, ..Default::default() }, 200);
    assert_eq!(stock, default, "Patch::default() is the centre");
    let down = render_patch(patch(0.0), 200);
    let up = render_patch(patch(1.0), 200);
    assert_ne!(stock, down);
    assert_ne!(stock, up);
    assert_ne!(down, up);
    assert!(down.iter().chain(&up).all(|s| s.is_finite() && s.abs() <= 1.5), "stays on the terrain");
}
