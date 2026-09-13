//! Numeric-tolerance C<->Rust check for the (floating-point, not bit-exact)
//! Tides2 port. Mirrors `tools/tides2_compare.cc`: dumps `<out_dir>/NN.f32`
//! (raw LE float32, 5 per frame: gate-high flag then the 4 output channels)
//! for every {ramp_mode x output_mode x range} combination, internal ramp
//! source only.
//!
//!   cargo run --release --example compare -p mi-tides2 -- /tmp/rust_f32
//!   g++ -O2 -DTEST -I. -Istmlib -o /tmp/tc tools/tides2_compare.cc \
//!       tides2/poly_slope_generator.cc tides2/resources.cc \
//!       tides2/ramp/ramp_extractor.cc   # in the C repo
//!   /tmp/tc /tmp/c_f32
//!   python3 tools/f32_diff.py /tmp/c_f32 /tmp/rust_f32

use std::env;
use std::fs;
use std::io::Write;

use stmlib::gate_flags::{extract_gate_flags, GateFlags};
use tides2::{OutputMode, PolySlopeGenerator, RampMode, Range};

const BLOCK: usize = 6;
const SAMPLE_RATE: f32 = 48000.0;
const SECONDS: usize = 4;

struct Pulse {
    total_duration: i32,
    on_duration: i32,
    num_repetitions: i32,
}

/// `tides2/test/fixtures.h`'s `PulseGenerator` -- test-only, not part of the
/// ported library.
#[derive(Default)]
struct PulseGenerator {
    counter: i32,
    previous_state: GateFlags,
    pulses: Vec<Pulse>,
}

impl PulseGenerator {
    fn add_pulses(&mut self, total_duration: i32, on_duration: i32, num_repetitions: i32) {
        self.pulses.push(Pulse { total_duration, on_duration, num_repetitions });
    }

    fn create_test_pattern(&mut self) {
        self.add_pulses(6000, 1000, 16);
        self.add_pulses(6000, 3000, 6);
        self.add_pulses(12000, 1000, 6);
        self.add_pulses(12000, 6000, 6);
        self.add_pulses(24000, 1000, 3);
        self.add_pulses(24000, 12000, 3);
    }

    fn render(&mut self, out: &mut [GateFlags]) {
        for slot in out.iter_mut() {
            let current_state = !self.pulses.is_empty() && self.counter < self.pulses[0].on_duration;
            self.counter += 1;
            if !self.pulses.is_empty() && self.counter >= self.pulses[0].total_duration {
                self.counter = 0;
                self.pulses[0].num_repetitions -= 1;
                if self.pulses[0].num_repetitions == 0 {
                    self.pulses.remove(0);
                }
            }
            self.previous_state = extract_gate_flags(self.previous_state, current_state);
            *slot = self.previous_state;
        }
    }
}

fn main() {
    let out_dir = env::args().nth(1).unwrap_or_else(|| ".".to_string());
    fs::create_dir_all(&out_dir).unwrap();

    let ramp_modes = [RampMode::Ad, RampMode::Looping, RampMode::Ar];
    let output_modes = [OutputMode::Gates, OutputMode::Amplitude, OutputMode::SlopePhase, OutputMode::Frequency];
    let ranges = [Range::Control, Range::Audio];

    let mut combo = 0;
    for &ramp_mode in &ramp_modes {
        for &output_mode in &output_modes {
            for &range in &ranges {
                let mut pulses = PulseGenerator::default();
                if ramp_mode != RampMode::Looping {
                    pulses.create_test_pattern();
                } else {
                    pulses.add_pulses(SAMPLE_RATE as i32, 100, 10);
                }

                let mut poly_slope = PolySlopeGenerator::new();

                let mut bytes = Vec::new();
                let total_samples = (SAMPLE_RATE as usize) * SECONDS;
                let mut i = 0;
                while i < total_samples {
                    let mut gate_flags = [GateFlags::LOW; BLOCK];
                    pulses.render(&mut gate_flags);

                    let f0 = (if ramp_mode == RampMode::Looping { 0.5 * 220.0 } else { 4.0 }) / SAMPLE_RATE;
                    let range = if ramp_mode == RampMode::Looping { Range::Audio } else { range };

                    let mut out = [tides2::OutputSample::default(); BLOCK];
                    poly_slope.render(
                        ramp_mode, output_mode, range, f0, 0.3, 0.6, 0.4, 0.25, &gate_flags, None, &mut out, BLOCK,
                    );

                    for (j, sample) in out.iter().enumerate() {
                        let frame = [
                            if gate_flags[j].contains(GateFlags::HIGH) { 1.0f32 } else { 0.0 },
                            sample.channel[0],
                            sample.channel[1],
                            sample.channel[2],
                            sample.channel[3],
                        ];
                        for v in frame {
                            bytes.extend_from_slice(&v.to_le_bytes());
                        }
                    }
                    i += BLOCK;
                }

                let path = format!("{out_dir}/{combo:02}.f32");
                fs::File::create(&path).unwrap().write_all(&bytes).unwrap();
                combo += 1;
            }
        }
    }
    println!("wrote {combo} combination dumps to {out_dir}");
}
