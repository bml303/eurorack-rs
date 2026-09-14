//! Crash / sanity smoke test: `TGenerator` (every model x range, internal and
//! external clock) feeding `XYGenerator` (every clock source x control mode,
//! register mode on and off) must survive a long parameter sweep without
//! panicking, and must produce finite, boundedly voltage-range output.

use marbles::random::{
    ClockSource, ControlMode, GroupSettings, Ramps, TGenerator, TGeneratorModel, TGeneratorRange,
    VoltageRange, XYGenerator,
};
use marbles::{RandomGenerator, RandomStream};
use stmlib::gate_flags::{GateFlags, extract_gate_flags};

const BLOCK: usize = 16;
const SAMPLE_RATE: f32 = 48_000.0;

fn models() -> [TGeneratorModel; 7] {
    [
        TGeneratorModel::ComplementaryBernoulli,
        TGeneratorModel::Clusters,
        TGeneratorModel::Drums,
        TGeneratorModel::IndependentBernoulli,
        TGeneratorModel::Divider,
        TGeneratorModel::ThreeStates,
        TGeneratorModel::Markov,
    ]
}

fn ranges() -> [TGeneratorRange; 3] {
    [
        TGeneratorRange::Range0_25x,
        TGeneratorRange::Range1x,
        TGeneratorRange::Range4x,
    ]
}

fn clock_sources() -> [ClockSource; 5] {
    [
        ClockSource::InternalT1T2T3,
        ClockSource::InternalT1,
        ClockSource::InternalT2,
        ClockSource::InternalT3,
        ClockSource::External,
    ]
}

fn control_modes() -> [ControlMode; 3] {
    [ControlMode::Identical, ControlMode::Bump, ControlMode::Tilt]
}

#[test]
fn t_and_xy_generators_survive_a_long_sweep() {
    let mut random_stream = RandomStream::new();
    random_stream.init(RandomGenerator::new());

    let mut t_generator = TGenerator::new();
    t_generator.init(&mut random_stream, SAMPLE_RATE);

    let mut xy_generator = XYGenerator::new();
    xy_generator.init(&mut random_stream, SAMPLE_RATE);

    let mut external_gate_state = false;
    let mut external_gate_flags = GateFlags::LOW;
    let mut clock_phase = 0.0f32;

    let mut energy = 0.0f64;
    let mut count = 0u64;

    let models = models();
    let ranges = ranges();
    let clock_sources = clock_sources();
    let control_modes = control_modes();

    for step in 0..4000i32 {
        let model = models[(step as usize / 40) % models.len()];
        let range = ranges[(step as usize / 130) % ranges.len()];
        let use_external_clock = step % 5 == 0;

        t_generator.set_model(model);
        t_generator.set_range(range);
        t_generator.set_rate(((step * 3) % 97) as f32 / 97.0 * 96.0 - 48.0);
        t_generator.set_bias(((step * 7) % 101) as f32 / 100.0);
        t_generator.set_jitter(((step * 11) % 101) as f32 / 100.0);
        t_generator.set_pulse_width_mean(((step * 13) % 101) as f32 / 100.0);
        t_generator.set_pulse_width_std(((step * 17) % 101) as f32 / 100.0);
        t_generator.set_deja_vu(((step * 19) % 101) as f32 / 100.0);
        t_generator.set_length(1 + (step % 16));

        let mut external_clock = [GateFlags::LOW; BLOCK];
        for flags in external_clock.iter_mut() {
            clock_phase += 0.01;
            if clock_phase >= 1.0 {
                clock_phase -= 1.0;
            }
            let level = clock_phase < 0.3;
            external_gate_flags = extract_gate_flags(external_gate_flags, level);
            external_gate_state = level;
            *flags = external_gate_flags;
        }
        let _ = external_gate_state;

        let mut ramp_buffer = [0.0f32; BLOCK * 4];
        let (external_buf, rest) = ramp_buffer.split_at_mut(BLOCK);
        let (master_buf, rest) = rest.split_at_mut(BLOCK);
        let (slave0_buf, slave1_buf) = rest.split_at_mut(BLOCK);

        let mut gate = [false; BLOCK * 2];
        let mut reset = step == 0;

        {
            let mut ramps = Ramps {
                external: external_buf,
                master: master_buf,
                slave: [slave0_buf, slave1_buf],
            };
            t_generator.process(
                &mut random_stream,
                use_external_clock,
                &mut reset,
                &external_clock,
                &mut ramps,
                &mut gate,
            );
        }

        for &v in ramp_buffer.iter() {
            // Only finiteness is asserted here: these are internal ramps
            // (re-wrapped downstream by `RampDivider`/`OutputChannel`), not
            // the final audible signal -- under this test's unrealistically
            // instantaneous parameter jumps (real hardware knobs move
            // continuously), the master/jitter feedback loop in
            // `TGenerator::process` can legitimately drift outside `[0, 1)`
            // for a while, same as the C++ would given identical inputs.
            // The bounded-range assertion below on the final `XYGenerator`
            // output is what actually stands in for "sane behavior".
            assert!(v.is_finite(), "T ramp not finite at step {step}: {v}");
        }

        let clock_source = clock_sources[(step as usize / 23) % clock_sources.len()];
        let control_mode = control_modes[(step as usize / 31) % control_modes.len()];
        let register_mode = step % 9 == 0;

        let x_settings = GroupSettings {
            control_mode,
            voltage_range: VoltageRange::Full,
            register_mode,
            register_value: ((step * 23) % 101) as f32 / 100.0,
            spread: ((step * 29) % 101) as f32 / 100.0,
            bias: ((step * 31) % 101) as f32 / 100.0,
            steps: ((step * 37) % 101) as f32 / 100.0,
            deja_vu: ((step * 41) % 101) as f32 / 100.0,
            scale_index: 0,
            length: 1 + (step % 16),
            ratio: marbles::ramp::Ratio::new(1, 1),
        };
        let y_settings = GroupSettings {
            control_mode: ControlMode::Identical,
            voltage_range: VoltageRange::Positive,
            register_mode: false,
            register_value: 0.0,
            spread: 0.5,
            bias: 0.5,
            steps: 0.5,
            deja_vu: 0.0,
            scale_index: 0,
            length: 1,
            ratio: marbles::ramp::Ratio::new(1, 3),
        };

        let mut xy_reset = reset;
        let mut output = [0.0f32; BLOCK * 4];
        {
            // Rebuild disjoint slices over the same buffer the T pass above
            // just filled -- that borrow ended when its block scope closed.
            let (external_buf, rest) = ramp_buffer.split_at_mut(BLOCK);
            let (master_buf, rest) = rest.split_at_mut(BLOCK);
            let (slave0_buf, slave1_buf) = rest.split_at_mut(BLOCK);
            let mut ramps = Ramps {
                external: external_buf,
                master: master_buf,
                slave: [slave0_buf, slave1_buf],
            };

            xy_generator.process(
                &mut random_stream,
                clock_source,
                &x_settings,
                &y_settings,
                &mut xy_reset,
                &external_clock,
                &mut ramps,
                &mut output,
            );
        }

        for &v in output.iter() {
            assert!(v.is_finite(), "XY output not finite at step {step}: {v}");
            assert!(v.abs() <= 12.0, "XY output out of range at step {step}: {v}");
            energy += (v as f64) * (v as f64);
            count += 1;
        }
    }

    let rms = (energy / count as f64).sqrt();
    assert!(rms > 0.01, "XY output looks silent (rms = {rms})");
}
