//! Host reproduction of `braids/test/braids_test.cc::TestAudioRendering`.
//!
//! Renders 5 seconds of `VOWEL_FOF` at 96 kHz with the exact same swept
//! parameter as the C test and writes `oscillator.wav`, so the two can be
//! byte-compared (`tools/wav_diff.py`).
//!
//! Pass a shape name (e.g. `cargo run --example render_wav -p mi-braids -- saw_square`)
//! to render that model instead; `--all` renders one second of every model.

use std::env;
use std::fs::File;
use std::io::{BufWriter, Write};

use braids::{MacroOscillator, MacroOscillatorShape};

const SAMPLE_RATE: u32 = 96_000;
const BLOCK: usize = 24;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--all") {
        for shape in MacroOscillatorShape::ALL {
            let name = format!("braids_{}.wav", shape_slug(shape));
            render_one(shape, 1, &name);
            println!("wrote {name}");
        }
        return;
    }

    let shape = match args.first() {
        Some(s) => parse_shape(s).unwrap_or_else(|| {
            eprintln!("unknown shape `{s}`");
            std::process::exit(2);
        }),
        None => MacroOscillatorShape::VowelFof,
    };
    render_one(shape, 5, "oscillator.wav");
    println!("wrote oscillator.wav ({shape:?}, 5s @ {SAMPLE_RATE} Hz)");
}

fn render_one(shape: MacroOscillatorShape, seconds: u32, path: &str) {
    let mut osc = MacroOscillator::new();
    osc.set_shape(shape);

    let total_blocks = SAMPLE_RATE * seconds / BLOCK as u32;
    let mut wav = WavWriter::create(path, SAMPLE_RATE, (total_blocks as usize) * BLOCK);

    let sync = [0u8; BLOCK];
    let mut block = [0i16; BLOCK];
    for i in 0..total_blocks {
        // Same triangle sweep on parameter 1 as the C test.
        let mut tri = (i.wrapping_mul(3)) as u16;
        if tri > 32767 {
            tri = 65535 - tri;
        }
        osc.set_parameters(tri as i16, 0);
        osc.set_pitch(48 << 7);
        osc.render(&sync, &mut block, BLOCK);
        wav.write_frames(&block);
    }
    wav.finish();
}

fn parse_shape(s: &str) -> Option<MacroOscillatorShape> {
    MacroOscillatorShape::ALL
        .into_iter()
        .find(|sh| shape_slug(*sh) == s.to_ascii_lowercase())
}

fn shape_slug(shape: MacroOscillatorShape) -> String {
    let mut out = String::new();
    for (i, ch) in format!("{shape:?}").chars().enumerate() {
        if ch.is_ascii_uppercase() && i != 0 {
            out.push('_');
        }
        out.push(ch.to_ascii_lowercase());
    }
    out
}

/// Minimal 16-bit mono PCM WAV writer (matches `stmlib::WavWriter`'s header).
struct WavWriter {
    out: BufWriter<File>,
    data_bytes: u32,
}

impl WavWriter {
    fn create(path: &str, sample_rate: u32, frames: usize) -> Self {
        let mut out = BufWriter::new(File::create(path).expect("create wav"));
        let data_len = (frames * 2) as u32;
        out.write_all(b"RIFF").unwrap();
        out.write_all(&(36 + data_len).to_le_bytes()).unwrap();
        out.write_all(b"WAVE").unwrap();
        out.write_all(b"fmt ").unwrap();
        out.write_all(&16u32.to_le_bytes()).unwrap();
        out.write_all(&1u16.to_le_bytes()).unwrap(); // PCM
        out.write_all(&1u16.to_le_bytes()).unwrap(); // mono
        out.write_all(&sample_rate.to_le_bytes()).unwrap();
        out.write_all(&(sample_rate * 2).to_le_bytes()).unwrap();
        out.write_all(&2u16.to_le_bytes()).unwrap();
        out.write_all(&16u16.to_le_bytes()).unwrap();
        out.write_all(b"data").unwrap();
        out.write_all(&data_len.to_le_bytes()).unwrap();
        Self { out, data_bytes: 0 }
    }

    fn write_frames(&mut self, frames: &[i16]) {
        for &s in frames {
            self.out.write_all(&s.to_le_bytes()).unwrap();
            self.data_bytes += 2;
        }
    }

    fn finish(mut self) {
        self.out.flush().unwrap();
        let _ = self.data_bytes;
    }
}
