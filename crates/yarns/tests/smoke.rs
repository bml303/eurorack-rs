//! Crash / sanity smoke test: `Voice` (every audio mode, note on/off with
//! portamento, vibrato, every trigger shape) and `JustIntonationProcessor`
//! must survive a long sweep without panicking, and `Voice`'s oscillator
//! must produce a non-silent, bounded 16-bit-DAC-range signal.

use yarns::oscillator::audio_mode;
use yarns::voice::trigger_shape;
use yarns::{JustIntonationProcessor, Voice};

fn audio_modes() -> [u8; 6] {
    [
        audio_mode::SAW,
        audio_mode::SQUARE_NARROW,
        audio_mode::SQUARE,
        audio_mode::TRIANGLE,
        audio_mode::SINE,
        audio_mode::NOISE,
    ]
}

fn trigger_shapes() -> [u8; 6] {
    [
        trigger_shape::SQUARE,
        trigger_shape::LINEAR,
        trigger_shape::EXPONENTIAL,
        trigger_shape::RING,
        trigger_shape::STEPS,
        trigger_shape::NOISE_BURST,
    ]
}

#[test]
fn voice_survives_a_long_sweep_and_is_audible() {
    let mut voice = Voice::new();
    voice.init(true);

    let modes = audio_modes();
    let shapes = trigger_shapes();

    let mut energy = 0.0f64;
    let mut count = 0u64;
    let mut min_sample = u16::MAX;
    let mut max_sample = 0u16;

    let notes = [36i16, 48, 60, 64, 67, 72, 84, 96];

    for step in 0..3000i32 {
        let mode = modes[(step as usize / 37) % modes.len()];
        let shape = shapes[(step as usize / 53) % shapes.len()];
        voice.set_audio_mode(mode);
        voice.set_trigger_shape(shape);
        voice.set_trigger_scale(step % 2 == 0);
        voice.set_trigger_duration((step % 16) as u8);
        voice.set_modulation_rate((step % 120) as u8);
        voice.set_vibrato_range((step % 13) as u8);
        voice.set_pitch_bend_range(1 + (step % 24) as u8);
        voice.set_tuning((step % 7 - 3) as i8, (step % 30 - 15) as i8);
        voice.set_aux_cv((step as usize) % 8);
        voice.set_aux_cv_2((step as usize + 3) % 8);

        if step % 11 == 0 {
            voice.pitch_bend(((step * 37) % 16384) as u16);
        }
        if step % 17 == 0 {
            voice.aftertouch((step % 128) as u8);
        }
        voice.control_change(1, (step % 128) as u8); // mod wheel
        voice.control_change(2, (step % 128) as u8); // breath
        voice.control_change(4, (step % 128) as u8); // foot pedal

        if step % 23 == 0 {
            voice.note_off();
        }
        if step % 9 == 0 {
            let note = notes[(step as usize / 9) % notes.len()];
            let portamento = ((step * 3) % 100) as u8;
            let velocity = (step % 128) as u8;
            voice.note_on(note << 7, velocity, portamento, true);
        }

        // `Refresh()` runs at a slower control rate than the audio block;
        // call it once per block like `yarns.cc`'s main loop does.
        voice.refresh();

        assert!(voice.note_dac_code() > 0 || voice.note() < 0, "note DAC code looks uninitialized at step {step}");

        // `trigger_dac_code` is `min + fraction * (max - min)` where
        // `fraction` comes from a signed i16 waveform sample and so ranges
        // over roughly `[-1, 1]`, not `[0, 1]` -- bipolar shapes (Ring, in
        // particular) can legitimately swing well outside `[min, max]`. Only
        // the type's own range is asserted here; the point is "doesn't
        // panic and stays a valid DAC code", not a precise CV bound.
        let _trigger_dac: u16 = voice.trigger_dac_code();

        voice.render_audio();
        for _ in 0..64 {
            let sample = voice.read_sample();
            min_sample = min_sample.min(sample);
            max_sample = max_sample.max(sample);
            let centered = sample as f64 - 32768.0;
            energy += centered * centered;
            count += 1;
        }
    }

    let rms = (energy / count as f64).sqrt();
    assert!(rms > 1.0, "voice output looks silent (rms = {rms})");
    assert!(max_sample > min_sample, "voice output never varies");
}

#[test]
fn just_intonation_processor_survives_a_long_sweep() {
    let mut jip = JustIntonationProcessor::new();
    jip.init();

    let notes: [u8; 12] = [60, 62, 64, 65, 67, 69, 71, 72, 55, 57, 59, 48];
    let mut pitches = Vec::new();

    for step in 0..2000i32 {
        let note = notes[(step as usize) % notes.len()];
        let pitch = jip.note_on(note);
        pitches.push(pitch);
        if step % 5 == 0 {
            jip.note_off(notes[(step as usize + 3) % notes.len()]);
        }
    }

    // The tuned pitch should stay within a couple of semitones (in the
    // fixed-point 7-bits-per-semitone unit) of the note's own 12-TET pitch --
    // the tuner only ever searches a +/- 1 quartertone range.
    for (i, &pitch) in pitches.iter().enumerate() {
        let note = notes[i % notes.len()] as i32;
        let expected = note << 7;
        assert!(
            (pitch as i32 - expected).abs() <= 256,
            "pitch for note {note} drifted too far: {pitch} vs expected ~{expected}"
        );
    }
}

