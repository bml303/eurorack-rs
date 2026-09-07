//! Pass an engine name (e.g. `cargo run --example plaits_wav -p mi-plaits -- additive`)
//! to render that engine instead; `--all` renders one second of every engine.

use std::env;
use std::fs::File;
use std::io::{BufWriter, Write};

use plaits::{
    dsp::SAMPLE_RATE,
    voice::{Frame, Modulations, Patch, Voice},
};

const BLOCK: usize = 24;
const ENGINES: &[&str] = &[
    "virtualanalogvcf",
    "phasedistortion",
    "sixopa",
    "sixopb",
    "sixopc",
    "waveterrain",
    "stringmachine",
    "chiptune",
    "virtualanalog",
    "waveshaping",
    "fm",
    "grain",
    "additive",
    "wavetable",
    "chord",
    "speech",
    "swarm",
    "noise",
    "particle",
    "string",
    "modal",
    "bassdrum",
    "snaredrum",
    "hihat",
];

fn name_to_engine(name: &str) -> i32 {
    if name == ENGINES[1] {
        return 1;
    }
    if name == ENGINES[2] {
        return 2;
    }
    if name == ENGINES[3] {
        return 3;
    }
    if name == ENGINES[4] {
        return 4;
    }
    if name == ENGINES[5] {
        return 5;
    }
    if name == ENGINES[6] {
        return 6;
    }
    if name == ENGINES[7] {
        return 7;
    }
    if name == ENGINES[8] {
        return 8;
    }
    if name == ENGINES[9] {
        return 9;
    }
    if name == ENGINES[10] {
        return 10;
    }
    if name == ENGINES[11] {
        return 11;
    }
    if name == ENGINES[12] {
        return 12;
    }
    if name == ENGINES[13] {
        return 13;
    }
    if name == ENGINES[14] {
        return 14;
    }
    if name == ENGINES[15] {
        return 15;
    }
    if name == ENGINES[16] {
        return 16;
    }
    if name == ENGINES[17] {
        return 17;
    }
    if name == ENGINES[18] {
        return 18;
    }
    if name == ENGINES[19] {
        return 19;
    }
    if name == ENGINES[20] {
        return 20;
    }
    if name == ENGINES[21] {
        return 21;
    }
    if name == ENGINES[22] {
        return 22;
    }
    if name == ENGINES[23] {
        return 23;
    }
    return 0;
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--all") {
        for i in 0..ENGINES.len() {
            let engine = i as i32;
            let name = format!("plaits_{}.wav", ENGINES[i]);
            render_engine(engine, 1, &name);
            println!("wrote {name} ({engine}, 5s @ {SAMPLE_RATE} Hz)");
        }
        return;
    }

    let engine = match args.first() {
        Some(s) => name_to_engine(s),
        None => 0,
    };
    let name = format!("plaits_{}.wav", ENGINES[engine as usize]);
    render_engine(engine, 5, &name);
    println!("wrote {name} ({engine}, 5s @ {SAMPLE_RATE} Hz)");
}

fn render_engine(engine: i32, seconds: u32, path: &str) {
    let mut voice = Voice::new(BLOCK);
    let mut patch = Patch::default();
    let mut modulations = Modulations::default();
    let note = 48.0;

    voice.init();
    patch.engine = engine;
    patch.harmonics = 0.5;
    patch.timbre = 0.5;
    patch.morph = 0.5;
    modulations.trigger_patched = true;
    modulations.level_patched = true;
    patch.note = note;
    modulations.trigger = 1.0;
    modulations.level = 127.0 as f32 / 127.0;

    let total_blocks = SAMPLE_RATE as u32 * seconds / BLOCK as u32;
    let mut wav = WavWriter::create(path, SAMPLE_RATE as u32, total_blocks as usize * BLOCK);

    let mut out_buf: [f32; BLOCK] = [0.0; BLOCK];
    let mut aux_buf: [f32; BLOCK] = [0.0; BLOCK];
    let mut samples_l: [i16; BLOCK] = [0; BLOCK];
    let mut samples_r: [i16; BLOCK] = [0; BLOCK];
    let mut frame_buf: [Frame; BLOCK] = [Frame { out: 0, aux: 0 }; BLOCK];

    for _ in 0..total_blocks {
        voice.render_frames(
            &patch,
            &modulations,
            &mut out_buf,
            &mut aux_buf,
            &mut samples_l,
            &mut samples_r,
            &mut frame_buf,
        );
        wav.write_frames(&frame_buf);
    }
    wav.finish();
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

    fn write_frames(&mut self, frames: &[Frame]) {
        for f in frames {
            self.out.write_all(&f.out.to_le_bytes()).unwrap();
            self.out.write_all(&f.aux.to_le_bytes()).unwrap();
        }
    }

    fn finish(mut self) {
        self.out.flush().unwrap();
    }
}
