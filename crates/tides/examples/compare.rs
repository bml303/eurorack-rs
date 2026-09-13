//! Rust side of the C<->Rust equivalence check. Mirrors `tools/tides_compare.cc`
//! exactly: dumps `<out_dir>/NN.pcm` (raw LE i16 triples: bipolar, unipolar bit
//! pattern, flags) for every {range x mode x sync} combination under the same
//! deterministic control/parameter sweep.
//!
//!   cargo run --release --example compare -p mi-tides -- /tmp/rust_pcm
//!   g++ -O2 -DTEST -I. -Istmlib -o /tmp/tides_compare tools/tides_compare.cc \
//!       tides/generator.cc tides/resources.cc   # in the C repo
//!   /tmp/tides_compare /tmp/c_pcm
//!   python3 tools/wav_diff.py /tmp/c_pcm /tmp/rust_pcm

use std::env;
use std::fs;
use std::io::Write;

use tides::{
    Generator, GeneratorMode, GeneratorRange, CONTROL_CLOCK, CONTROL_CLOCK_RISING,
    CONTROL_FREEZE, CONTROL_GATE, CONTROL_GATE_FALLING, CONTROL_GATE_RISING,
};

const BLOCK: usize = 16;
const BLOCKS: usize = 3000;

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

fn main() {
    let out_dir = env::args().nth(1).unwrap_or_else(|| ".".to_string());
    fs::create_dir_all(&out_dir).unwrap();

    let ranges = [GeneratorRange::High, GeneratorRange::Medium, GeneratorRange::Low];
    let modes = [GeneratorMode::Ad, GeneratorMode::Looping, GeneratorMode::Ar];
    let syncs = [false, true];

    let mut combo = 0;
    for &range in &ranges {
        for &mode in &modes {
            for &sync in &syncs {
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

                    let control: [u8; BLOCK] =
                        core::array::from_fn(|j| control_at(n + j as u32));
                    let mut out = [tides::GeneratorSample::default(); BLOCK];
                    generator.render(&control, &mut out);

                    for s in out {
                        bytes.extend_from_slice(&s.bipolar.to_le_bytes());
                        bytes.extend_from_slice(&(s.unipolar as i16).to_le_bytes());
                        bytes.extend_from_slice(&(s.flags as i16).to_le_bytes());
                    }
                    n += BLOCK as u32;
                }

                let path = format!("{out_dir}/{combo:02}.pcm");
                fs::File::create(&path).unwrap().write_all(&bytes).unwrap();
                combo += 1;
            }
        }
    }
    println!("wrote {combo} combination dumps to {out_dir}");
}
