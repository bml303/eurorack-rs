//! Render a short phrase from one Edges oscillator to a 16-bit mono WAV.
//!
//!   cargo run --release --example edges_wav -p mi-edges -- \
//!       [triangle|nes_triangle|noise|nes_noise_long|nes_noise_short|sine|square] [out.wav]
//!
//! `square` drives a [`TimerOscillator`]; everything else drives the sampled
//! [`DigitalOscillator`].

use std::env;
use std::fs::File;
use std::io::{BufWriter, Write};

use edges::{AUDIO_BLOCK_SIZE, DigitalOscillator, OscillatorShape, PulseWidth, TimerOscillator};

const SAMPLE_RATE: u32 = 48_120;
const SECONDS: u32 = 6;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let which = args.first().map(String::as_str).unwrap_or("triangle");
    let path = args.get(1).map(String::as_str).unwrap_or("edges.wav");

    let shape = match which {
        "nes_triangle" => Some(OscillatorShape::NesTriangle),
        "noise" => Some(OscillatorShape::PitchedNoise),
        "nes_noise_long" => Some(OscillatorShape::NesNoiseLong),
        "nes_noise_short" => Some(OscillatorShape::NesNoiseShort),
        "sine" => Some(OscillatorShape::Sine),
        "square" => None,
        _ => Some(OscillatorShape::Triangle),
    };

    let total = SAMPLE_RATE * SECONDS;
    let mut wav = WavWriter::create(path, SAMPLE_RATE, total as usize);

    // A little arpeggio, one note every half second, with a gate 3/4 of the time.
    let sequence = [45i16, 52, 57, 64, 57, 52];
    let note_at = |sample: u32| sequence[(sample / (SAMPLE_RATE / 2)) as usize % sequence.len()];
    let gate_at = |sample: u32| (sample % (SAMPLE_RATE / 2)) < (SAMPLE_RATE * 3 / 8);

    match shape {
        Some(shape) => {
            let mut osc = DigitalOscillator::new();
            osc.set_cv_pw(210);
            let mut buf = [0u16; AUDIO_BLOCK_SIZE];
            let mut i = 0u32;
            while i < total {
                osc.update_pitch(note_at(i) << 7, shape);
                osc.gate(gate_at(i));
                osc.render(&mut buf);
                for &s in &buf {
                    wav.write_u12(s);
                }
                i += AUDIO_BLOCK_SIZE as u32;
            }
        }
        None => {
            let mut osc = TimerOscillator::new();
            let mut buf = [0u16; AUDIO_BLOCK_SIZE];
            let mut i = 0u32;
            while i < total {
                osc.update_pitch(note_at(i) << 7, PulseWidth::Pw75);
                osc.render_square(&mut buf, SAMPLE_RATE as f32, gate_at(i));
                for &s in &buf {
                    wav.write_u12(s);
                }
                i += AUDIO_BLOCK_SIZE as u32;
            }
        }
    }
    wav.finish();
    println!("wrote {path} ({which}, {SECONDS}s @ {SAMPLE_RATE} Hz mono)");
}

/// Minimal 16-bit mono PCM WAV writer.
struct WavWriter {
    out: BufWriter<File>,
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
        out.write_all(&1u16.to_le_bytes()).unwrap();
        out.write_all(&1u16.to_le_bytes()).unwrap();
        out.write_all(&sample_rate.to_le_bytes()).unwrap();
        out.write_all(&(sample_rate * 2).to_le_bytes()).unwrap();
        out.write_all(&2u16.to_le_bytes()).unwrap();
        out.write_all(&16u16.to_le_bytes()).unwrap();
        out.write_all(b"data").unwrap();
        out.write_all(&data_len.to_le_bytes()).unwrap();
        Self { out }
    }

    /// 12-bit unsigned (0..4095, mid 2048) -> 16-bit signed.
    fn write_u12(&mut self, s: u16) {
        let v = ((s as i32 - 2048) * 15).clamp(-32768, 32767) as i16;
        self.out.write_all(&v.to_le_bytes()).unwrap();
    }

    fn finish(mut self) {
        self.out.flush().unwrap();
    }
}
