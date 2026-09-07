//! Render ~16 s of Rings to a 16-bit stereo WAV, mirroring the C's
//! `rings_test.cc` (`TestModal` / `TestString` / `TestStringSynthPart`).
//!
//!   cargo run --release --example rings_wav -p mi-rings -- \
//!       [modal|sympathetic|string|fm|quantized|string_reverb|synth] [out.wav]

use std::env;
use std::fs::File;
use std::io::{BufWriter, Write};

use rings::{FxType, Part, Patch, PerformanceState, ResonatorModel, StringSynthPart};

const SAMPLE_RATE: u32 = 48_000;
const BLOCK: usize = 24;
const SECONDS: u32 = 16;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let which = args.first().map(String::as_str).unwrap_or("modal");
    let path = args.get(1).map(String::as_str).unwrap_or("rings.wav");

    let total = SAMPLE_RATE * SECONDS;
    let mut wav = WavWriter::create(path, SAMPLE_RATE, total as usize);
    let sequence = [69.0f32, 57.0, 45.0, 57.0, 69.0];

    if which == "synth" {
        let mut part = StringSynthPart::new();
        part.set_polyphony(2);
        part.set_fx(FxType::Ensemble);
        let mut patch = Patch {
            structure: 0.42,
            brightness: 0.3,
            damping: 0.8,
            position: 0.4,
        };
        let mut seq_counter = 0usize;
        let mut i = 0u32;
        while i < total {
            let strum = i.is_multiple_of(SAMPLE_RATE * 2);
            if strum {
                seq_counter = (seq_counter + 1) % sequence.len();
            }
            patch.brightness = 0.2 + 0.6 * (i as f32 / total as f32);
            let perf = PerformanceState {
                strum,
                internal_exciter: true,
                note: sequence[seq_counter] - 45.0,
                tonic: 43.0,
                chord: (i / (SAMPLE_RATE * 4)) as i32 % 11,
                ..Default::default()
            };
            let input = [0.0f32; BLOCK];
            let mut out = [0.0f32; BLOCK];
            let mut aux = [0.0f32; BLOCK];
            part.process(&perf, &patch, &input, &mut out, &mut aux, BLOCK);
            wav.write(&out, &aux);
            i += BLOCK as u32;
        }
    } else {
        let (model, polyphony, internal) = match which {
            "sympathetic" => (ResonatorModel::SympatheticString, 2, true),
            "string" => (ResonatorModel::String, 3, true),
            "fm" => (ResonatorModel::FmVoice, 1, true),
            "quantized" => (ResonatorModel::SympatheticStringQuantized, 2, true),
            "string_reverb" => (ResonatorModel::StringAndReverb, 2, true),
            _ => (ResonatorModel::Modal, 4, false),
        };
        let mut part = Part::new();
        part.set_model(model);
        part.set_polyphony(polyphony);
        let patch = Patch {
            structure: 0.25,
            brightness: 0.4,
            damping: 0.85,
            position: 0.7,
        };

        let mut seq_counter = 0usize;
        let mut impulse = 0.0f32;
        let mut i = 0u32;
        while i < total {
            let strum = i.is_multiple_of(SAMPLE_RATE / 2);
            if strum {
                seq_counter = (seq_counter + 1) % sequence.len();
                impulse = 1.0;
            }
            let perf = PerformanceState {
                strum,
                internal_exciter: internal,
                internal_note: true,
                note: sequence[seq_counter] - 24.0,
                tonic: 12.0,
                chord: (i / SAMPLE_RATE) as i32 % 11,
                ..Default::default()
            };
            let mut input = [0.0f32; BLOCK];
            if !internal {
                for s in input.iter_mut() {
                    *s = impulse;
                    impulse *= 0.99;
                }
            }
            let mut out = [0.0f32; BLOCK];
            let mut aux = [0.0f32; BLOCK];
            part.process(&perf, &patch, &input, &mut out, &mut aux, BLOCK);
            wav.write(&out, &aux);
            i += BLOCK as u32;
        }
    }

    wav.finish();
    println!("wrote {path} ({which}, {SECONDS}s @ {SAMPLE_RATE} Hz stereo)");
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
