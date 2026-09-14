//! Crash / sanity smoke test: `Modulator` (cross-modulation + vocoder path
//! and the frequency-shifter easter egg), `Oscillator` (every shape), and
//! the `SampleRateConverterUp`/`Down` pair must survive a long parameter
//! sweep without panicking, stay finite, and produce audible (non-silent)
//! output.

use warps::modulator::ShortFrame;
use warps::oscillator::Oscillator;
use warps::parameters::OscillatorShape;
use warps::sample_rate_converter::{SampleRateConverterDown, SampleRateConverterUp, SRC_DOWN_6_48, SRC_UP_6_48};
use warps::Modulator;

const BLOCK: usize = 96;
const SAMPLE_RATE: f32 = 96_000.0;

fn sine_input(phase: &mut f32, freq_hz: f32) -> [ShortFrame; BLOCK] {
    let mut frames = [ShortFrame::default(); BLOCK];
    for f in frames.iter_mut() {
        let s = (*phase * core::f32::consts::TAU).sin();
        let v = (s * 20_000.0) as i16;
        f.l = v;
        f.r = v;
        *phase += freq_hz / SAMPLE_RATE;
        if *phase >= 1.0 {
            *phase -= 1.0;
        }
    }
    frames
}

#[test]
fn modulator_survives_a_long_sweep_and_is_audible() {
    let mut modulator = Modulator::new();
    modulator.init(SAMPLE_RATE);

    let mut phase = 0.0f32;
    let mut any_sound = false;
    let mut any_aux_sound = false;

    for step in 0..2000i32 {
        let input = sine_input(&mut phase, 220.0 + (step % 50) as f32 * 4.0);

        {
            let p = modulator.mutable_parameters();
            p.carrier_shape = step % 4;
            p.channel_drive = [((step * 3) % 100) as f32 / 100.0, ((step * 7) % 100) as f32 / 100.0];
            p.modulation_algorithm = ((step * 5) % 1000) as f32 / 1000.0;
            p.modulation_parameter = ((step * 11) % 1000) as f32 / 1000.0;
            p.note = 24.0 + (step % 48) as f32;
        }

        let mut output = [ShortFrame::default(); BLOCK];
        modulator.process(&input, &mut output, BLOCK);

        for frame in output.iter() {
            if frame.l.unsigned_abs() > 200 {
                any_sound = true;
            }
            if frame.r.unsigned_abs() > 50 {
                any_aux_sound = true;
            }
        }
    }

    assert!(any_sound, "Modulator produced no audible main output over the whole sweep");
    assert!(any_aux_sound, "Modulator produced no audible aux output over the whole sweep");
}

#[test]
fn modulator_bypass_is_a_pass_through() {
    let mut modulator = Modulator::new();
    modulator.init(SAMPLE_RATE);
    modulator.set_bypass(true);
    assert!(modulator.bypass());

    let mut phase = 0.0f32;
    let input = sine_input(&mut phase, 440.0);
    let mut output = [ShortFrame::default(); BLOCK];
    modulator.process(&input, &mut output, BLOCK);

    for (i, o) in input.iter().zip(output.iter()) {
        assert_eq!(i.l, o.l);
        assert_eq!(i.r, o.r);
    }
}

#[test]
fn modulator_easter_egg_survives_a_sweep_and_is_audible() {
    let mut modulator = Modulator::new();
    modulator.init(SAMPLE_RATE);
    modulator.set_easter_egg(true);
    assert!(modulator.easter_egg());

    let mut phase = 0.0f32;
    let mut any_sound = false;

    for step in 0..2000i32 {
        let input = sine_input(&mut phase, 220.0);

        {
            let p = modulator.mutable_parameters();
            p.carrier_shape = step % 4;
            p.frequency_shift_pot = ((step * 3) % 100) as f32 / 100.0;
            p.frequency_shift_cv = ((step * 7) % 100) as f32 / 100.0 - 0.5;
            p.phase_shift = ((step * 13) % 1000) as f32 / 1000.0;
            p.channel_drive = [((step * 5) % 100) as f32 / 100.0, ((step * 9) % 100) as f32 / 100.0];
            p.note = 36.0 + (step % 24) as f32;
        }

        let mut output = [ShortFrame::default(); BLOCK];
        modulator.process(&input, &mut output, BLOCK);

        for frame in output.iter() {
            if frame.l.unsigned_abs() > 200 || frame.r.unsigned_abs() > 200 {
                any_sound = true;
            }
        }
    }

    assert!(any_sound, "Modulator's easter egg produced no audible output over the whole sweep");
}

#[test]
fn oscillator_every_shape_is_finite_and_audible() {
    let shapes = [
        OscillatorShape::Sine,
        OscillatorShape::Triangle,
        OscillatorShape::Saw,
        OscillatorShape::Pulse,
        OscillatorShape::NoiseLp,
    ];
    let modulation = [0.0f32; BLOCK];

    for shape in shapes {
        let mut osc = Oscillator::default();
        osc.init(SAMPLE_RATE);

        let mut any_sound = false;
        for block in 0..200 {
            let note = 24.0 + (block % 60) as f32;
            let mut out = [0.0f32; BLOCK];
            let gain = osc.render(shape, note, &modulation, &mut out);
            assert!(gain.is_finite());
            for &s in out.iter() {
                assert!(s.is_finite(), "{shape:?} produced a non-finite sample");
                if s.abs() > 0.01 {
                    any_sound = true;
                }
            }
        }
        assert!(any_sound, "{shape:?} produced no audible output over the sweep");
    }
}

#[test]
fn sample_rate_converter_roundtrip_preserves_a_sine_without_blowing_up() {
    let mut up: SampleRateConverterUp<8> = SampleRateConverterUp::new(6, &SRC_UP_6_48);
    let mut down: SampleRateConverterDown<48> = SampleRateConverterDown::new(6, &SRC_DOWN_6_48);
    up.init();
    down.init();

    let mut phase = 0.0f32;
    let mut peak_in = 0.0f32;
    let mut peak_out = 0.0f32;

    for _ in 0..200 {
        let mut samples_in = [0.0f32; BLOCK];
        for s in samples_in.iter_mut() {
            *s = (phase * core::f32::consts::TAU).sin() * 0.5;
            phase += 1000.0 / SAMPLE_RATE;
            if phase >= 1.0 {
                phase -= 1.0;
            }
            peak_in = peak_in.max(s.abs());
        }

        let mut oversampled = [0.0f32; BLOCK * 6];
        up.process(&samples_in, &mut oversampled);
        for &s in oversampled.iter() {
            assert!(s.is_finite());
        }

        let mut samples_out = [0.0f32; BLOCK];
        down.process(&oversampled, &mut samples_out);
        for &s in samples_out.iter() {
            assert!(s.is_finite());
            peak_out = peak_out.max(s.abs());
        }
    }

    // Filter group delay means the very first blocks are near-silent while
    // the pipeline fills; by 200 blocks in, the round-tripped peak should be
    // within shouting distance of the input's.
    assert!(peak_out > peak_in * 0.5, "round-tripped sine lost too much amplitude: in={peak_in}, out={peak_out}");
    assert!(peak_out < peak_in * 1.5, "round-tripped sine gained too much amplitude: in={peak_in}, out={peak_out}");
}
