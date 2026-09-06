//! Rust side of the C<->Rust comparison. Mirrors `tools/clouds_compare.cc`
//! exactly: for every (playback mode, quality) pair it renders the same
//! deterministic parameter sweep over an arithmetic sawtooth input and dumps
//! `<out_dir>/<MM>.pcm` (raw little-endian interleaved stereo i16).
//!
//! Clouds is a floating-point port with no bit-exactness contract, but 10 of
//! the 12 dumps still come out byte-identical; `tools/wav_diff.py` reports the
//! maximum per-sample delta for the rest.
//!
//!   cargo run --release --example clouds_compare -p mi-clouds -- /tmp/rust_pcm
//!   # then build + run tools/clouds_compare.cc into /tmp/c_pcm and:
//!   python3 tools/wav_diff.py /tmp/c_pcm /tmp/rust_pcm

use std::env;
use std::fs;
use std::io::Write;

use clouds::{GranularProcessor, PlaybackMode, ShortFrame};
use stmlib::Random;

// A plain arithmetic sawtooth -- broadband test material with no
// transcendental function, so the C and Rust inputs are bit-identical and
// any output delta is the DSP port's, not `sinf`'s.

const BLOCK: usize = 32;
const BLOCKS: usize = 1500;
/// The firmware runs `Prepare()` in a tight `while (1)` loop between audio
/// blocks; the WSOLA correlator search is spread across those calls. Mirror
/// that with a fixed budget per block so Stretch mode actually produces
/// sound.
const PREPARE_ITERS: usize = 32;

const MODES: [PlaybackMode; 3] = [
    PlaybackMode::Granular,
    PlaybackMode::Stretch,
    PlaybackMode::LoopingDelay,
];

fn tri(x: f32) -> f32 {
    let x = (x - x.floor()).abs();
    if x < 0.5 {
        x * 2.0
    } else {
        2.0 - x * 2.0
    }
}

fn main() {
    let out_dir = env::args().nth(1).unwrap_or_else(|| ".".to_string());
    fs::create_dir_all(&out_dir).unwrap();

    for (mode_idx, mode) in MODES.into_iter().enumerate() {
        for quality in 0..4i32 {
            Random::seed(0x21);
            let mut gp = GranularProcessor::new();
            gp.set_playback_mode(mode);
            gp.set_quality(quality);
            for _ in 0..PREPARE_ITERS {
                gp.prepare();
            }

            let mut phase = 0.0f32;
            let mut bytes = Vec::with_capacity(BLOCKS * BLOCK * 4);
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
                for _ in 0..PREPARE_ITERS {
                    gp.prepare();
                }

                for frame in output {
                    bytes.extend_from_slice(&frame.l.to_le_bytes());
                    bytes.extend_from_slice(&frame.r.to_le_bytes());
                }
            }

            let idx = mode_idx * 4 + quality as usize;
            let path = format!("{out_dir}/{idx:02}.pcm");
            fs::File::create(&path).unwrap().write_all(&bytes).unwrap();
        }
    }
    println!("wrote {} dumps to {out_dir}", MODES.len() * 4);
}
