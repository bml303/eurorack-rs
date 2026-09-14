//! Crash / sanity smoke test: `Processors` must survive a long sweep
//! across every `ProcessorFunction`/`ControlMode`/parameter combination,
//! called in blocks of 4 (`peaks::kBlockSize`, matching the firmware's own
//! call pattern -- see the crate's module doc comment on why that matters
//! for a few engines). Each engine is also exercised individually.

use peaks::gate_processor::{extract_gate_flags, ControlMode, GateFlags};
use peaks::processors::{Processors, ProcessorFunction};

const BLOCK: usize = 4;

/// Turns a boolean "is the gate high" pattern into a `GateFlags` stream
/// with correctly tagged rising/falling edges, using the same
/// `extract_gate_flags` state machine the real hardware's gate scanner
/// uses.
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

fn functions() -> [ProcessorFunction; 12] {
    [
        ProcessorFunction::Envelope,
        ProcessorFunction::Lfo,
        ProcessorFunction::TapLfo,
        ProcessorFunction::BassDrum,
        ProcessorFunction::SnareDrum,
        ProcessorFunction::HighHat,
        ProcessorFunction::FmDrum,
        ProcessorFunction::PulseShaper,
        ProcessorFunction::PulseRandomizer,
        ProcessorFunction::BouncingBall,
        ProcessorFunction::MiniSequencer,
        ProcessorFunction::NumberStation,
    ]
}

#[test]
fn processors_survive_a_long_sweep_across_every_function() {
    let mut p = Processors::new();
    let gates = gate_stream((0..20_000i32).map(|step| (step % 47) < 5));

    for (block_index, block) in gates.chunks(BLOCK).enumerate() {
        if block.len() < BLOCK {
            break;
        }
        let step = block_index as i32;
        if step % 173 == 0 {
            p.set_function(functions()[(step / 173) as usize % 12]);
        }
        if step % 211 == 0 {
            p.set_control_mode(if step % 422 == 0 { ControlMode::Half } else { ControlMode::Full });
        }
        if step % 5 == 0 {
            p.set_parameter(0, ((step * 37) % 65536) as u16);
            p.set_parameter(1, ((step * 53) % 65536) as u16);
            p.set_parameter(2, ((step * 71) % 65536) as u16);
            p.set_parameter(3, ((step * 89) % 65536) as u16);
        }

        let mut out = [0i16; BLOCK];
        p.process(block, &mut out);

        let _ = p.function();
        let _ = p.number_station().digit();
        let _ = p.number_station().gate();
    }
}

#[test]
fn multistage_envelope_triggers_and_releases_via_real_gate_edges() {
    use peaks::modulations::multistage_envelope::MultistageEnvelope;

    let mut env = MultistageEnvelope::default();
    env.init();
    // A short, fast release so it actually completes within this test's window.
    env.set_adsr(0, 8192, 16384, 0);

    // Long gate-high period, then release.
    let gates = gate_stream((0..4000).map(|i| i < 3000));
    let mut out = vec![0i16; gates.len()];
    env.process(&gates, &mut out);

    let peak = out.iter().copied().max().unwrap();
    assert!(peak > 20000, "envelope never reached a high level while gated: peak={peak}");
    let tail_avg: i32 = out[out.len() - 200..].iter().map(|&v| v as i32).sum::<i32>() / 200;
    assert!(tail_avg.abs() < 5000, "envelope did not release: tail average={tail_avg}");
}

#[test]
fn lfo_every_shape_is_alive_synced_and_unsynced() {
    use peaks::modulations::lfo::{Lfo, LfoShape};

    for sync in [false, true] {
        for shape in [LfoShape::Sine, LfoShape::Triangle, LfoShape::Square, LfoShape::Steps, LfoShape::Noise] {
            let mut lfo = Lfo::default();
            lfo.init();
            lfo.set_sync(sync);
            lfo.set_shape(shape);
            lfo.set_rate(60000);
            lfo.set_parameter(-10000);
            lfo.set_level(65535);

            // Unsynced: a single trigger, then let it free-run (frequent
            // retriggers would keep resetting phase_ back to 0 and the LFO
            // would never complete a cycle). Synced: needs a periodic clock
            // to lock its tap-tempo pattern predictor onto.
            let gates = if sync { gate_stream((0..8000i32).map(|i| (i % 97) < 4)) } else { gate_stream((0..8000i32).map(|i| i < 4)) };
            let mut out = vec![0i16; gates.len()];
            lfo.process(&gates, &mut out);

            let any_alive = out.windows(2).any(|w| w[0] != w[1]);
            assert!(any_alive, "LFO shape {shape:?} (sync={sync}) never changed output");
        }
    }
}

