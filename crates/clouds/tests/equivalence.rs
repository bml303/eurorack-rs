//! Regression lock for the DSP output.
//!
//! Clouds is floating point with no bit-exactness contract, so this test does
//! not link the C. Instead it renders the exact sweep of
//! `examples/compare.rs` / `tools/clouds_compare.cc` and checks a hash of
//! every (mode, quality) dump against a frozen value, so an accidental change
//! to the DSP is caught in CI without the C toolchain.
//!
//! The real fidelity check is manual: build `tools/clouds_compare.cc`, run
//! both renderers, and diff with `tools/wav_diff.py` (see `PORTING.md`). As of
//! the port, 13 of 16 dumps are bit-identical to the C firmware DSP (all of
//! Granular and Spectral), 2 more differ by <= 1 LSB on <= 2 of 96000
//! samples, and mono Stretch diverges into a different-but-valid WSOLA splice
//! late in the run.

use clouds::{GranularProcessor, PlaybackMode, ShortFrame};
use stmlib::Random;

const BLOCK: usize = 32;
const BLOCKS: usize = 1500;

const MODES: [PlaybackMode; 4] = [
    PlaybackMode::Granular,
    PlaybackMode::Stretch,
    PlaybackMode::LoopingDelay,
    PlaybackMode::Spectral,
];

fn tri(x: f32) -> f32 {
    let x = (x - x.floor()).abs();
    if x < 0.5 {
        x * 2.0
    } else {
        2.0 - x * 2.0
    }
}

fn fnv1a(seed: u64, bytes: &[u8]) -> u64 {
    let mut h = seed;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

fn render(mode: PlaybackMode, quality: i32) -> u64 {
    Random::seed(0x21);
    let mut gp = GranularProcessor::new();
    gp.set_playback_mode(mode);
    gp.set_quality(quality);
    gp.prepare();

    let mut phase = 0.0f32;
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for i in 0..BLOCKS {
        let t = i as f32 / BLOCKS as f32;
        {
            let p = gp.mutable_parameters();
            p.position = tri(t * 2.0);
            p.size = 0.2 + 0.6 * tri(t * 3.0 + 0.1);
            p.pitch = -7.0 + 14.0 * tri(t * 1.3);
            p.density = 0.3 + 0.5 * tri(t * 2.5 + 0.3);
            p.texture = tri(t * 1.7 + 0.5);
            p.dry_wet = 1.0;
            p.stereo_spread = 0.5;
            p.feedback = 0.0;
            p.reverb = 0.0;
            p.freeze = (i / 200) % 3 == 2;
            p.trigger = i % 48 == 0;
            p.gate = false;
        }

        let mut input = [ShortFrame::default(); BLOCK];
        for frame in input.iter_mut() {
            phase += 220.0 / 32000.0;
            if phase >= 1.0 {
                phase -= 1.0;
            }
            let s = ((phase - 0.5) * 24000.0) as i16;
            frame.l = s;
            frame.r = s;
        }
        let mut output = [ShortFrame::default(); BLOCK];
        gp.process(&input, &mut output);
        gp.prepare();

        for frame in output {
            hash = fnv1a(hash, &frame.l.to_le_bytes());
            hash = fnv1a(hash, &frame.r.to_le_bytes());
        }
    }
    hash
}

#[test]
fn dsp_output_is_unchanged() {
    // mode-major: [granular q0..3, stretch q0..3, looping q0..3, spectral q0..3]
    const EXPECTED: [u64; 16] = [
        0xf73e2c87f045dcf3,
        0x154a3a7f4073f8e2,
        0x220a9248f375cc95,
        0x15d59487c0acceb1,
        0x1a68aa060bd28a73,
        0xd657ce98967730fc,
        0xf3d9ebc1ce3980a7,
        0xf3d9ebc1ce3980a7,
        0xb0c8b1b0df5a3041,
        0xfdf87447a81061dc,
        0x2cd00776d98a04ce,
        0x424016a15d6d2298,
        0x8ba03dfa8961f280,
        0x2c2e213ff47525e1,
        0xf9a7fd762a09226a,
        0xeac6ab9df52c94a1,
    ];

    let mut actual = [0u64; 16];
    for (mode_idx, mode) in MODES.into_iter().enumerate() {
        for quality in 0..4usize {
            actual[mode_idx * 4 + quality] = render(mode, quality as i32);
        }
    }

    assert_eq!(
        actual, EXPECTED,
        "\nDSP output changed. If intentional, update EXPECTED to:\n{actual:#018x?}"
    );
}
