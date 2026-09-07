//! Render ~12 s of Elements to a 16-bit stereo WAV, mirroring the C's
//! `elements_test.cc::TestPart` so the two can be diffed by ear or with
//! `tools/wav_diff.py`.
//!
//!   cargo run --release --example elements_wav -p mi-elements -- [modal|string|strings|ominous] [out.wav]

use std::env;
use std::fs::File;
use std::io::{BufWriter, Write};

use elements::{Part, PerformanceState, ResonatorModel};

const SAMPLE_RATE: u32 = 32_000;
const BLOCK: usize = 16;
const SECONDS: u32 = 12;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let (model, easter_egg) = match args.first().map(String::as_str) {
        Some("string") => (ResonatorModel::String, false),
        Some("strings") => (ResonatorModel::Strings, false),
        Some("ominous") => (ResonatorModel::Modal, true),
        _ => (ResonatorModel::Modal, false),
    };
    let path = args.get(1).map(String::as_str).unwrap_or("elements.wav");

    let mut part = Part::new();
    part.set_resonator_model(model);
    part.set_easter_egg(easter_egg);
    {
        let p = part.patch_mut();
        p.exciter_envelope_shape = 0.0;
        p.exciter_bow_level = 0.0;
        p.exciter_bow_timbre = 0.0;
        p.exciter_blow_level = 0.0;
        p.exciter_blow_meta = 0.0;
        p.exciter_blow_timbre = 0.0;
        p.exciter_strike_level = 0.5;
        p.exciter_strike_meta = 0.5;
        p.exciter_strike_timbre = 0.3;
        p.resonator_geometry = 0.4;
        p.resonator_brightness = 0.7;
        p.resonator_damping = 0.8;
        p.resonator_position = 0.3;
        p.space = 0.1;
    }

    let sequence = [69.0f32, 57.0, 45.0, 57.0, 69.0];
    let total = SAMPLE_RATE * SECONDS;
    let mut wav = WavWriter::create(path, SAMPLE_RATE, total as usize);

    let blow_in = [0.0f32; BLOCK];
    let strike_in = [0.0f32; BLOCK];

    let mut i = 0u32;
    while i < total {
        let seq = ((i / (SAMPLE_RATE * 2)) % 5) as usize;
        let perf = PerformanceState {
            gate: (i % SAMPLE_RATE) < SAMPLE_RATE / 2,
            note: sequence[seq] - 12.0,
            modulation: 0.0,
            strength: 0.5,
        };

        let mut main = [0.0f32; BLOCK];
        let mut aux = [0.0f32; BLOCK];
        part.process(&perf, &blow_in, &strike_in, &mut main, &mut aux, BLOCK);
        wav.write(&main, &aux);
        i += BLOCK as u32;
    }
    wav.finish();
    println!(
        "wrote {path} ({model:?}, easter_egg={easter_egg}, {SECONDS}s @ {SAMPLE_RATE} Hz stereo)"
    );
}

/// Minimal 16-bit stereo PCM WAV writer.
struct WavWriter {
    out: BufWriter<File>,
}

impl WavWriter {
    fn create(path: &str, sample_rate: u32, frames: usize) -> Self {
        let mut out = BufWriter::new(File::create(path).expect("create wav"));
        let data_len = (frames * 4) as u32;
        out.write_all(b"RIFF").unwrap();
        out.write_all(&(36 + data_len).to_le_bytes()).unwrap();
        out.write_all(b"WAVE").unwrap();
        out.write_all(b"fmt ").unwrap();
        out.write_all(&16u32.to_le_bytes()).unwrap();
        out.write_all(&1u16.to_le_bytes()).unwrap();
        out.write_all(&2u16.to_le_bytes()).unwrap();
        out.write_all(&sample_rate.to_le_bytes()).unwrap();
        out.write_all(&(sample_rate * 4).to_le_bytes()).unwrap();
        out.write_all(&4u16.to_le_bytes()).unwrap();
        out.write_all(&16u16.to_le_bytes()).unwrap();
        out.write_all(b"data").unwrap();
        out.write_all(&data_len.to_le_bytes()).unwrap();
        Self { out }
    }

    fn write(&mut self, left: &[f32], right: &[f32]) {
        for (l, r) in left.iter().zip(right.iter()) {
            let to_i16 = |x: f32| (x.clamp(-1.0, 1.0) * 32767.0) as i16;
            self.out.write_all(&to_i16(*l).to_le_bytes()).unwrap();
            self.out.write_all(&to_i16(*r).to_le_bytes()).unwrap();
        }
    }

    fn finish(mut self) {
        self.out.flush().unwrap();
    }
}
