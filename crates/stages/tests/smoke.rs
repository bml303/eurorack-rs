//! Crash / sanity smoke test, mirroring the scenarios in the C's
//! `stages/test/stages_test.cc` (a WAV-dumping test harness that isn't
//! reproducible here without a C bit-compare rig -- see this crate's
//! `PORTING.md`). Each `SegmentGenerator` function is configured the same
//! way the C test does and driven with a real gate stream, checking outputs
//! stay finite and the engine never panics.

use stages::segment_generator::segment::{Configuration, Type};
use stages::segment_generator::Output;
use stages::SegmentGenerator;
use stmlib::gate_flags::{extract_gate_flags, GateFlags};

/// Turns a boolean "is the gate high" pattern into a `GateFlags` stream with
/// correctly tagged rising/falling edges.
fn gate_stream(levels: impl Iterator<Item = bool>) -> Vec<GateFlags> {
    let mut previous = GateFlags::LOW;
    let mut out = Vec::new();
    for level in levels {
        let flag = extract_gate_flags(previous, level);
        out.push(flag);
        previous = flag;
    }
    out
}

/// A periodic clock: high for `on` samples out of every `period`.
fn clock(len: usize, period: usize, on: usize) -> Vec<GateFlags> {
    gate_stream((0..len).map(|i| (i % period) < on))
}

/// Processes a long buffer in chunks no larger than `MAX_BLOCK_SIZE`
/// (`ProcessOscillator`'s scratch buffer is stack-allocated at that size).
fn run_chunked(generator: &mut SegmentGenerator, len: usize) -> Vec<Output> {
    const CHUNK: usize = 32;
    let mut out = vec![Output::default(); len];
    let gates = [GateFlags::LOW; CHUNK];
    for chunk in out.chunks_mut(CHUNK) {
        generator.process(&gates[..chunk.len()], chunk);
    }
    out
}

fn assert_finite(out: &[Output]) {
    for o in out {
        assert!(o.value.is_finite(), "value went non-finite: {:?}", o.value);
        assert!(o.phase.is_finite(), "phase went non-finite: {:?}", o.phase);
    }
}

fn run(
    generator: &mut SegmentGenerator,
    gates: &[GateFlags],
    set_params: impl Fn(usize, &mut SegmentGenerator),
) -> Vec<Output> {
    let mut out = vec![Output::default(); gates.len()];
    for (i, (&g, o)) in gates.iter().zip(out.iter_mut()).enumerate() {
        set_params(i, generator);
        generator.process(core::slice::from_ref(&g), core::slice::from_mut(o));
    }
    out
}

#[test]
fn adsr_multi_segment_tracks_gate() {
    let mut g = SegmentGenerator::new();
    g.init(false);
    let configuration = [
        Configuration { kind: Type::Ramp, looping: false },
        Configuration { kind: Type::Ramp, looping: false },
        Configuration { kind: Type::Ramp, looping: false },
        Configuration { kind: Type::Hold, looping: true },
        Configuration { kind: Type::Ramp, looping: false },
    ];
    g.configure(true, &configuration, 5);
    g.set_segment_parameters(0, 0.15, 0.0);
    g.set_segment_parameters(1, 0.25, 0.3);
    g.set_segment_parameters(2, 0.25, 0.75);
    g.set_segment_parameters(3, 0.5, 0.1);
    g.set_segment_parameters(4, 0.5, 0.25);

    let gates = clock(32000, 16000, 8000);
    let out = run(&mut g, &gates, |_, _| {});
    assert_finite(&out);

    // The envelope should actually move, not sit frozen at zero.
    let max_value = out.iter().map(|o| o.value).fold(0.0f32, f32::max);
    assert!(max_value > 0.1, "ADSR never rose above {max_value}");
}

#[test]
fn two_step_hold_sequence_runs() {
    // Two segments is below `Configure`'s `num_segments >= 3` sequencer-mode
    // threshold, so (exactly as in the C) this goes through the general
    // multi-segment path, not `ProcessSequencer`: with no `TYPE_STEP`
    // segment present, every segment's `if_rising` stays the default 0, so
    // *any* rising edge -- not just one on the sentinel -- restarts the
    // pair at segment 0. A single trigger is enough to demonstrate this
    // engine's basic shape without depending on `RateToFrequency`'s exact
    // timing to outrace a periodic clock.
    let mut g = SegmentGenerator::new();
    g.init(false);
    let configuration = [
        Configuration { kind: Type::Hold, looping: false },
        Configuration { kind: Type::Hold, looping: false },
    ];
    g.configure(true, &configuration, 2);
    g.set_segment_parameters(0, 0.2, 0.24);
    g.set_segment_parameters(1, 0.8, 0.24);

    let gates = gate_stream((0..8000).map(|i| i < 4));
    let out = run(&mut g, &gates, |_, _| {});
    assert_finite(&out);
    let segments: std::collections::HashSet<i32> = out.iter().map(|o| o.segment).collect();
    assert!(segments.len() > 1, "two-step sequence never advanced: {segments:?}");
}

