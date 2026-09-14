//! Crash / sanity smoke test: `Processor` must survive a long sweep across
//! every `ProcessorFunction`/`alternate`/`linked` combination without
//! panicking, and each algorithm should be independently exercised for
//! plausible (bounded, non-static where expected) output. Also covers the
//! fixed-point `log2`/`exp2` helpers and the small shared utilities
//! (`Svf`, `AudioCvMeter`, `meta_parameters`).

use streams::compressor::Compressor;
use streams::processor::ProcessorFunction;
use streams::{AudioCvMeter, Envelope, FilterController, LorenzGenerator, Processor, Svf, Vactrol};

fn functions() -> [ProcessorFunction; 6] {
    [
        ProcessorFunction::Envelope,
        ProcessorFunction::Vactrol,
        ProcessorFunction::Follower,
        ProcessorFunction::Compressor,
        ProcessorFunction::FilterController,
        ProcessorFunction::LorenzGenerator,
    ]
}

#[test]
fn processor_survives_a_long_sweep_across_every_function() {
    let mut p = Processor::new();

    for step in 0..20_000i32 {
        if step % 173 == 0 {
            p.set_function(functions()[(step / 173) as usize % 6]);
        }
        if step % 211 == 0 {
            p.set_alternate(step % 422 == 0);
        }
        if step % 307 == 0 {
            p.set_linked(step % 614 == 0);
        }
        if step % 5 == 0 {
            p.set_parameter(0, ((step * 37) % 65536) as u16);
            p.set_parameter(1, ((step * 53) % 65536) as u16);
        }
        if step % 7 == 0 {
            p.set_global(0, ((step * 61) % 65536) as u16);
            p.set_global(1, ((step * 67) % 65536) as u16);
            p.set_global(2, ((step * 71) % 65536) as u16);
            p.set_global(3, ((step * 73) % 65536) as u16);
        }
        p.configure();

        let audio = (((step * 997) % 65536) - 32768) as i16;
        let excite = (((step * 1009) % 65536) - 32768) as i16;
        let (_gain, _frequency) = p.process(audio, excite);

        let _ = p.function();
        let _ = p.alternate();
        let _ = p.linked();
        let _ = p.last_gain();
        let _ = p.last_frequency();
        let _ = p.gain_reduction();
    }
}

#[test]
fn envelope_triggers_and_reaches_full_level_on_a_gate() {
    let mut env = Envelope::default();
    env.init();
    env.set_ad(0, 8192);

    let mut gain = 0u16;
    let mut frequency = 0u16;
    let mut peak_gain = 0u16;
    // Hold "excite" high (above the Schmitt trigger threshold) for a while.
    for _ in 0..2000 {
        env.process(0, 30000, &mut gain, &mut frequency);
        peak_gain = peak_gain.max(gain);
    }
    assert!(peak_gain > 25000, "envelope never reached a high gain while gated: peak={peak_gain}");

    // Release: gain should eventually decay back towards zero.
    for _ in 0..20000 {
        env.process(0, 0, &mut gain, &mut frequency);
    }
    assert!(gain < 2000, "envelope did not decay after release: gain={gain}");
}

#[test]
fn vactrol_plucked_and_continuous_modes_survive_a_sweep() {
    for alternate in [false, true] {
        let mut v = Vactrol::default();
        v.init();
        let parameters = [20000i32, 40000i32];
        v.configure(alternate, &parameters, None);

        let mut gain = 0u16;
        let mut frequency = 0u16;
        let mut any_gain = false;
        for step in 0..5000i32 {
            let excite = if step % 200 < 20 { 30000i16 } else { 0i16 };
            v.process(0, excite, &mut gain, &mut frequency);
            if gain > 100 {
                any_gain = true;
            }
        }
        assert!(any_gain, "Vactrol (alternate={alternate}) never produced gain over the sweep");
    }
}

#[test]
fn follower_survives_a_long_audio_sweep() {
    let mut f = streams::Follower::default();
    f.init();
    let parameters = [16384i32, 32768i32];
    f.configure(false, &parameters, None);

    let mut phase = 0.0f32;
    let mut gain = 0u16;
    let mut frequency = 0u16;
    let mut any_gain = false;
    for _ in 0..10_000 {
        phase += 440.0 / 48000.0;
        if phase >= 1.0 {
            phase -= 1.0;
        }
        let excite = ((phase * core::f32::consts::TAU).sin() * 20000.0) as i16;
        f.process(0, excite, &mut gain, &mut frequency);
        if gain > 10 {
            any_gain = true;
        }
    }
    assert!(any_gain, "Follower never produced gain over a sine sweep");
}

