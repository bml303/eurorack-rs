//! Golden-checksum equivalence test.
//!
//! `CRC32[i]` is the CRC-32 of 96 000 rendered samples (raw LE i16) for macro
//! shape `i`, produced by the **C firmware DSP** (`braids/{analog,digital,macro}
//! _oscillator.cc` at commit 08460a6) via `tools/braids_compare.cc`, under the
//! exact parameter sweep this test replays.
//!
//! 47 of 48 shapes are bit-identical. `WaveLine` (39) is exact except for a
//! single render block at maximum timbre, where the C does an out-of-bounds read
//! of `wave_line[64]` (a latent Braids bug, result depends on `.rodata`
//! layout); this port clamps to the last valid entry there.
//!
//! Regenerate after an intentional change:
//!   g++ -O2 -DTEST -I. -o /tmp/bc ../eurorack-rs/tools/braids_compare.cc \
//!       braids/{analog,digital,macro}_oscillator.cc braids/resources.cc \
//!       stmlib/utils/random.cc && /tmp/bc /tmp/c && \
//!   python3 -c "import zlib;[print(hex(zlib.crc32(open(f'/tmp/c/{i:02}.pcm','rb').read()))) for i in range(48)]"

use braids::{MacroOscillator, MacroOscillatorShape};
use stmlib::Random;

const BLOCK: usize = 24;
const BLOCKS: usize = 4000;

#[rustfmt::skip]
const CRC32: [u32; 48] = [
    0xc6a3f1a5, 0xe1834ec4, 0xba2d9351, 0x581a72bf, 0x3f371b11, 0x528af456, 0x11582c1b, 0x61b78318,
    0xb1d40410, 0x5d46afc7, 0xa9321e73, 0x2875b6af, 0x84ae5193, 0xbc2b3b9f, 0x7086ff61, 0x11596b99,
    0x52b9bd95, 0x48a33fda, 0x053b5974, 0x78d3910e, 0x8e21bc97, 0x36448a92, 0xc823d8be, 0xc301f1a9,
    0x24c9cda2, 0x13d56df3, 0xea96876d, 0x57eae365, 0x0d3af988, 0xcaa4e6d7, 0x15011400, 0x3d28291f,
    0xaf166bee, 0xb6a23753, 0xf5002e8a, 0x842a6f67, 0xea655df0, 0x21fd259b, 0x613a9a88, 0x2441ef18,
    0x8c91665c, 0x29b2b1be, 0x1f25f0e0, 0x528bf9a7, 0xdf182274, 0x53dbd831, 0x59bd9a1b, 0xaaf90e74,
];

/// Shapes whose C reference contains a layout-dependent out-of-bounds read that
/// this port deliberately doesn't reproduce.
const KNOWN_DEVIATIONS: &[usize] = &[MacroOscillatorShape::WaveLine as usize];

fn crc32(bytes: &[u8]) -> u32 {
    // Plain CRC-32 (IEEE), no external crate.
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

fn render_shape(shape: MacroOscillatorShape) -> Vec<u8> {
    Random::seed(0x21);
    let mut osc = MacroOscillator::new();
    osc.set_shape(shape);
    let mut out = Vec::with_capacity(BLOCKS * BLOCK * 2);
    for i in 0..BLOCKS {
        let mut sync = [0u8; BLOCK];
        if i % 37 == 0 {
            sync[i % BLOCK] = 1;
        }
        osc.set_parameters(
            (((i as i32) * 163) & 0x7fff) as i16,
            (((i as i32) * 617) & 0x7fff) as i16,
        );
        osc.set_pitch((((24 << 7) + i as i32 * 17).min(120 << 7)) as i16);
        if i % 128 == 0 {
            osc.strike();
        }
        let mut block = [0i16; BLOCK];
        osc.render(&sync, &mut block, BLOCK);
        for s in block {
            out.extend_from_slice(&s.to_le_bytes());
        }
    }
    out
}

#[test]
fn matches_c_firmware_dsp() {
    let mut failures = Vec::new();
    for (idx, shape) in MacroOscillatorShape::ALL.into_iter().enumerate() {
        if KNOWN_DEVIATIONS.contains(&idx) {
            continue;
        }
        let got = crc32(&render_shape(shape));
        if got != CRC32[idx] {
            failures.push(format!(
                "shape {idx} ({shape:?}): got {got:#010x}, want {:#010x}",
                CRC32[idx]
            ));
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}