#[test]
fn drum_voices_survive_triggers_and_are_audible() {
    use peaks::drums::bass_drum::BassDrum;
    use peaks::drums::fm_drum::FmDrum;
    use peaks::drums::high_hat::HighHat;
    use peaks::drums::snare_drum::SnareDrum;

    let gates = gate_stream((0..8000i32).map(|i| (i % 500) < 4));

    let mut bd = BassDrum::default();
    bd.init();
    bd.configure(&[30000, 40000, 20000, 50000], ControlMode::Full);
    let mut out = vec![0i16; gates.len()];
    bd.process(&gates, &mut out);
    assert!(out.iter().any(|&v| v.unsigned_abs() > 1000), "BassDrum produced no audible output");

    let mut sd = SnareDrum::default();
    sd.init();
    sd.configure(&[30000, 40000, 20000, 50000], ControlMode::Full);
    let mut out = vec![0i16; gates.len()];
    sd.process(&gates, &mut out);
    assert!(out.iter().any(|&v| v.unsigned_abs() > 1000), "SnareDrum produced no audible output");

    let mut hh = HighHat::default();
    hh.init();
    hh.configure(&[0, 0, 0, 0], ControlMode::Full);
    let mut out = vec![0i16; gates.len()];
    hh.process(&gates, &mut out);
    assert!(out.iter().any(|&v| v.unsigned_abs() > 100), "HighHat produced no audible output");

    for sd_range in [false, true] {
        let mut fm = FmDrum::default();
        fm.init();
        fm.set_sd_range(sd_range);
        fm.configure(&[30000, 40000, 20000, 10000], ControlMode::Full);
        // FmDrum's recompute cadence assumes blocks of 4 -- process it that way.
        let mut out = vec![0i16; gates.len()];
        for (chunk_in, chunk_out) in gates.chunks(BLOCK).zip(out.chunks_mut(BLOCK)) {
            if chunk_in.len() == BLOCK {
                fm.process(chunk_in, chunk_out);
            }
        }
        assert!(out.iter().any(|&v| v.unsigned_abs() > 1000), "FmDrum (sd_range={sd_range}) produced no audible output");
    }
}

#[test]
fn bouncing_ball_survives_and_bounces() {
    use peaks::modulations::bouncing_ball::BouncingBall;

    let mut ball = BouncingBall::default();
    ball.init();
    ball.configure(&[40000, 50000, 60000, 40000], ControlMode::Full);
    assert!(ball.fill_buffer());

    let gates = gate_stream((0..10_000i32).map(|i| (i % 3000) < 4));
    let mut out = vec![0i16; gates.len()];
    ball.process(&gates, &mut out);

    assert!(out.iter().any(|&v| v > 100));
    assert!(out.iter().all(|&v| v >= 0), "bouncing ball position should never render negative");
}

#[test]
fn mini_sequencer_advances_and_resets() {
    use peaks::modulations::mini_sequencer::MiniSequencer;

    let mut seq = MiniSequencer::default();
    seq.init();
    seq.configure(&[10000, 40000, 60000, 20000], ControlMode::Full);

    let gates = gate_stream((0..2000i32).map(|i| (i % 20) < 2));
    let mut out = vec![0i16; gates.len()];
    seq.process(&gates, &mut out);

    let distinct: std::collections::HashSet<i16> = out.iter().copied().collect();
    assert!(distinct.len() > 1, "mini sequencer output never varied across steps");
}

#[test]
fn pulse_shaper_and_randomizer_survive_and_produce_pulses() {
    use peaks::pulse_processor::pulse_randomizer::PulseRandomizer;
    use peaks::pulse_processor::pulse_shaper::PulseShaper;

    let gates = gate_stream((0..4000i32).map(|i| (i % 200) < 4));

    let mut shaper = PulseShaper::default();
    shaper.init();
    shaper.configure(&[10000, 20000, 30000, 20000], ControlMode::Full);
    let mut any_high = false;
    for chunk in gates.chunks(BLOCK) {
        if chunk.len() < BLOCK {
            break;
        }
        let mut out = [0i16; BLOCK];
        shaper.process(chunk, &mut out);
        if out.iter().any(|&v| v != 0) {
            any_high = true;
        }
    }
    assert!(any_high, "PulseShaper never produced a pulse");

    let mut randomizer = PulseRandomizer::default();
    randomizer.init();
    randomizer.configure(&[65535, 65535, 20000, 0], ControlMode::Full);
    let mut any_high = false;
    for chunk in gates.chunks(BLOCK) {
        if chunk.len() < BLOCK {
            break;
        }
        let mut out = [0i16; BLOCK];
        randomizer.process(chunk, &mut out);
        if out.iter().any(|&v| v != 0) {
            any_high = true;
        }
    }
    assert!(any_high, "PulseRandomizer never produced a pulse");
}

#[test]
fn number_station_survives_voice_and_tone_modes() {
    use peaks::NumberStation;

    for voice in [false, true] {
        let mut ns = NumberStation::default();
        ns.init();
        ns.set_voice(voice);
        ns.configure(&[20000, 40000, 30000, 20000], ControlMode::Full);

        let gates = gate_stream((0..4000i32).map(|i| (i % 300) < 200));
        // NumberStation downsamples 4x internally and needs blocks that are
        // multiples of 4 to emit every output sample -- process it that way.
        let mut out = vec![0i16; gates.len()];
        for (chunk_in, chunk_out) in gates.chunks(BLOCK).zip(out.chunks_mut(BLOCK)) {
            if chunk_in.len() == BLOCK {
                ns.process(chunk_in, chunk_out);
            }
        }

        assert!(out.iter().any(|&v| v != 0), "NumberStation (voice={voice}) produced no output");
        let _ = ns.digit();
        let _ = ns.gate();
    }
}

#[test]
fn calibration_data_dac_code_stays_in_16_bit_range() {
    use peaks::CalibrationData;

    let mut cal = CalibrationData::default();
    cal.init();
    cal.set_dac_offset(0, 1000);
    cal.set_dac_offset(1, -1000);

    for value in [-32768i16, -1000, 0, 1000, 32767] {
        for channel in [0usize, 1] {
            let _ = cal.dac_code(channel, value); // must not panic for any i16 input
        }
    }
    // Higher `value` should map to a lower DAC code (inverted).
    assert!(cal.dac_code(0, 20000) < cal.dac_code(0, -20000));
}
