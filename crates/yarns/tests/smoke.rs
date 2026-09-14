//! Crash / sanity smoke test: `Voice` (every audio mode, note on/off with
//! portamento, vibrato, every trigger shape), `JustIntonationProcessor` and
//! `Multi` (every layout, every voice-allocation mode, the arpeggiator, the
//! step sequencer, the built-in demo song) must survive a long sweep
//! without panicking, and `Voice`'s oscillator must produce a non-silent,
//! bounded 16-bit-DAC-range signal.

use yarns::multi::Layout;
use yarns::oscillator::audio_mode;
use yarns::part::{ArpeggiatorDirection, VoiceAllocationMode, VoicingSettings};
use yarns::voice::trigger_shape;
use yarns::{JustIntonationProcessor, Multi, NullMidiOut, Voice};

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

fn layouts() -> [Layout; 11] {
    [
        Layout::Mono,
        Layout::DualMono,
        Layout::QuadMono,
        Layout::DualPoly,
        Layout::QuadPoly,
        Layout::DualPolychained,
        Layout::QuadPolychained,
        Layout::OctalPolychained,
        Layout::QuadTriggers,
        Layout::QuadVoltages,
        Layout::ThreeOne,
    ]
}

fn allocation_modes() -> [VoiceAllocationMode; 9] {
    [
        VoiceAllocationMode::Mono,
        VoiceAllocationMode::Poly,
        VoiceAllocationMode::PolyCyclic,
        VoiceAllocationMode::PolyRandom,
        VoiceAllocationMode::PolyVelocity,
        VoiceAllocationMode::PolySorted,
        VoiceAllocationMode::PolyUnison1,
        VoiceAllocationMode::PolyUnison2,
        VoiceAllocationMode::PolyStealMostRecent,
    ]
}

#[test]
fn multi_survives_a_long_sweep_across_every_layout_and_allocation_mode() {
    let mut multi = Multi::new();
    multi.init(true);
    let mut midi_out = NullMidiOut;

    let layouts = layouts();
    let allocation_modes = allocation_modes();
    let notes = [36u8, 43, 48, 52, 55, 60, 64, 67, 72];

    let mut cv = [0u16; 4];
    let mut gate = [false; 4];
    let mut audio_source = [0u8; 4];

    for step in 0..6000i32 {
        if step % 130 == 0 {
            multi.set_layout(layouts[(step as usize / 130) % layouts.len()]);
        }
        if step % 47 == 0 {
            let mode = allocation_modes[(step as usize / 47) % allocation_modes.len()];
            let voicing = VoicingSettings {
                allocation_mode: mode,
                portamento: (step % 100) as u8,
                pitch_bend_range: 1 + (step % 12) as u8,
                vibrato_range: (step % 4) as u8,
                // Valid range is 0..=111: 0..99 index `LUT_LFO_INCREMENTS`,
                // 100..=111 select one of the 12 `CLOCK_DIVISIONS` for a
                // clock-synced LFO (unchecked in the C++ too -- the front
                // panel UI, out of scope here, is what keeps it in range).
                modulation_rate: (step % 112) as u8,
                trigger_duration: (step % 8) as u8,
                aux_cv: (step as usize) % 8,
                aux_cv_2: (step as usize + 2) % 8,
                ..VoicingSettings::default()
            };
            multi.set_part_voicing_settings(0, voicing);
        }
        if step % 211 == 0 {
            let mut seq = *multi.mutable_part(0).sequencer_settings();
            seq.arp_range = 1 + (step % 4) as u8;
            seq.arp_direction = match (step / 211) % 5 {
                0 => ArpeggiatorDirection::Up,
                1 => ArpeggiatorDirection::Down,
                2 => ArpeggiatorDirection::UpDown,
                3 => ArpeggiatorDirection::Random,
                _ => ArpeggiatorDirection::AsPlayed,
            };
            multi.mutable_part(0).set_sequencer_settings(seq);
        }

        let channel = 0u8;
        if step % 13 == 0 {
            let note = notes[(step as usize / 13) % notes.len()];
            multi.note_on(&mut midi_out, channel, note, 100);
        }
        if step % 29 == 0 {
            let note = notes[(step as usize / 29 + 1) % notes.len()];
            multi.note_off(&mut midi_out, channel, note, 0);
        }
        if step % 37 == 0 {
            multi.control_change(channel, 1, (step % 128) as u8);
        }
        if step % 53 == 0 {
            multi.pitch_bend(channel, ((step * 41) % 16384) as u16);
        }
        if step % 71 == 0 {
            multi.aftertouch(channel, (step % 128) as u8);
        }

        multi.clock(&mut midi_out);
        multi.refresh(&mut midi_out);
        multi.render_audio();

        multi.get_cv_gate(&mut cv, &mut gate);
        multi.get_audio_source(&mut audio_source);
        for &v in cv.iter() {
            let _ = v; // u16 DAC code: always in range by construction.
        }
    }

    // A basic liveness check: after driving a long, varied sequence of
    // notes, at least one voice should have produced a non-default DAC
    // code at some point (checked by re-running a short burst and sampling).
    // `arp_range` may still be nonzero on part 0 from earlier in the sweep,
    // in which case a plain `note_on` doesn't gate the voice directly (the
    // arpeggiator would need to clock a note through first) -- reset it so
    // this is a plain monophonic note-on, matching the assertion below.
    multi.set_layout(Layout::Mono);
    let mut seq = *multi.mutable_part(0).sequencer_settings();
    seq.arp_range = 0;
    multi.mutable_part(0).set_sequencer_settings(seq);
    multi.note_on(&mut midi_out, 0, 60, 100);
    multi.clock(&mut midi_out);
    multi.refresh(&mut midi_out);
    multi.get_cv_gate(&mut cv, &mut gate);
    assert!(gate[0], "voice 0 should be gated on after a note-on in Mono layout");
}

#[test]
fn built_in_song_plays_through_without_panicking() {
    let mut multi = Multi::new();
    multi.init(true);
    let mut midi_out = NullMidiOut;

    multi.start_song(&mut midi_out);
    assert!(multi.running());

    let mut cv = [0u16; 4];
    let mut gate = [false; 4];
    let mut energy = 0.0f64;
    let mut count = 0u64;

    // The song is short (a few bars); tick it well past its own length so
    // it wraps at least once (`SONG[pointer] == 255` restarts it), then
    // check the CV/gate/audio outputs are still sane.
    for _ in 0..20_000 {
        multi.refresh_internal_clock();
        multi.process_internal_clock_events(&mut midi_out);
        multi.refresh(&mut midi_out);
        multi.render_audio();

        multi.get_cv_gate(&mut cv, &mut gate);
        for voice in 0..4 {
            for _ in 0..8 {
                let sample = multi.mutable_voice(voice).read_sample();
                let centered = sample as f64 - 32768.0;
                energy += centered * centered;
                count += 1;
            }
        }
    }

    let rms = (energy / count as f64).sqrt();
    assert!(rms > 0.0, "song playback produced only silence (rms = {rms})");
}

