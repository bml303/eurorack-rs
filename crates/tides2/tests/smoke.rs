//! Coverage the golden-checksum test doesn't reach: the external-ramp-source
//! path through `PolySlopeGenerator` (`equivalence.rs` only drives the
//! internal-ramp path, matching `tides_test.cc`'s `TestPolySlopeGenerator`),
//! and `RampExtractor` end-to-end. Crash/panic + plausibility smoke tests, in
//! the spirit of `mi-plaits`'s `tests/smoke.rs` -- not numeric-correctness
//! tests.

use stmlib::gate_flags::GateFlags;
use tides2::{OutputMode, OutputSample, PolySlopeGenerator, RampExtractor, RampMode, Range, Ratio};

const BLOCK: usize = 6;
const SAMPLE_RATE: f32 = 48000.0;

#[test]
fn poly_slope_generator_external_ramp_is_finite() {
    let ramp_modes = [RampMode::Ad, RampMode::Looping, RampMode::Ar];
    let output_modes =
        [OutputMode::Gates, OutputMode::Amplitude, OutputMode::SlopePhase, OutputMode::Frequency];
    let ranges = [Range::Control, Range::Audio];

    for &ramp_mode in &ramp_modes {
        for &output_mode in &output_modes {
            for &range in &ranges {
                let mut poly_slope = PolySlopeGenerator::new();
                let mut phase = 0.0f32;
                let f0 = 4.0 / SAMPLE_RATE;
                let no_gate = [GateFlags::LOW; BLOCK];

                for step in 0..2000 {
                    let mut ramp = [0.0f32; BLOCK];
                    for r in ramp.iter_mut() {
                        *r = phase;
                        phase += f0;
                        if phase >= 1.0 {
                            phase -= 1.0;
                        }
                    }

                    let pw = ((step * 37) % 101) as f32 / 100.0;
                    let shape = ((step * 53) % 101) as f32 / 100.0;
                    let smoothness = ((step * 71) % 101) as f32 / 100.0;
                    let shift = ((step * 13) % 101) as f32 / 100.0;

                    let mut out = [OutputSample::default(); BLOCK];
                    poly_slope.render(
                        ramp_mode,
                        output_mode,
                        range,
                        f0,
                        pw,
                        shape,
                        smoothness,
                        shift,
                        &no_gate,
                        Some(&ramp),
                        &mut out,
                        BLOCK,
                    );

                    for sample in out {
                        for c in sample.channel {
                            assert!(
                                c.is_finite(),
                                "{ramp_mode:?}/{output_mode:?}/{range:?} step {step}: non-finite output {c}"
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn ramp_extractor_tracks_a_steady_clock() {
    // A steady 2 Hz gate train (50% duty): the extractor should converge to a
    // ramp frequency near 2/48000 and keep producing a ramp in [0, 1).
    let mut extractor = RampExtractor::new();
    extractor.init(SAMPLE_RATE, 40.0 / SAMPLE_RATE);

    let period = (SAMPLE_RATE / 2.0) as usize; // 2 Hz
    let on = period / 2;
    let ratio = Ratio { ratio: 1.0, q: 1 };

    // Starts LOW (sample 0 is the first of the `period - on` low samples) so
    // the very first-ever edge fed to the freshly-`init`ed extractor is not a
    // RISING at `total_duration == 0` -- that's a divide-by-zero-by-way-of
    // -`period` in the *original C* too (`expected_phase = ... / period`,
    // `period` from a pulse of duration 0), not something this port
    // introduced, but it hangs the `while (expected_phase >= 1.0) ...` loop
    // either way, so the test steers clear of it rather than "fixing" the
    // port to tolerate an input the firmware itself can't.
    let mut previous_high = false;
    let mut last_frequency = 0.0f32;
    let mut n = 0usize;
    while n < SAMPLE_RATE as usize * 6 {
        let mut gate_flags = [GateFlags::LOW; BLOCK];
        for (j, g) in gate_flags.iter_mut().enumerate() {
            let sample_index = n + j;
            let high = (sample_index % period) >= (period - on);
            *g = if high {
                if previous_high {
                    GateFlags::HIGH
                } else {
                    GateFlags::HIGH | GateFlags::RISING
                }
            } else if previous_high {
                GateFlags::FALLING
            } else {
                GateFlags::LOW
            };
            previous_high = high;
        }

        let mut ramp = [0.0f32; BLOCK];
        last_frequency = extractor.process(true, false, ratio, &gate_flags, &mut ramp, BLOCK);

        for &r in &ramp {
            assert!((0.0..1.0).contains(&r), "ramp {r} out of [0, 1)");
        }
        n += BLOCK;
    }

    let expected = 2.0 / SAMPLE_RATE;
    assert!(
        (last_frequency - expected).abs() < expected * 0.05,
        "extractor frequency {last_frequency} did not converge near {expected}"
    );
}