#[test]
fn compressor_reduces_gain_on_a_loud_signal() {
    let mut c = Compressor::default();
    c.init();
    let parameters = [20000i32, 50000i32];
    c.configure(false, &parameters, None);

    let mut gain = 0u16;
    let mut frequency = 0u16;
    for _ in 0..5000 {
        c.process(30000, 30000, &mut gain, &mut frequency);
    }
    // Some gain reduction should have kicked in for a loud, sustained signal.
    assert!(c.gain_reduction() != 0, "compressor reported zero gain reduction for a loud sustained signal");
    assert_eq!(frequency, 65535);
}

#[test]
fn filter_controller_tracks_excite() {
    let mut fc = FilterController::default();
    fc.init();
    let parameters = [10000i32, 60000i32];
    fc.configure(false, &parameters, None);

    let mut gain = 0u16;
    let mut frequency_low = 0u16;
    let mut frequency_high = 0u16;
    for _ in 0..500 {
        fc.process(0, 0, &mut gain, &mut frequency_low);
    }
    for _ in 0..500 {
        fc.process(0, 30000, &mut gain, &mut frequency_high);
    }
    assert_eq!(gain, 0);
    assert_ne!(frequency_low, frequency_high, "filter controller's frequency output didn't track excite");
}

#[test]
fn lorenz_generator_survives_a_long_sweep_on_both_channel_indices() {
    for index in [0u8, 1u8] {
        let mut lg = LorenzGenerator::default();
        lg.init();
        lg.set_index(index);
        let parameters = [40000i32, 20000i32];
        lg.configure(false, &parameters, None);

        let mut gain = 0u16;
        let mut frequency = 0u16;
        for step in 0..10_000i32 {
            let excite = ((step * 37) % 65536 - 32768) as i16;
            lg.process(0, excite, &mut gain, &mut frequency);
        }
    }
}

#[test]
fn log2_is_monotonic_and_exp2_survives_its_output_range() {
    // `log2`'s 16.16 fixed-point scale isn't "octaves relative to 1.0" (so
    // `exp2(log2(v)) == v` doesn't hold in linear units) -- but both must
    // stay monotonic and neither should panic across a wide input range.
    let mut previous = i32::MIN;
    for value in [1i32, 2, 256, 1000, 10000, 100000, 1_000_000, i32::MAX / 2, i32::MAX] {
        let l = Compressor::log2(value);
        assert!(l >= previous, "log2 should be non-decreasing: log2({value}) = {l} < previous {previous}");
        previous = l;
        let _ = Compressor::exp2(l);
    }

    // Kept inside a range where the final left-shift by `num_shifts`
    // doesn't itself overflow i32 -- `exp2` is dead code in the C++ (never
    // called), so there's no real usage pattern to say what its valid
    // input domain is meant to be beyond "doesn't panic", which the
    // `wrapping_*` ops already guarantee unconditionally.
    let mut previous = i32::MIN;
    for l in [-200_000i32, -1000, 0, 1000, 65536, 200_000] {
        let e = Compressor::exp2(l);
        assert!(e >= previous, "exp2 should be non-decreasing: exp2({l}) = {e} < previous {previous}");
        previous = e;
    }
}

#[test]
fn svf_survives_a_sweep_and_stays_in_16_bit_range() {
    let mut svf = Svf::default();
    svf.init();
    svf.set_frequency(20 << 7);
    svf.set_resonance(20000);

    let mut phase = 0.0f32;
    for _ in 0..10_000 {
        phase += 220.0 / 48000.0;
        if phase >= 1.0 {
            phase -= 1.0;
        }
        let sample = ((phase * core::f32::consts::TAU).sin() * 30000.0) as i32;
        svf.process(sample);
        assert!(svf.lp().abs() <= 32767);
        assert!(svf.bp().abs() <= 32767);
        assert!(svf.hp().abs() <= 32767);
    }
}

#[test]
fn audio_cv_meter_survives_a_sweep_and_discriminates() {
    let mut meter = AudioCvMeter::default();
    meter.init();

    // A slow-moving "CV" signal: long runs between zero crossings.
    // `average_zero_crossing_interval_` is a one-pole filter (~1/8 per
    // update) updated only at each crossing/reset event, so it takes many
    // ~800-sample periods to converge -- give it plenty of them.
    for step in 0..60_000i32 {
        let sample = if (step / 800) % 2 == 0 { 10000 } else { -10000 };
        meter.process(sample);
    }
    assert!(meter.cv(), "meter should classify a slow-moving signal as CV");

    let mut meter = AudioCvMeter::default();
    meter.init();
    // A fast audio-rate signal: zero crossings every couple of samples.
    let mut phase = 0.0f32;
    for _ in 0..3000 {
        phase += 440.0 / 48000.0;
        if phase >= 1.0 {
            phase -= 1.0;
        }
        let sample = ((phase * core::f32::consts::TAU).sin() * 10000.0) as i32;
        meter.process(sample);
    }
    assert!(!meter.cv(), "meter should classify a 440 Hz signal as audio");
}