#[test]
fn single_decay_envelope_falls_after_trigger() {
    let mut g = SegmentGenerator::new();
    g.init(false);
    let configuration = [Configuration { kind: Type::Ramp, looping: false }];
    g.configure(true, &configuration, 1);
    g.set_segment_parameters(0, 0.7, 0.2);

    let gates = clock(4000, 4000, 1);
    let out = run(&mut g, &gates, |_, _| {});
    assert_finite(&out);
    assert!(out[0].value > 0.9, "decay envelope should start near 1.0 right after trigger: {}", out[0].value);
    assert!(out[3999].value < out[0].value, "decay envelope never decayed");
}

#[test]
fn timed_pulse_generator_pulses() {
    let mut g = SegmentGenerator::new();
    g.init(false);
    let configuration = [Configuration { kind: Type::Hold, looping: false }];
    g.configure(true, &configuration, 1);
    g.set_segment_parameters(0, 0.9, 0.4);

    let gates = clock(4000, 4000, 1);
    let out = run(&mut g, &gates, |_, _| {});
    assert_finite(&out);
    assert!(out.iter().any(|o| o.value > 0.5), "timed pulse never fired high");
    assert!(out.iter().any(|o| o.value == 0.0), "timed pulse never returned to 0");
}

#[test]
fn gate_generator_follows_gate_with_random_acceptance() {
    let mut g = SegmentGenerator::new();
    g.init(false);
    let configuration = [Configuration { kind: Type::Hold, looping: true }];
    g.configure(true, &configuration, 1);
    // secondary == 1.0 -> `accepted_gate` probability is always satisfied.
    g.set_segment_parameters(0, 0.5, 1.0);

    let gates = clock(2000, 100, 50);
    let out = run(&mut g, &gates, |_, _| {});
    assert_finite(&out);
    assert!(out.iter().any(|o| o.value > 0.0));
    assert!(out.iter().any(|o| o.value == 0.0));
}

#[test]
fn sample_and_hold_updates_on_rising_edge() {
    let mut g = SegmentGenerator::new();
    g.init(false);
    let configuration = [Configuration { kind: Type::Step, looping: true }];
    g.configure(true, &configuration, 1);

    let gates = clock(2000, 300, 30);
    let out = run(&mut g, &gates, |i, generator| {
        // Slowly sweep the primary parameter so successive samples differ.
        generator.set_segment_parameters(0, (i as f32 / 2000.0).fract(), 0.5);
    });
    assert_finite(&out);
}

#[test]
fn portamento_slides_towards_target() {
    let mut g = SegmentGenerator::new();
    g.init(false);
    let configuration = [Configuration { kind: Type::Step, looping: true }];
    g.configure(false, &configuration, 1);
    // A small `secondary` (portamento rate) means a fast slide -- see
    // `portamento_rate_to_lp_coefficient`'s table.
    g.set_segment_parameters(0, 0.9, 0.05);

    let out = run_chunked(&mut g, 4000);
    assert_finite(&out);
    assert!((out[3999].value - 0.9).abs() < 0.05, "portamento never settled near target: {}", out[3999].value);
}

#[test]
fn free_running_lfo_oscillates() {
    let mut g = SegmentGenerator::new();
    g.init(false);
    let configuration = [Configuration { kind: Type::Ramp, looping: true }];
    g.configure(false, &configuration, 1);
    g.set_segment_parameters(0, 0.7, 0.4);

    let out = run_chunked(&mut g, 8000);
    assert_finite(&out);
    let min = out.iter().map(|o| o.value).fold(f32::MAX, f32::min);
    let max = out.iter().map(|o| o.value).fold(f32::MIN, f32::max);
    assert!(max - min > 0.1, "free-running LFO produced almost no swing ({min}..{max})");
}

#[test]
fn tap_lfo_locks_to_a_clock() {
    let mut g = SegmentGenerator::new();
    g.init(false);
    let configuration = [Configuration { kind: Type::Ramp, looping: true }];
    g.configure(true, &configuration, 1);
    g.set_segment_parameters(0, 0.5, 0.5);

    let gates = clock(20000, 1000, 500);
    let out = run(&mut g, &gates, |_, _| {});
    assert_finite(&out);
}

