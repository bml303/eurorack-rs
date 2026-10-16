//! `UserWavetable`: custom waves in place of built-in ones and the wave
//! map's map, for the wavetable shapes (`MacroOscillator::set_user_wavetable`)

use braids::resources::{WT_MAP, WT_WAVES};
use braids::user_wavetable::{MAP_SIZE, MAX_CUSTOM_WAVES, WAVE_SIZE};
use braids::{MacroOscillator, MacroOscillatorShape, UserWavetable};
use std::sync::Mutex;
use stmlib::Random;

const SHAPES: [MacroOscillatorShape; 4] = [
    MacroOscillatorShape::Wavetables,
    MacroOscillatorShape::WaveMap,
    MacroOscillatorShape::WaveLine,
    MacroOscillatorShape::WaveParaphonic,
];

/// The global RNG (WTx4 seeds its phases from it) for one render at a time
static RNG: Mutex<()> = Mutex::new(());

/// `shape` over a sweep of both parameters, with `setup` done first
fn render(shape: MacroOscillatorShape, setup: impl Fn(&mut MacroOscillator)) -> Vec<i16> {
    render_with(shape, setup, |step| (((step * 55) & 0x7fff) as i16, ((step * 211) & 0x7fff) as i16))
}

/// `shape` with the parameters `parameters` gives each block
fn render_with(
    shape: MacroOscillatorShape,
    setup: impl Fn(&mut MacroOscillator),
    parameters: impl Fn(i32) -> (i16, i16),
) -> Vec<i16> {
    let _rng = RNG.lock().unwrap();
    Random::seed(0x21);
    let mut osc = MacroOscillator::new();
    setup(&mut osc);
    osc.set_shape(shape);
    let sync = [0u8; 24];
    let mut out = Vec::new();
    for step in 0..600i32 {
        let (p1, p2) = parameters(step);
        osc.set_parameters(p1, p2);
        osc.set_pitch(((48 << 7) + (step % 24) * 64) as i16);
        let mut block = [0i16; 24];
        osc.render(&sync, &mut block, 24);
        out.extend_from_slice(&block);
    }
    out
}

fn leak(table: UserWavetable) -> &'static UserWavetable {
    Box::leak(Box::new(table))
}

fn built_in(w: usize) -> [u8; WAVE_SIZE] {
    WT_WAVES[w * WAVE_SIZE..(w + 1) * WAVE_SIZE].try_into().unwrap()
}

/// A wave nothing like Braids': a square
fn square() -> [u8; WAVE_SIZE] {
    core::array::from_fn(|i| if i % 128 < 64 { 250 } else { 6 })
}

#[test]
fn the_built_in_table_plays_as_without_one() {
    let mut copies = UserWavetable::new();
    for w in [0, 17, 100, 255] {
        assert!(copies.set_wave(w, &built_in(w as usize)));
    }
    copies.set_map(&WT_MAP);
    let (plain, copies) = (leak(UserWavetable::new()), leak(copies));
    for shape in SHAPES {
        let reference = render(shape, |_| {});
        assert_eq!(render(shape, |o| o.set_user_wavetable(Some(plain))), reference, "{shape:?}");
        assert_eq!(render(shape, |o| o.set_user_wavetable(Some(copies))), reference, "{shape:?}: copies");
    }
}

#[test]
fn a_replaced_wave_plays_in_every_shape() {
    // Every wave the shapes reach replaced: all four sound different
    let mut squares = UserWavetable::new();
    let mut replaced = 0;
    for w in 0..=255u8 {
        replaced += squares.set_wave(w, &square()) as usize;
    }
    assert_eq!(replaced, MAX_CUSTOM_WAVES, "no more than room for");
    assert_eq!(squares.custom_waves(), MAX_CUSTOM_WAVES);
    // Replacing again takes no room
    assert!(squares.set_wave(3, &built_in(3)));
    assert!(!squares.set_wave(200, &square()));

    // The waves each shape plays from its first position on (WTBL's first
    // table, WLIN's and WTx4's first waves, the map's corner)
    let mut first = UserWavetable::new();
    for w in [0, 1, 187, 179, 157, 161, WT_MAP[0], WT_MAP[1], WT_MAP[16], WT_MAP[17]] {
        first.set_wave(w, &square());
    }
    let first = leak(first);
    for shape in SHAPES {
        let replaced = render_with(shape, |o| o.set_user_wavetable(Some(first)), |_| (0, 0));
        assert_ne!(replaced, render_with(shape, |_| {}, |_| (0, 0)), "{shape:?} plays the replaced waves");
    }
}

