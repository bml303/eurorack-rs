//! Crash / sanity smoke test: `PatternGenerator` (both output modes, every
//! clock resolution, swing/gate mode on and off, a long tick sweep across
//! the whole 32-step pattern several times) and `Clock` (every BPM in its
//! table, every resolution, `wrap`/`raising_edge`/`past_falling_edge`) must
//! survive without panicking, and the pattern generator must produce a
//! varying, non-trivial rhythm (not always silent, not always the same
//! state).

use std::collections::HashSet;

use grids::pattern_generator::DrumsSettings;
use grids::{Clock, ClockResolution, Options, PatternGenerator, PatternGeneratorSettings};

#[test]
fn pattern_generator_survives_a_long_sweep_and_is_rhythmically_alive() {
    let mut pg = PatternGenerator::new();
    pg.init();

    let mut states: HashSet<u8> = HashSet::new();
    let mut any_hit = false;

    for step in 0..20_000i32 {
        if step % 97 == 0 {
            let options = Options {
                swing: step % 2 == 0,
                gate_mode: step % 3 == 0,
                output_clock: step % 5 == 0,
                tap_tempo: step % 7 == 0,
                ..Default::default()
            };
            pg.set_output_mode(if (step / 97) % 2 == 0 { 0 } else { 1 });
            pg.set_clock_resolution(((step / 97) % 4) as u8);
            pg.set_options(options);
        }
        if step % 131 == 0 {
            let mut settings = PatternGeneratorSettings::default();
            settings.options.drums = DrumsSettings {
                x: ((step * 3) % 256) as u8,
                y: ((step * 5) % 256) as u8,
                randomness: ((step * 7) % 256) as u8,
            };
            settings.options.euclidean_length = [
                ((step * 11) % 256) as u8,
                ((step * 13) % 256) as u8,
                ((step * 17) % 256) as u8,
            ];
            settings.density = [
                ((step * 19) % 256) as u8,
                ((step * 23) % 256) as u8,
                ((step * 29) % 256) as u8,
            ];
            *pg.mutable_settings() = settings;
        }
        if step % 211 == 0 {
            pg.retrigger();
        }

        pg.tick_clock(1);
        pg.increment_pulse_counter();
        if step % 17 == 0 {
            pg.clock_falling_edge();
        }

        let state = pg.state();
        states.insert(state);
        if state & 0x07 != 0 {
            any_hit = true;
        }

        // `led_pattern`/`on_beat`/`on_first_beat`/`swing_amount` must not
        // panic and should stay within their documented bit ranges.
        assert!(pg.led_pattern() <= 0x0e, "led_pattern out of the BD|SD|HH bit range");
        let _ = pg.on_beat();
        let _ = pg.on_first_beat();
        let _ = pg.swing_amount();
        assert!(pg.step() < grids::pattern_generator::STEPS_PER_PATTERN);
    }

    assert!(states.len() > 4, "pattern generator output looks static (only {} distinct states seen)", states.len());
    assert!(any_hit, "pattern generator never triggered a single instrument over 20000 ticks");
}

#[test]
fn options_pack_unpack_round_trips_through_every_byte_value() {
    for byte in 0u8..=255 {
        let mut options = Options::default();
        options.unpack(byte);
        // `pack`/`unpack` aren't bit-exact inverses of *every* byte (3 of
        // the 8 bits -- `clock_resolution`'s `0x07` nibble collapses any
        // value `>= 2` to `Ppqn24` -- see `clock_resolution_from_u8`), but
        // packing straight back must reproduce a byte that unpacks to the
        // *same* `Options` again (idempotent past the first unpack).
        let repacked = options.pack();
        let mut options2 = Options::default();
        options2.unpack(repacked);
        assert_eq!(format!("{options:?}"), format!("{options2:?}"), "unpack(pack(unpack({byte}))) != unpack({byte})");
    }
}

#[test]
fn clock_survives_every_bpm_and_resolution() {
    let mut clock = Clock::new();
    let resolutions = [ClockResolution::Ppqn4, ClockResolution::Ppqn8, ClockResolution::Ppqn24];

    for bpm in 0u16..512 {
        for &resolution in &resolutions {
            clock.update(bpm, resolution);
            clock.reset();
            for tick in 0..100 {
                clock.tick();
                let _ = clock.raising_edge();
                let _ = clock.past_falling_edge();
                if tick % 7 == 0 {
                    clock.wrap((tick % 5 - 2) as i8);
                }
            }
        }
    }

    clock.lock();
    assert!(clock.locked());
    clock.unlock();
    assert!(!clock.locked());
    assert_eq!(clock.bpm(), 511);

    // A basic sanity check on the resolution scaling: 24 PPQN should tick
    // roughly 3x faster than 8 PPQN for the same BPM (`phase_increment` is
    // tripled), which itself should be roughly 2x 4 PPQN.
    clock.update(120, ClockResolution::Ppqn8);
    clock.reset();
    let mut ticks_8 = 0u32;
    while !clock.raising_edge() || ticks_8 == 0 {
        clock.tick();
        ticks_8 += 1;
        if ticks_8 > 1_000_000 {
            break;
        }
    }
    clock.update(120, ClockResolution::Ppqn24);
    clock.reset();
    let mut ticks_24 = 0u32;
    while !clock.raising_edge() || ticks_24 == 0 {
        clock.tick();
        ticks_24 += 1;
        if ticks_24 > 1_000_000 {
            break;
        }
    }
    assert!(ticks_24 < ticks_8, "24 PPQN ({ticks_24} ticks/cycle) should wrap faster than 8 PPQN ({ticks_8})");
}