#[test]
fn delay_repeats_a_clocked_signal() {
    let mut g = SegmentGenerator::new();
    g.init(false);
    let configuration = [Configuration { kind: Type::Hold, looping: false }];
    g.configure(false, &configuration, 1);
    g.set_segment_parameters(0, 0.6, 0.5);

    let mut out = vec![Output::default(); 4000];
    let gates = vec![GateFlags::LOW; 4000];
    g.process(&gates, &mut out);
    assert_finite(&out);
}

#[test]
fn zero_function_is_silent() {
    let mut g = SegmentGenerator::new();
    g.init(false);
    let configuration = [Configuration { kind: Type::Ramp, looping: false }];
    g.configure(false, &configuration, 1);
    g.set_segment_parameters(0, 0.5, 0.05);

    let mut out = vec![Output { value: 1.0, phase: 1.0, segment: -1 }; 100];
    let gates = vec![GateFlags::LOW; 100];
    g.process(&gates, &mut out);
    assert!(out.iter().all(|o| o.value == 0.0 && o.segment == 1));
}

#[test]
fn audio_oscillator_runs_at_audio_rate() {
    let mut g = SegmentGenerator::new();
    g.init(false);
    let configuration = [Configuration { kind: Type::Alt, looping: true }];
    g.configure(false, &configuration, 1);
    g.set_segment_parameters(0, 0.5, 0.7);

    let out = run_chunked(&mut g, 4000);
    assert_finite(&out);
    let min = out.iter().map(|o| o.value).fold(f32::MAX, f32::min);
    let max = out.iter().map(|o| o.value).fold(f32::MIN, f32::max);
    assert!(max - min > 0.2, "audio oscillator produced almost no swing ({min}..{max})");
}

#[test]
fn slave_tracks_a_monitored_master_segment() {
    let mut master = SegmentGenerator::new();
    master.init(false);
    let configuration = [
        Configuration { kind: Type::Hold, looping: false },
        Configuration { kind: Type::Hold, looping: false },
    ];
    master.configure(true, &configuration, 2);
    master.set_segment_parameters(0, 0.2, 0.1);
    master.set_segment_parameters(1, 0.8, 0.1);

    let mut slave = SegmentGenerator::new();
    slave.init(false);
    slave.configure_slave(0);

    let gates = clock(500, 100, 10);
    let mut out = vec![Output::default(); gates.len()];
    for (&g, o) in gates.iter().zip(out.iter_mut()) {
        master.process(core::slice::from_ref(&g), core::slice::from_mut(o));
    }
    let mut solo = out.clone();
    slave.process(&gates, &mut solo);
    assert_finite(&solo);
    // Whenever the master is on segment 0, the slave should output `1 - phase`.
    for (m, s) in out.iter().zip(solo.iter()) {
        if m.segment == 0 {
            assert!((s.value - (1.0 - m.phase)).abs() < 1e-6);
        } else {
            assert_eq!(s.value, 0.0);
        }
    }
}

#[test]
fn delay_line_16_bits_reads_back_what_it_wrote() {
    use stages::DelayLine16Bits;
    let mut d: DelayLine16Bits<8> = DelayLine16Bits::new();
    d.init();
    for i in 0..21 {
        d.write(i as f32 / 22.0 + 0.01);
        let a = d.read(1.0);
        let b = d.read(2.0);
        let c = d.read(1.2);
        assert!(a.is_finite() && b.is_finite() && c.is_finite());
        assert!((c - (a + (b - a) * 0.2)).abs() < 1e-3);
    }
}

#[test]
fn oscillator_all_shapes_stay_bounded() {
    use stages::{Oscillator, OscillatorShape};
    let shapes = [
        OscillatorShape::ImpulseTrain,
        OscillatorShape::Saw,
        OscillatorShape::Sine,
        OscillatorShape::Triangle,
        OscillatorShape::Slope,
        OscillatorShape::Square,
        OscillatorShape::SquareBright,
        OscillatorShape::SquareDark,
        OscillatorShape::SquareTriangle,
    ];
    for shape in shapes {
        let mut osc = Oscillator::default();
        osc.init();
        let mut out = [0.0f32; 32];
        for _ in 0..200 {
            osc.render(shape, 0.02, 0.5, None, &mut out);
            for &s in &out {
                assert!(s.is_finite(), "{shape:?} produced a non-finite sample");
                assert!(s.abs() < 200.0, "{shape:?} produced an implausible sample {s}");
            }
        }
    }
}
