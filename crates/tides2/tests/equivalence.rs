//! Golden-checksum check against the C DSP.
//!
//! Tides2 is floating-point (Cortex-M4F, hardware FPU) so this port doesn't
//! *claim* bit-exactness the way `mi-tides`/`mi-braids` do (see
//! `crates/tides2/PORTING.md`) -- but for the sweep this test replays
//! (internal ramp source, `tides2/test/tides_test.cc`'s
//! `TestPolySlopeGenerator` pattern, extended with an explicit `Range` sweep),
//! it happens to come out bit-identical to the C on this toolchain, and CRC-32
//! is a cheap way to keep it that way. A future toolchain/target producing
//! slightly different rounding wouldn't be a bug -- see
//! `crates/tides2/PORTING.md` before "fixing" a failure here.
//!
//! `CRC32[i]` is the CRC-32 of combination `i`'s 3 840 000-byte (160 000
//! frames x 5 x f32) render, produced by the **C firmware DSP**
//! (`tides2/poly_slope_generator.cc` at commit 08460a6) via
//! `tools/tides2_compare.cc`, under the exact sweep this test replays (shared,
//! by construction not by code, with `examples/compare.rs`).
//!
//! Regenerate after an intentional change:
//!   g++ -O2 -DTEST -I. -Istmlib -o /tmp/t2c ../eurorack-rs/tools/tides2_compare.cc \
//!       tides2/poly_slope_generator.cc tides2/resources.cc \
//!       tides2/ramp/ramp_extractor.cc && /tmp/t2c /tmp/c && \
//!   python3 -c "import zlib;[print(hex(zlib.crc32(open(f'/tmp/c/{i:02}.f32','rb').read()))) for i in range(24)]"

use stmlib::gate_flags::{extract_gate_flags, GateFlags};
use tides2::{OutputMode, OutputSample, PolySlopeGenerator, RampMode, Range};

const BLOCK: usize = 6;
const SAMPLE_RATE: f32 = 48000.0;
const SECONDS: usize = 4;

#[rustfmt::skip]
const CRC32: [u32; 24] = [
    0x4b12e8b4, 0x6a85e53d, 0x328af749, 0x212a265c, 0xffbb6603, 0xbe9ded4d, 0x9e104017, 0x9af74b48,
    0xe6f38b3b, 0xe6f38b3b, 0x75a7228e, 0x75a7228e, 0x4c3919f3, 0x4c3919f3, 0x1233df87, 0x1233df87,
    0xd51eb2cf, 0xf23713d0, 0xc7945f61, 0xacb9c74a, 0xfc3c475e, 0xebd112bb, 0xe13b319c, 0x764eac79,
];

struct Pulse {
    total_duration: i32,
    on_duration: i32,
    num_repetitions: i32,
}

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

fn crc32(bytes: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        table[i] = c;
        i += 1;
    }
    let mut crc = 0xFFFF_FFFFu32;
    for &b in bytes {
        crc = table[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

fn render_combo(ramp_mode: RampMode, output_mode: OutputMode, range: Range) -> Vec<u8> {
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

        let mut out = [OutputSample::default(); BLOCK];
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
    bytes
}

#[test]
fn matches_c_firmware_dsp() {
    let ramp_modes = [RampMode::Ad, RampMode::Looping, RampMode::Ar];
    let output_modes =
        [OutputMode::Gates, OutputMode::Amplitude, OutputMode::SlopePhase, OutputMode::Frequency];
    let ranges = [Range::Control, Range::Audio];

    let mut failures = Vec::new();
    let mut idx = 0;
    for &ramp_mode in &ramp_modes {
        for &output_mode in &output_modes {
            for &range in &ranges {
                let got = crc32(&render_combo(ramp_mode, output_mode, range));
                if got != CRC32[idx] {
                    failures.push(format!(
                        "combo {idx} ({ramp_mode:?}, {output_mode:?}, {range:?}): got {got:#010x}, want {:#010x}",
                        CRC32[idx]
                    ));
                }
                idx += 1;
            }
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}
