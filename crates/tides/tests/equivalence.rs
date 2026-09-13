//! Golden-checksum equivalence test.
//!
//! `CRC32[i]` is the CRC-32 of the 144 000-byte (16 000-sample x 3 x i16)
//! render for combination `i` of {range x mode x sync}, produced by the
//! **C firmware DSP** (`tides/generator.cc` at commit 08460a6) via
//! `tools/tides_compare.cc`, under the exact control/parameter sweep this test
//! replays (see `examples/compare.rs`, which shares the sweep logic).
//!
//! All 18 combinations are bit-identical.
//!
//! Regenerate after an intentional change:
//!   g++ -O2 -DTEST -I. -Istmlib -o /tmp/tc ../eurorack-rs/tools/tides_compare.cc \
//!       tides/generator.cc tides/resources.cc && /tmp/tc /tmp/c && \
//!   python3 -c "import zlib;[print(hex(zlib.crc32(open(f'/tmp/c/{i:02}.pcm','rb').read()))) for i in range(18)]"

use tides::{
    Generator, GeneratorMode, GeneratorRange, GeneratorSample, CONTROL_CLOCK, CONTROL_CLOCK_RISING,
    CONTROL_FREEZE, CONTROL_GATE, CONTROL_GATE_FALLING, CONTROL_GATE_RISING,
};

const BLOCK: usize = 16;
const BLOCKS: usize = 3000;

#[rustfmt::skip]
const CRC32: [u32; 18] = [
    0x2cb7e4ac, 0xea41ab8b, 0x0a20561b, 0x8da91ea6, 0x8435958e, 0xe331cafc,
    0x9d981b2a, 0x12dec439, 0xb8325dc8, 0x89759bc8, 0x1bca8d74, 0xaed0c216,
    0xceea482b, 0x12dec439, 0x27fc9c99, 0x89759bc8, 0x131b6ad4, 0xaed0c216,
];

fn control_at(n: u32) -> u8 {
    let period = 512u32;
    let clock_period = 37u32;
    let freeze_period = 733u32;
    let freeze_len = 13u32;

    let gate = (n % period) < (period / 4);
    let gate_prev = n != 0 && ((n - 1) % period) < (period / 4);
    let clock = (n % clock_period) < (clock_period / 2);
    let clock_prev = n != 0 && ((n - 1) % clock_period) < (clock_period / 2);
    let freeze = (n % freeze_period) < freeze_len;

    let mut control = 0u8;
    if freeze {
        control |= CONTROL_FREEZE;
    }
    if gate {
        control |= CONTROL_GATE;
    }
    if gate && !gate_prev {
        control |= CONTROL_GATE_RISING;
    }
    if !gate && gate_prev {
        control |= CONTROL_GATE_FALLING;
    }
    if clock {
        control |= CONTROL_CLOCK;
    }
    if clock && !clock_prev {
        control |= CONTROL_CLOCK_RISING;
    }
    control
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
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

fn render_combo(range: GeneratorRange, mode: GeneratorMode, sync: bool) -> Vec<u8> {
    let mut generator = Generator::new();
    generator.set_range(range);
    generator.set_mode(mode);
    generator.set_sync(sync);

    let mut bytes = Vec::with_capacity(BLOCKS * BLOCK * 3 * 2);
    let mut n = 0u32;
    for b in 0..BLOCKS {
        let b = b as i64;
        let shape = (((b * 163) & 0x7fff) - 16384) as i16;
        let slope = (((b * 617) & 0xffff) - 32768) as i16;
        let smoothness = (((b * 941) & 0xffff) - 32768) as i16;
        let pitch = ((24i64 << 7) + ((b * 37) % (72 * 128))) as i16;

        generator.set_shape(shape);
        generator.set_slope(slope);
        generator.set_smoothness(smoothness);
        generator.set_pitch(pitch);

        let control: [u8; BLOCK] = core::array::from_fn(|j| control_at(n + j as u32));
        let mut out = [GeneratorSample::default(); BLOCK];
        generator.render(&control, &mut out);

        for s in out {
            bytes.extend_from_slice(&s.bipolar.to_le_bytes());
            bytes.extend_from_slice(&(s.unipolar as i16).to_le_bytes());
            bytes.extend_from_slice(&(s.flags as i16).to_le_bytes());
        }
        n += BLOCK as u32;
    }
    bytes
}

#[test]
fn matches_c_firmware_dsp() {
    let ranges = [GeneratorRange::High, GeneratorRange::Medium, GeneratorRange::Low];
    let modes = [GeneratorMode::Ad, GeneratorMode::Looping, GeneratorMode::Ar];
    let syncs = [false, true];

    let mut failures = Vec::new();
    let mut idx = 0;
    for &range in &ranges {
        for &mode in &modes {
            for &sync in &syncs {
                let got = crc32(&render_combo(range, mode, sync));
                if got != CRC32[idx] {
                    failures.push(format!(
                        "combo {idx} ({range:?}, {mode:?}, sync={sync}): got {got:#010x}, want {:#010x}",
                        CRC32[idx]
                    ));
                }
                idx += 1;
            }
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}
