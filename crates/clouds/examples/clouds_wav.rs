//! Render a few seconds of one playback mode to a 16-bit stereo WAV, for
//! auditioning the port.
//!
//!   cargo run --release --example clouds_wav -p mi-clouds -- granular
//!   cargo run --release --example clouds_wav -p mi-clouds -- stretch clouds.wav
//!
//! Modes: `granular` (default), `stretch`, `looping`, `spectral`.

use std::env;
use std::fs::File;
use std::io::{BufWriter, Write};

use clouds::{GranularProcessor, PlaybackMode, ShortFrame};

const SAMPLE_RATE: u32 = 32_000;
const BLOCK: usize = 32;
const SECONDS: u32 = 8;
const PREPARE_ITERS: usize = 32;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mode = match args.first().map(String::as_str) {
        Some("stretch") => PlaybackMode::Stretch,
        Some("looping") => PlaybackMode::LoopingDelay,
        Some("spectral") => PlaybackMode::Spectral,
        _ => PlaybackMode::Granular,
    };
    let path = args.get(1).map(String::as_str).unwrap_or("clouds.wav");

    let mut gp = GranularProcessor::new();
    gp.set_playback_mode(mode);
    gp.set_quality(0);
    for _ in 0..PREPARE_ITERS {
        gp.prepare();
    }

    let total_blocks = SAMPLE_RATE * SECONDS / BLOCK as u32;
    let mut wav = WavWriter::create(path, SAMPLE_RATE, total_blocks as usize * BLOCK);

    let mut phase = 0.0f32;
    for i in 0..total_blocks {
        let t = i as f32 / total_blocks as f32;
        {
            let p = gp.mutable_parameters();
            p.position = 0.5 - 0.4 * (t * std::f32::consts::TAU).sin();
            p.size = 0.5;
            p.pitch = 7.0 * (t * 3.0).sin();
            p.density = 0.6;
            p.texture = 0.5;
            p.dry_wet = 1.0;
            p.stereo_spread = 0.4;
            p.feedback = 0.0;
            p.reverb = 0.2;
            p.freeze = (i / 128) % 4 == 3;
            p.trigger = i % 32 == 0;
        }

        let mut input = [ShortFrame::default(); BLOCK];
        for frame in input.iter_mut() {
            phase += 160.0 / SAMPLE_RATE as f32;
            if phase >= 1.0 {
                phase -= 1.0;
            }
            let s = ((phase - 0.5) * 18000.0) as i16;
            frame.l = s;
            frame.r = s;
        }
        let mut output = [ShortFrame::default(); BLOCK];
        gp.process(&input, &mut output);
        for _ in 0..PREPARE_ITERS {
            gp.prepare();
        }
        wav.write(&output);
    }
    wav.finish();
    println!("wrote {path} ({mode:?}, {SECONDS}s @ {SAMPLE_RATE} Hz stereo)");
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
        out.write_all(&1u16.to_le_bytes()).unwrap(); // PCM
        out.write_all(&2u16.to_le_bytes()).unwrap(); // stereo
        out.write_all(&sample_rate.to_le_bytes()).unwrap();
        out.write_all(&(sample_rate * 4).to_le_bytes()).unwrap();
        out.write_all(&4u16.to_le_bytes()).unwrap();
        out.write_all(&16u16.to_le_bytes()).unwrap();
        out.write_all(b"data").unwrap();
        out.write_all(&data_len.to_le_bytes()).unwrap();
        Self { out }
    }

    fn write(&mut self, frames: &[ShortFrame]) {
        for f in frames {
            self.out.write_all(&f.l.to_le_bytes()).unwrap();
            self.out.write_all(&f.r.to_le_bytes()).unwrap();
        }
    }

    fn finish(mut self) {
        self.out.flush().unwrap();
    }
}
