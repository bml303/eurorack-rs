//! Crash / sanity smoke test: every resonator model, every polyphony, plus the
//! string-synth easter egg and the strummer, must survive a long parameter
//! sweep (strums, note changes, extreme structure / damping / position) without
//! panicking, and must produce finite, bounded, audible output.

use rings::{FxType, Part, Patch, PerformanceState, ResonatorModel, StringSynthPart, Strummer};

const BLOCK: usize = 24;

fn models() -> [ResonatorModel; 6] {
    [
        ResonatorModel::Modal,
        ResonatorModel::SympatheticString,
        ResonatorModel::String,
        ResonatorModel::FmVoice,
        ResonatorModel::SympatheticStringQuantized,
        ResonatorModel::StringAndReverb,
    ]
}

fn sweep_part(model: ResonatorModel, polyphony: i32, internal_exciter: bool) -> f64 {
    let mut part = Part::new();
    part.set_model(model);
    part.set_polyphony(polyphony);

    let notes = [45.0f32, 52.0, 57.0, 64.0, 69.0];
    let mut energy = 0.0f64;
    let mut count = 0u64;
    let mut phase = 0.0f32;

    for step in 0..2000i32 {
        let patch = Patch {
            structure: ((step * 7) % 101) as f32 / 100.0,
            brightness: ((step * 13) % 101) as f32 / 100.0,
            damping: ((step * 3) % 101) as f32 / 100.0,
            position: ((step * 29) % 101) as f32 / 100.0,
        };
        let perf = PerformanceState {
            strum: step % 37 == 0,
            internal_exciter,
            internal_strum: false,
            internal_note: true,
            tonic: 12.0,
            note: notes[(step as usize / 50) % notes.len()],
            fm: 0.0,
            chord: (step / 90) % 11,
        };

        let mut input = [0.0f32; BLOCK];
        if !internal_exciter {
            for s in input.iter_mut() {
                phase += 110.0 / 48000.0;
                if phase >= 1.0 {
                    phase -= 1.0;
                }
                *s = (phase - 0.5) * 0.5;
            }
        }
        let mut out = [0.0f32; BLOCK];
        let mut aux = [0.0f32; BLOCK];
        part.process(&perf, &patch, &input, &mut out, &mut aux, BLOCK);

        for (o, a) in out.iter().zip(aux.iter()) {
            assert!(
                o.is_finite() && a.is_finite(),
                "{model:?} p{polyphony} not finite at {step}"
            );
            assert!(
                o.abs() < 4.0 && a.abs() < 4.0,
                "{model:?} p{polyphony} blew up at {step}"
            );
            energy += (*o as f64).powi(2) + (*a as f64).powi(2);
            count += 2;
        }
    }
    (energy / count as f64).sqrt()
}

#[test]
fn every_model_and_polyphony_survives_a_sweep() {
    for model in models() {
        for polyphony in [1, 2, 3, 4] {
            let rms = sweep_part(model, polyphony, true);
            assert!(rms > 1e-4, "{model:?} p{polyphony} internal: rms {rms}");
        }
        // external exciter
        let rms = sweep_part(model, 1, false);
        assert!(rms > 1e-4, "{model:?} external: rms {rms}");
    }
}

#[test]
fn string_model_tracks_pitch() {
    // Mirrors `rings_test.cc::TestPitchAccuracy`: pluck A3 (MIDI 57) on the
    // STRING model and check the fundamental.
    let mut part = Part::new();
    part.set_model(ResonatorModel::String);
    part.set_polyphony(1);
    let patch = Patch {
        structure: 0.25,
        brightness: 0.3, // fairly dark, so the fundamental dominates
        damping: 0.9,
        position: 0.5,
    };

    let mut samples: Vec<f32> = Vec::new();
    for step in 0..2400 {
        let perf = PerformanceState {
            strum: step == 0,
            internal_exciter: true,
            note: 0.0,
            tonic: 57.0,
            ..Default::default()
        };
        let input = [0.0f32; BLOCK];
        let mut out = [0.0f32; BLOCK];
        let mut aux = [0.0f32; BLOCK];
        part.process(&perf, &patch, &input, &mut out, &mut aux, BLOCK);
        if step >= 400 {
            samples.extend_from_slice(&out);
        }
    }

    // Autocorrelation: A3 = 220 Hz -> period ~218 samples at 48 kHz.
    let lag_of = |lo: usize, hi: usize| {
        let mut best_lag = lo;
        let mut best = f64::NEG_INFINITY;
        for lag in lo..hi {
            let mut acc = 0.0f64;
            for i in 0..samples.len() - lag {
                acc += samples[i] as f64 * samples[i + lag] as f64;
            }
            if acc > best {
                best = acc;
                best_lag = lag;
            }
        }
        best_lag
    };
    let lag = lag_of(120, 360);
    let freq = 48000.0 / lag as f32;
    assert!(
        (freq - 220.0).abs() < 10.0,
        "A3 string read as {freq:.1} Hz (lag {lag})"
    );
}

