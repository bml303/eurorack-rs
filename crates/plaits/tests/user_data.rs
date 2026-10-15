//! The host's user data: a 6-op FM bank given to a slot plays instead of
//! its bundled bank (`Voice::set_user_fm_bank`), and a user terrain sounds
//! as the wave terrain engine's terrain 8 (`Voice::set_user_terrain`).

use plaits::resources::{SYX_BANK_0, SYX_BANK_1};
use plaits::{Modulations, Patch, Voice};

const BLOCK_SIZE: usize = 12;

fn patch(engine: i32, harmonics: f32) -> Patch {
    Patch {
        note: 48.0,
        harmonics,
        timbre: 0.5,
        morph: 0.5,
        frequency_modulation_amount: 0.0,
        timbre_modulation_amount: 0.0,
        morph_modulation_amount: 0.0,
        engine,
        decay: 0.5,
        lpg_colour: 0.5,
    }
}

/// `blocks` blocks of OUT from a voice playing `engine`, a note held from
/// the start; `setup` configures the voice first
fn render(engine: i32, harmonics: f32, blocks: usize, setup: impl FnOnce(&mut Voice)) -> Vec<f32> {
    let mut voice = Voice::new(BLOCK_SIZE);
    voice.init();
    setup(&mut voice);
    let p = patch(engine, harmonics);
    let m = Modulations { trigger: 1.0, trigger_patched: true, ..Default::default() };
    let mut out = Vec::new();
    for _ in 0..blocks {
        let (mut o, mut a) = ([0.0; BLOCK_SIZE], [0.0; BLOCK_SIZE]);
        voice.render(&p, &m, &mut o, &mut a);
        out.extend_from_slice(&o);
    }
    out
}

#[test]
fn a_user_bank_replaces_the_slots_bundled_one() {
    let bundled_0 = render(2, 0.3, 400, |_| {});
    let bundled_1 = render(3, 0.3, 400, |_| {});
    let user = render(2, 0.3, 400, |v| v.set_user_fm_bank(0, Some(&SYX_BANK_1)));
    assert_ne!(bundled_0, bundled_1, "two different banks");
    assert_eq!(user, bundled_1, "slot 0 with bank 1 plays as slot 1");
    // Cleared again: the bundled bank
    let cleared = render(2, 0.3, 400, |v| {
        v.set_user_fm_bank(0, Some(&SYX_BANK_1));
        v.set_user_fm_bank(0, None);
    });
    assert_eq!(cleared, bundled_0);
    // A slot past the third is ignored
    assert_eq!(render(2, 0.3, 400, |v| v.set_user_fm_bank(3, Some(&SYX_BANK_1))), bundled_0);
    let _ = SYX_BANK_0;
}

#[test]
fn a_user_terrain_sounds_as_terrain_8() {
    static FLAT: [u8; 64 * 64] = [0; 64 * 64];
    static RAMP: [u8; 64 * 64] = {
        let mut t = [0u8; 64 * 64];
        let mut i = 0;
        while i < t.len() {
            // A ramp across x, as signed bytes
            t[i] = ((i % 64) as i32 * 4 - 128) as i8 as u8;
            i += 1;
        }
        t
    };
    // With a user terrain there are 9: HARMONICS at the top plays the user's
    let energy = |x: &[f32]| x.iter().map(|v| v * v).sum::<f32>();
    let flat = render(5, 1.0, 400, |v| v.set_user_terrain(Some(&FLAT)));
    let ramp = render(5, 1.0, 400, |v| v.set_user_terrain(Some(&RAMP)));
    let bundled = render(5, 1.0, 400, |_| {});
    assert!(energy(&flat) < 1e-6, "a flat terrain is silent: {}", energy(&flat));
    assert!(energy(&ramp) > 1.0, "a ramp sounds: {}", energy(&ramp));
    assert!(energy(&bundled) > 1.0, "without one, the top is bundled terrain 7: {}", energy(&bundled));
}
