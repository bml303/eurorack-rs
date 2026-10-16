//! The host's user data: a 6-op FM bank given to a slot plays instead of
//! its bundled bank (`Voice::set_user_fm_bank`), and a user terrain sounds
//! as the wave terrain engine's terrain 8 (`Voice::set_user_terrain`).
//! A user chord table plays in the string machine, chiptune and chords
//! engines (`Voice::set_user_chords`).

use plaits::resources::{SYX_BANK_0, SYX_BANK_1};
use plaits::{Chord, Modulations, Patch, Voice};

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

/// The bundled chords as a user table
static BUNDLED_CHORDS: [Chord; 11] = [
    [0.00, 0.01, 11.99, 12.00],
    [0.00, 7.00, 7.01, 12.00],
    [0.00, 5.00, 7.00, 12.00],
    [0.00, 3.00, 7.00, 12.00],
    [0.00, 3.00, 7.00, 10.00],
    [0.00, 3.00, 10.00, 14.00],
    [0.00, 3.00, 10.00, 17.00],
    [0.00, 2.00, 9.00, 16.00],
    [0.00, 4.00, 11.00, 14.00],
    [0.00, 4.00, 7.00, 11.00],
    [0.00, 4.00, 7.00, 12.00],
];

/// Three chords none of the bundled ones has
static CLUSTERS: [Chord; 3] = [[0.0, 1.0, 2.0, 3.0], [0.0, 6.0, 13.0, 18.0], [0.0, 0.5, 5.5, 11.0]];

#[test]
fn a_user_chord_table_plays_in_the_three_chord_engines() {
    for engine in [6, 7, 14] {
        for harmonics in [0.0, 0.4, 1.0] {
            let bundled = render(engine, harmonics, 400, |_| {});
            let same = render(engine, harmonics, 400, |v| v.set_user_chords(Some(&BUNDLED_CHORDS)));
            assert_eq!(same, bundled, "engine {engine} at {harmonics}: the bundled chords as a table");
            let clusters = render(engine, harmonics, 400, |v| v.set_user_chords(Some(&CLUSTERS)));
            assert_ne!(clusters, bundled, "engine {engine} at {harmonics}: other chords");
            let cleared = render(engine, harmonics, 400, |v| {
                v.set_user_chords(Some(&CLUSTERS));
                v.set_user_chords(None);
            });
            assert_eq!(cleared, bundled, "engine {engine} at {harmonics}: cleared");
        }
    }
    // Other engines don't take it
    assert_eq!(render(8, 0.4, 400, |v| v.set_user_chords(Some(&CLUSTERS))), render(8, 0.4, 400, |_| {}));
}

#[test]
fn a_new_chord_table_takes_effect_at_the_reload() {
    // The chords engine plays for 100 blocks, then gets the clusters
    let play = |reload: bool| {
        let mut voice = Voice::new(BLOCK_SIZE);
        voice.init();
        let p = patch(14, 0.4);
        let m = Modulations { trigger: 1.0, trigger_patched: true, ..Default::default() };
        let mut out = Vec::new();
        for block in 0..400 {
            if block == 100 {
                voice.set_user_chords(Some(&CLUSTERS));
                if reload {
                    voice.reload_user_data();
                }
            }
            let (mut o, mut a) = ([0.0; BLOCK_SIZE], [0.0; BLOCK_SIZE]);
            voice.render(&p, &m, &mut o, &mut a);
            out.extend_from_slice(&o);
        }
        out
    };
    let bundled = render(14, 0.4, 400, |_| {});
    assert_eq!(play(false), bundled, "set alone: the playing engine keeps its chords");
    let reloaded = play(true);
    let at = 100 * BLOCK_SIZE;
    assert_eq!(reloaded[..at], bundled[..at]);
    assert_ne!(reloaded[at..], bundled[at..], "reloaded: the clusters");
}