#[test]
fn bypass_passes_input_through() {
    let mut part = Part::new();
    part.set_bypass(true);
    let input: [f32; BLOCK] = core::array::from_fn(|i| (i as f32 - 12.0) * 0.05);
    let mut out = [0.0f32; BLOCK];
    let mut aux = [0.0f32; BLOCK];
    part.process(
        &PerformanceState::default(),
        &Patch::default(),
        &input,
        &mut out,
        &mut aux,
        BLOCK,
    );
    assert_eq!(out, input);
    assert_eq!(aux, input);
}

#[test]
fn strummer_fires_on_note_change_and_inhibits() {
    // Short inter-onset interval (~10 ms = 20 blocks) so the note toggles
    // (every 25 blocks) are not inhibited.
    let sr = 48000.0 / BLOCK as f32;
    let mut fast = Strummer::new();
    fast.init(0.01, sr);
    let mut fast_fired = 0;
    for step in 0..400 {
        let mut perf = PerformanceState {
            internal_strum: true,
            internal_note: false, // note CV drives the strum
            note: if step % 50 < 25 { 48.0 } else { 60.0 },
            ..Default::default()
        };
        fast.process(None, BLOCK, &mut perf);
        if perf.strum {
            fast_fired += 1;
        }
    }
    assert!(
        (12..=16).contains(&fast_fired),
        "fast strummer fired {fast_fired} times"
    );

    // Long inter-onset interval (~100 ms = 200 blocks) inhibits most toggles.
    let mut slow = Strummer::new();
    slow.init(0.1, sr);
    let mut slow_fired = 0;
    for step in 0..400 {
        let mut perf = PerformanceState {
            internal_strum: true,
            internal_note: false,
            note: if step % 50 < 25 { 48.0 } else { 60.0 },
            ..Default::default()
        };
        slow.process(None, BLOCK, &mut perf);
        if perf.strum {
            slow_fired += 1;
        }
    }
    assert!(
        slow_fired < fast_fired,
        "inhibit timer did nothing: {slow_fired} vs {fast_fired}"
    );
}

#[test]
fn string_synth_part_survives_a_sweep_and_makes_sound() {
    for fx in [
        FxType::Formant,
        FxType::Chorus,
        FxType::Reverb,
        FxType::Formant2,
        FxType::Ensemble,
        FxType::Reverb2,
    ] {
        let mut part = StringSynthPart::new();
        part.set_polyphony(2);
        part.set_fx(fx);

        let notes = [45.0f32, 52.0, 57.0, 64.0];
        let mut energy = 0.0f64;
        let mut count = 0u64;

        for step in 0..1500i32 {
            let patch = Patch {
                structure: ((step * 7) % 101) as f32 / 100.0,
                brightness: ((step * 13) % 101) as f32 / 100.0,
                damping: ((step * 3) % 101) as f32 / 100.0,
                position: ((step * 29) % 101) as f32 / 100.0,
            };
            let perf = PerformanceState {
                strum: step % 61 == 0,
                internal_exciter: true,
                note: notes[(step as usize / 40) % notes.len()],
                tonic: 0.0,
                chord: (step / 70) % 11,
                ..Default::default()
            };
            let input = [0.0f32; BLOCK];
            let mut out = [0.0f32; BLOCK];
            let mut aux = [0.0f32; BLOCK];
            part.process(&perf, &patch, &input, &mut out, &mut aux, BLOCK);
            for (o, a) in out.iter().zip(aux.iter()) {
                assert!(
                    o.is_finite() && a.is_finite(),
                    "{fx:?} not finite at {step}"
                );
                assert!(o.abs() < 4.0 && a.abs() < 4.0, "{fx:?} blew up at {step}");
                energy += (*o as f64).powi(2) + (*a as f64).powi(2);
                count += 2;
            }
        }
        let rms = (energy / count as f64).sqrt();
        assert!(rms > 1e-4, "string synth {fx:?}: rms {rms}");
    }
}
