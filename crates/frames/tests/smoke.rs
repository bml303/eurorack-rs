//! Crash / sanity smoke test: `Keyframer` and `PolyLfo` must survive long
//! sweeps without panicking. Specifically targets the three latent
//! out-of-bounds reads this port's fidelity check found in the C++
//! (`Keyframer::Evaluate`'s unconditional `keyframes_[position]` reads and
//! `RemoveKeyframe`'s unconditional `keyframes_[splice_point]` read, all
//! reachable exactly when the keyframe list is completely full and a
//! timestamp past the last keyframe is evaluated/removed) and the
//! `Easing`/`lookup_table_table` out-of-bounds read reachable when
//! evaluating exactly at a keyframe's own timestamp.

use frames::keyframer::{EasingCurve, NUM_CHANNELS};
use frames::{Keyframer, PolyLfo};

fn values(v: u16) -> [u16; NUM_CHANNELS] {
    [v, v.wrapping_mul(2), v.wrapping_mul(3), v.wrapping_mul(5)]
}

#[test]
fn full_keyframe_list_evaluated_past_the_last_keyframe_does_not_panic() {
    let mut kf = Keyframer::new();
    kf.init();
    // Fill all 64 slots (the array's exact capacity) with increasing
    // timestamps -- `position == num_keyframes()` (64) is then reachable.
    for i in 0..64u16 {
        assert!(kf.add_keyframe(i * 1000, &values(i)));
    }
    assert_eq!(kf.num_keyframes(), 64);

    // A timestamp past every keyframe: `find_keyframe` lands exactly at
    // `num_keyframes()`.
    kf.evaluate(65000);
    assert_eq!(kf.position(), 64);
    assert_eq!(kf.nearest_keyframe(), 64);
    // Past the end uses the last keyframe's own values verbatim.
    assert_eq!(kf.level(0), values(63)[0]);

    // Also sweep every timestamp in the tail region, not just one spot check.
    for t in 63000u16..=65000 {
        kf.evaluate(t);
    }
}

#[test]
fn evaluating_exactly_at_a_keyframe_boundary_does_not_panic() {
    let mut kf = Keyframer::new();
    kf.init();
    // Large, widely-spaced channel values so the boundary rounding below
    // (the curve's `shaped_scale` tops out at 65535, and `>> 1 >> 15` loses
    // its very last ULP) stays a negligible fraction of the target.
    let value_at = |i: u16| -> [u16; NUM_CHANNELS] { [i * 6000, i * 6000, i * 6000, i * 6000] };
    for i in 0..10u16 {
        assert!(kf.add_keyframe(i * 6000, &value_at(i)));
    }
    // Evaluating exactly at an interior keyframe's own timestamp makes
    // `scale` come out to exactly 65536 in `Easing`'s lookup-table branch.
    for curve in [EasingCurve::InQuartic, EasingCurve::OutQuartic, EasingCurve::Sine, EasingCurve::Bounce] {
        for ch in 0..NUM_CHANNELS {
            kf.mutable_settings(ch).easing_curve = curve;
        }
        for i in 1..9u16 {
            kf.evaluate(i * 6000);
            // At the exact boundary, the interpolation must resolve to
            // (within the curve's inherent last-ULP rounding) the
            // keyframe's own values (scale == 1.0 exactly).
            let target = value_at(i)[0];
            let got = kf.level(0);
            assert!(got.abs_diff(target) <= 4, "curve {curve:?} at keyframe {i}: got {got}, expected close to {target}");
        }
    }
}

#[test]
fn removing_past_the_end_of_a_full_list_returns_false_without_panicking() {
    let mut kf = Keyframer::new();
    kf.init();
    for i in 0..64u16 {
        assert!(kf.add_keyframe(i * 1000, &values(i)));
    }
    assert!(!kf.remove_keyframe(65000));
    assert_eq!(kf.num_keyframes(), 64);
}

#[test]
fn keyframer_survives_a_long_add_remove_evaluate_sweep() {
    let mut kf = Keyframer::new();
    kf.init();

    for step in 0..5000i32 {
        let timestamp = ((step * 97) % 65536) as u16;
        match step % 5 {
            0 | 1 => {
                kf.add_keyframe(timestamp, &values(timestamp));
            }
            2 => {
                kf.remove_keyframe(timestamp);
            }
            _ => {}
        }
        for ch in 0..NUM_CHANNELS {
            let s = kf.mutable_settings(ch);
            s.easing_curve = match (step + ch as i32) % 6 {
                0 => EasingCurve::Step,
                1 => EasingCurve::Linear,
                2 => EasingCurve::InQuartic,
                3 => EasingCurve::OutQuartic,
                4 => EasingCurve::Sine,
                _ => EasingCurve::Bounce,
            };
            s.response = ((step * 3) % 256) as u8;
        }
        kf.evaluate(((step * 131) % 65536) as u16);
        let _ = kf.find_nearest_keyframe(((step * 211) % 65536) as u16, 500);
        let _ = kf.sample_animation(0, ((step * 17) % 65536) as u16, step % 2 == 0);
    }
}

#[test]
fn empty_keyframer_falls_back_to_immediate_values() {
    let mut kf = Keyframer::new();
    kf.init();
    for ch in 0..NUM_CHANNELS {
        kf.set_immediate(ch, 1234 + ch as u16);
    }
    kf.evaluate(1000);
    assert_eq!(kf.position(), -1);
    assert_eq!(kf.nearest_keyframe(), -1);
    for ch in 0..NUM_CHANNELS {
        assert_eq!(kf.level(ch), 1234 + ch as u16);
    }
}

#[test]
fn convert_to_dac_code_endpoints_are_finite_and_in_range() {
    for gain in [0u16, 1, 32768, 65534, 65535] {
        for response in [0u8, 1, 128, 255] {
            let code = Keyframer::convert_to_dac_code(gain, response);
            assert!(code <= 4095, "dac code {code} out of the 12-bit DAC range for gain={gain}, response={response}");
        }
    }
}

#[test]
fn poly_lfo_survives_a_long_sweep_and_is_alive() {
    let mut lfo = PolyLfo::new();
    lfo.init();

    let mut any_variation = false;
    let mut previous_level = lfo.level(0);

    for step in 0..20_000i32 {
        lfo.set_shape((step * 37 % 65536) as u16);
        lfo.set_shape_spread((step * 53 % 65536) as u16);
        lfo.set_spread((step * 71 % 65536) as u16);
        lfo.set_coupling((step * 89 % 65536) as u16);

        let frequency = (step * 5) % 65536;
        lfo.render(frequency);

        for i in 0..NUM_CHANNELS {
            let _ = lfo.dac_code(i);
        }
        let _ = lfo.color();

        if lfo.level(0) != previous_level {
            any_variation = true;
        }
        previous_level = lfo.level(0);
    }

    assert!(any_variation, "PolyLfo's output never changed over the whole sweep");
}
