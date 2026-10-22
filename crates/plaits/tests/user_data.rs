//! The host's user data: a 6-op FM bank given to a slot plays instead of
//! its bundled bank (`Voice::set_user_fm_bank`), and a user terrain sounds
//! as the wave terrain engine's terrain 8 (`Voice::set_user_terrain`).
//! A user chord table plays in the string machine, chiptune and chords
//! engines (`Voice::set_user_chords`), a user wavetable as the wavetable
//! engine's fourth bank (`Voice::set_user_wavetable`).

use plaits::resources::{SYX_BANK_0, SYX_BANK_1};
use plaits::engines::wavetable_engine::{NUM_CUSTOM_WAVES, WAVE_SIZE};
use plaits::resources::WAV_INTEGRATED_WAVES;
use plaits::{Chord, Modulations, Patch, UserWavetable, Voice};

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
        fourth_macro: 0.5,
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

/// A user wavetable from `map`, its custom waves copies of built-in waves
/// 0..15 (`custom(n)` = built-in `n`), leaked for the voice
fn wavetable(map: impl Fn(usize) -> u8) -> &'static UserWavetable {
    let mut waves = [0i16; NUM_CUSTOM_WAVES * WAVE_SIZE];
    waves.copy_from_slice(&WAV_INTEGRATED_WAVES[..NUM_CUSTOM_WAVES * WAVE_SIZE]);
    Box::leak(Box::new(UserWavetable { map: core::array::from_fn(map), waves }))
}

/// HARMONICS in the middle: the fourth bank alone (Z = 3.5 folds to 3)
const BANK_D: f32 = 0.5;

#[test]
fn a_user_wavetable_with_plaits_shuffle_plays_as_without_one() {
    // The C's fallback map: wave i of the bank is built-in (192 + i) * 101 % 192
    let shuffle = wavetable(|i| ((192 + i) * 101 % 192) as u8);
    let bundled = render(13, BANK_D, 400, |_| {});
    assert_eq!(render(13, BANK_D, 400, |v| v.set_user_wavetable(Some(shuffle))), bundled);
}

#[test]
fn custom_waves_play_where_the_map_puts_them() {
    // Custom wave n is a copy of built-in wave n: either map sounds the same
    let built_in = wavetable(|i| (i % NUM_CUSTOM_WAVES) as u8);
    let custom = wavetable(|i| (192 + i % NUM_CUSTOM_WAVES) as u8);
    let a = render(13, BANK_D, 400, |v| v.set_user_wavetable(Some(built_in)));
    let b = render(13, BANK_D, 400, |v| v.set_user_wavetable(Some(custom)));
    assert_eq!(a, b);
    assert_ne!(a, render(13, BANK_D, 400, |_| {}), "not the shuffle");
    // Past the custom waves: the last one
    let past = wavetable(|_| 250);
    let last = wavetable(|_| (192 + NUM_CUSTOM_WAVES - 1) as u8);
    assert_eq!(
        render(13, BANK_D, 400, |v| v.set_user_wavetable(Some(past))),
        render(13, BANK_D, 400, |v| v.set_user_wavetable(Some(last)))
    );
}

#[test]
fn a_user_wavetable_leaves_the_other_banks() {
    let custom = wavetable(|i| (192 + i % NUM_CUSTOM_WAVES) as u8);
    // HARMONICS at 0: bank A (higher values glide through D first)
    assert_eq!(
        render(13, 0.0, 400, |v| v.set_user_wavetable(Some(custom))),
        render(13, 0.0, 400, |_| {}),
        "bank A"
    );
    // Cleared: the shuffle again
    let cleared = render(13, BANK_D, 400, |v| {
        v.set_user_wavetable(Some(custom));
        v.set_user_wavetable(None);
    });
    assert_eq!(cleared, render(13, BANK_D, 400, |_| {}));
}

#[test]
fn a_user_wavetable_from_the_editors_block() {
    let mut block = vec![0u8; UserWavetable::BLOCK_SIZE];
    block[0] = 200;
    block[63] = 7;
    block[64..66].copy_from_slice(&(-2i16).to_le_bytes());
    let last = UserWavetable::DATA_SIZE - 2;
    block[last..last + 2].copy_from_slice(&1234i16.to_le_bytes());
    let t = UserWavetable::from_bytes(&block).unwrap();
    assert_eq!((t.map[0], t.map[63], t.waves[0], t.waves[t.waves.len() - 1]), (200, 7, -2, 1234));
    assert_eq!(UserWavetable::DATA_SIZE, 64 + 15 * 132 * 2);
    assert!(UserWavetable::from_bytes(&block[..UserWavetable::DATA_SIZE]).is_some(), "without the padding");
    assert!(UserWavetable::from_bytes(&block[..UserWavetable::DATA_SIZE - 1]).is_none());
}
