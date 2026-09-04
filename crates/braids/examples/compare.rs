//! Rust side of the C<->Rust equivalence check. Mirrors
//! `tools/braids_compare.cc` exactly: dumps `<out_dir>/NN.pcm` (raw LE i16) for
//! every macro shape under the same deterministic parameter sweep.
//!
//!   cargo run --release --example compare -p mi-braids -- /tmp/rust_pcm
//!   g++ -O2 -DTEST -I. -o /tmp/braids_compare tools/braids_compare.cc ...  # in C repo
//!   /tmp/braids_compare /tmp/c_pcm
//!   python3 tools/wav_diff.py /tmp/c_pcm /tmp/rust_pcm

use std::env;
use std::fs;
use std::io::Write;

use braids::{MacroOscillator, MacroOscillatorShape};
use stmlib::Random;

const BLOCK: usize = 24;
const BLOCKS: usize = 4000;

fn main() {
    let out_dir = env::args().nth(1).unwrap_or_else(|| ".".to_string());
    fs::create_dir_all(&out_dir).unwrap();

    for (shape_idx, shape) in MacroOscillatorShape::ALL.into_iter().enumerate() {
        Random::seed(0x21); // isolate each shape's RNG sequence
        let mut osc = MacroOscillator::new();
        osc.set_shape(shape);

        let mut bytes = Vec::with_capacity(BLOCKS * BLOCK * 2);
        for i in 0..BLOCKS {
            let mut sync = [0u8; BLOCK];
            if i % 37 == 0 {
                sync[i % BLOCK] = 1;
            }
            let p1 = ((i as i32) * 163) & 0x7fff;
            let p2 = ((i as i32) * 617) & 0x7fff;
            osc.set_parameters(p1 as i16, p2 as i16);
            // Cap at note 120: above ~127 the Fluted model, and at max timbre
            // the WaveLine model, do out-of-bounds table reads in the C (latent
            // Braids bugs). This port substitutes safe in-range reads there.
            let pitch = (((24 << 7) + i as i32 * 17).min(120 << 7)) as i16;
            osc.set_pitch(pitch);
            if i % 128 == 0 {
                osc.strike();
            }
            let mut block = [0i16; BLOCK];
            osc.render(&sync, &mut block, BLOCK);
            for s in block {
                bytes.extend_from_slice(&s.to_le_bytes());
            }
        }
        let path = format!("{out_dir}/{shape_idx:02}.pcm");
        fs::File::create(&path).unwrap().write_all(&bytes).unwrap();
    }
    println!(
        "wrote {} shape dumps to {out_dir}",
        MacroOscillatorShape::COUNT
    );
}
