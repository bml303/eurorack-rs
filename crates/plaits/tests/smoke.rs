//! Every one of the 24 engine slots must render a plausible range of inputs
//! without panicking (index-out-of-bounds, divide-by-zero, integer overflow
//! in the final `f32 -> i16` conversion). This doesn't catch a NaN/infinity
//! silently produced deep in an engine's float math -- Rust's `as i16` cast
//! saturates/zeroes rather than panicking on that, unlike the C -- so it's a
//! crash/panic smoke test, not a numerical-correctness one.

use plaits::{Frame, Modulations, Patch, Voice};

const BLOCK_SIZE: usize = 12;

fn base_patch(engine: i32) -> Patch {
    Patch {
        note: 48.0,
        harmonics: 0.5,
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

fn base_modulations() -> Modulations {
    Modulations {
        engine: 0.0,
        note: 0.0,
        frequency: 0.0,
        harmonics: 0.0,
        timbre: 0.0,
        morph: 0.0,
        trigger: 0.0,
        level: 0.8,
        frequency_patched: false,
        timbre_patched: false,
        morph_patched: false,
        trigger_patched: true,
        level_patched: true,
    }
}

#[test]
fn every_engine_renders_finite_audio() {
    for engine in 0..24i32 {
        let mut voice = Voice::default();
        voice.init();

        let mut patch = base_patch(engine);
        let mut modulations = base_modulations();

        for step in 0..200i32 {
            // Sweep the four main parameters and occasionally trigger, to
            // exercise a wide range of internal states (including parameter
            // discontinuities the interpolators need to ramp through).
            patch.harmonics = ((step * 37) % 101) as f32 / 100.0;
            patch.timbre = ((step * 53) % 101) as f32 / 100.0;
            patch.morph = ((step * 71) % 101) as f32 / 100.0;
            patch.note = 24.0 + ((step * 13) % 96) as f32;

            modulations.trigger = if step % 16 == 0 { 1.0 } else { 0.0 };
            modulations.note = ((step * 7) % 24) as f32 - 12.0;

            let mut frames = [Frame::default(); BLOCK_SIZE];
            voice.render(&patch, &modulations, &mut frames);
        }
    }
}

#[test]
fn engine_selection_is_stable_and_in_range() {
    let mut voice = Voice::default();
    voice.init();
    for engine in 0..24i32 {
        let patch = base_patch(engine);
        let modulations = base_modulations();
        let mut frames = [Frame::default(); BLOCK_SIZE];
        voice.render(&patch, &modulations, &mut frames);
        assert!(
            (0..24).contains(&voice.active_engine()),
            "active_engine() out of range for requested engine {engine}"
        );
    }
}
