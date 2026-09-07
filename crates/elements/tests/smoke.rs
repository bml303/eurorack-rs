//! Crash / sanity smoke test: every resonator model and the easter-egg voice
//! must survive a long parameter sweep (gate toggling, model switching, extreme
//! `space` / damping / geometry) without panicking -- no out-of-bounds table
//! reads, no NaN blow-up -- and must actually make sound.

use elements::{Part, PerformanceState, ResonatorModel};

const BLOCK: usize = 16;

fn sweep(model: ResonatorModel, easter_egg: bool) -> f64 {
    let mut part = Part::new();
    part.set_easter_egg(easter_egg);
    part.set_resonator_model(model);

    let notes = [45.0f32, 57.0, 69.0, 52.0, 60.0];
    let mut energy = 0.0f64;
    let mut count = 0u64;

    for step in 0..2400i32 {
        let t = step as f32 / 2400.0;
        {
            let p = part.patch_mut();
            p.exciter_envelope_shape = ((step * 7) % 101) as f32 / 100.0;
            p.exciter_bow_level = ((step * 11) % 101) as f32 / 100.0;
            p.exciter_bow_timbre = ((step * 13) % 101) as f32 / 100.0;
            p.exciter_blow_level = ((step * 17) % 131) as f32 / 100.0;
            p.exciter_blow_meta = ((step * 19) % 101) as f32 / 100.0;
            p.exciter_blow_timbre = ((step * 23) % 101) as f32 / 100.0;
            p.exciter_strike_level = ((step * 29) % 131) as f32 / 100.0;
            p.exciter_strike_meta = ((step * 31) % 101) as f32 / 100.0;
            p.exciter_strike_timbre = ((step * 37) % 101) as f32 / 100.0;
            p.exciter_signature = ((step * 5) % 101) as f32 / 100.0;
            p.resonator_geometry = ((step * 41) % 101) as f32 / 100.0;
            p.resonator_brightness = ((step * 43) % 101) as f32 / 100.0;
            p.resonator_damping = ((step * 3) % 101) as f32 / 100.0;
            p.resonator_position = ((step * 47) % 101) as f32 / 100.0;
            p.reverb_lp = 0.7;
            p.reverb_diffusion = 0.625;
            // Cover raw / spread / reverb / freeze regions of the "space" macro.
            p.space = (t * 2.0) * 1.9;
        }

        let perf = PerformanceState {
            gate: (step % 40) < 24,
            note: notes[(step as usize / 80) % notes.len()] - 12.0,
            modulation: 0.0,
            strength: 0.5,
        };

        let blow_in = [0.0f32; BLOCK];
        let strike_in = [0.0f32; BLOCK];
        let mut main = [0.0f32; BLOCK];
        let mut aux = [0.0f32; BLOCK];
        part.process(&perf, &blow_in, &strike_in, &mut main, &mut aux, BLOCK);

        for (m, a) in main.iter().zip(aux.iter()) {
            assert!(m.is_finite(), "main not finite at step {step}");
            assert!(a.is_finite(), "aux not finite at step {step}");
            energy += (*m as f64).powi(2) + (*a as f64).powi(2);
            count += 2;
        }
    }

    (energy / count as f64).sqrt()
}

#[test]
fn every_model_survives_a_sweep_and_makes_sound() {
    for model in [
        ResonatorModel::Modal,
        ResonatorModel::String,
        ResonatorModel::Strings,
    ] {
        let rms = sweep(model, false);
        assert!(rms > 1e-4, "{model:?} output too quiet: rms {rms}");
    }
}

#[test]
fn easter_egg_voice_survives_a_sweep_and_makes_sound() {
    let rms = sweep(ResonatorModel::Modal, true);
    assert!(rms > 1e-4, "ominous voice output too quiet: rms {rms}");
}

#[test]
fn bypass_passes_inputs_through() {
    let mut part = Part::new();
    part.set_bypass(true);

    let blow_in = [0.1f32; BLOCK];
    let strike_in = [-0.2f32; BLOCK];
    let mut main = [0.0f32; BLOCK];
    let mut aux = [0.0f32; BLOCK];
    let perf = PerformanceState::default();
    part.process(&perf, &blow_in, &strike_in, &mut main, &mut aux, BLOCK);

    assert_eq!(main, strike_in);
    assert_eq!(aux, blow_in);
}

#[test]
fn external_excitation_drives_the_resonator() {
    let mut part = Part::new();
    {
        let p = part.patch_mut();
        p.exciter_blow_level = 0.0;
        p.exciter_strike_level = 0.0;
        p.exciter_bow_level = 0.0;
        p.resonator_damping = 0.5;
        p.space = 0.0;
    }
    let perf = PerformanceState {
        gate: false,
        note: 45.0,
        modulation: 0.0,
        strength: 0.0,
    };

    let mut energy = 0.0f64;
    for step in 0..400 {
        let mut strike_in = [0.0f32; BLOCK];
        if step == 0 {
            strike_in[0] = 1.0; // an impulse into the STRIKE input
        }
        let blow_in = [0.0f32; BLOCK];
        let mut main = [0.0f32; BLOCK];
        let mut aux = [0.0f32; BLOCK];
        part.process(&perf, &blow_in, &strike_in, &mut main, &mut aux, BLOCK);
        for m in main {
            energy += (m as f64).powi(2);
        }
    }
    assert!(
        energy.sqrt() > 1e-3,
        "resonator did not ring: {}",
        energy.sqrt()
    );
}