#[test]
fn the_map_is_the_wave_maps_alone() {
    let mut map = WT_MAP;
    map.reverse();
    let mut table = UserWavetable::new();
    table.set_map(&map);
    let table = leak(table);
    for shape in SHAPES {
        let other = render(shape, |o| o.set_user_wavetable(Some(table)));
        let reference = render(shape, |_| {});
        if shape == MacroOscillatorShape::WaveMap {
            assert_ne!(other, reference);
        } else {
            assert_eq!(other, reference, "{shape:?}");
        }
    }
}

#[test]
fn clearing_plays_the_built_in_waves_again() {
    let mut table = UserWavetable::new();
    table.set_wave(WT_MAP[0], &square());
    let table = leak(table);
    let shape = MacroOscillatorShape::WaveMap;
    let cleared = render(shape, |o| {
        o.set_user_wavetable(Some(table));
        o.set_user_wavetable(None);
    });
    assert_eq!(cleared, render(shape, |_| {}));
}

/// A file with `waves` at their slots, and `map`
fn file(waves: &[(u8, [u8; WAVE_SIZE])], map: Option<&[u8; MAP_SIZE]>) -> Vec<u8> {
    let mut f = b"BRWT".to_vec();
    f.extend([1, waves.len() as u8, map.is_some() as u8, 0]);
    f.extend(waves.iter().map(|(slot, _)| slot));
    if let Some(map) = map {
        f.extend(map);
    }
    for (_, wave) in waves {
        f.extend(wave);
    }
    f
}

#[test]
fn a_file_decodes() {
    let mut map = [7u8; MAP_SIZE];
    map[255] = 9;
    let mut expected = UserWavetable::new();
    expected.set_wave(9, &square());
    expected.set_wave(4, &built_in(5));
    let mut with_map = expected.clone();
    with_map.set_map(&map);

    let waves = [(9, square()), (4, built_in(5))];
    assert_eq!(UserWavetable::from_bytes(&file(&waves, None)), Some(expected));
    assert_eq!(UserWavetable::from_bytes(&file(&waves, Some(&map))), Some(with_map));
    assert_eq!(UserWavetable::from_bytes(&file(&[], None)), Some(UserWavetable::new()));

    let full: Vec<_> = (0..MAX_CUSTOM_WAVES as u8).map(|w| (w, square())).collect();
    let largest = file(&full, Some(&map));
    assert_eq!(largest.len(), UserWavetable::MAX_SIZE);
    assert!(largest.len() <= 4096);
    assert!(UserWavetable::from_bytes(&largest).is_some());
}

#[test]
fn a_file_that_is_not_one() {
    let waves = [(9, square()), (4, square())];
    let good = file(&waves, None);
    assert!(UserWavetable::from_bytes(&good[..good.len() - 1]).is_none(), "short");
    assert!(UserWavetable::from_bytes(&[good.clone(), vec![0]].concat()).is_none(), "long");
    assert!(UserWavetable::from_bytes(&good[..5]).is_none(), "a header only");
    assert!(UserWavetable::from_bytes(&file(&[(9, square()), (9, square())], None)).is_none(), "a slot twice");
    let too_many: Vec<_> = (0..=MAX_CUSTOM_WAVES as u8).map(|w| (w, square())).collect();
    assert!(UserWavetable::from_bytes(&file(&too_many, None)).is_none(), "too many waves");
    for (at, value) in [(0, b'X'), (4, 2), (6, 2), (6, 0x81)] {
        let mut bad = good.clone();
        bad[at] = value;
        assert!(UserWavetable::from_bytes(&bad).is_none(), "byte {at} = {value}");
    }
}
