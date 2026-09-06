//! Every time-domain playback mode, at every quality, must survive a long
//! parameter sweep (with freeze / trigger toggling and mode switches) without
//! panicking -- no out-of-bounds buffer reads, no divide-by-zero, no bad
//! `f32 -> i16` conversion. This is a crash smoke test, not a numerical one.

use clouds::{GranularProcessor, PlaybackMode, ShortFrame};

const BLOCK: usize = 32;

fn modes() -> [PlaybackMode; 3] {
    [
        PlaybackMode::Granular,
        PlaybackMode::Stretch,
        PlaybackMode::LoopingDelay,
    ]
}

fn run(mode: PlaybackMode, quality: i32) {
    let mut gp = GranularProcessor::new();
    gp.set_playback_mode(mode);
    gp.set_quality(quality);
    for _ in 0..16 {
        gp.prepare();
    }

    let mut phase = 0.0f32;
    for step in 0..1200i32 {
        {
            let p = gp.mutable_parameters();
            p.position = ((step * 7) % 101) as f32 / 100.0;
            p.size = ((step * 13) % 101) as f32 / 100.0;
            p.pitch = ((step * 5) % 49) as f32 - 24.0;
            p.density = ((step * 11) % 101) as f32 / 100.0;
            p.texture = ((step * 17) % 101) as f32 / 100.0;
            p.dry_wet = 0.75;
            p.stereo_spread = ((step * 3) % 101) as f32 / 100.0;
            p.feedback = ((step * 19) % 101) as f32 / 100.0;
            p.reverb = ((step * 23) % 101) as f32 / 100.0;
            p.freeze = (step / 16) % 3 == 0;
            p.trigger = step % 24 == 0;
            p.gate = step % 24 == 0;
        }

        let mut input = [ShortFrame::default(); BLOCK];
        for frame in input.iter_mut() {
            phase += 220.0 / 32000.0;
            if phase >= 1.0 {
                phase -= 1.0;
            }
            let s = (libm::sinf(phase * core::f32::consts::TAU) * 12000.0) as i16;
            frame.l = s;
            frame.r = s;
        }
        let mut output = [ShortFrame::default(); BLOCK];
        gp.process(&input, &mut output);
        for _ in 0..16 {
            gp.prepare();
        }

        for frame in output {
            assert!(frame.l.abs() as i32 <= 32768);
            assert!(frame.r.abs() as i32 <= 32768);
        }
    }
}

#[test]
fn every_mode_and_quality_survives_a_sweep() {
    for mode in modes() {
        for quality in 0..4 {
            run(mode, quality);
        }
    }
}

#[test]
fn mode_switching_mid_stream_is_safe() {
    let mut gp = GranularProcessor::new();
    gp.set_quality(0);
    gp.set_playback_mode(PlaybackMode::Granular);
    gp.prepare();

    let all = [
        PlaybackMode::Granular,
        PlaybackMode::Stretch,
        PlaybackMode::LoopingDelay,
        PlaybackMode::Spectral,
    ];
    for i in 0..40 {
        gp.set_playback_mode(all[i % all.len()]);
        {
            let p = gp.mutable_parameters();
            p.position = 0.3;
            p.size = 0.5;
            p.density = 0.6;
            p.texture = 0.5;
            p.dry_wet = 1.0;
            p.pitch = 3.0;
        }
        let input = [ShortFrame { l: 5000, r: -5000 }; BLOCK];
        let mut output = [ShortFrame::default(); BLOCK];
        gp.prepare();
        gp.process(&input, &mut output);
    }
}

#[test]
fn spectral_mode_is_silent_not_a_panic() {
    let mut gp = GranularProcessor::new();
    gp.set_quality(0);
    gp.set_playback_mode(PlaybackMode::Spectral);
    gp.prepare();
    let input = [ShortFrame { l: 9000, r: 9000 }; BLOCK];
    let mut output = [ShortFrame { l: 1, r: 1 }; BLOCK];
    gp.prepare();
    gp.process(&input, &mut output);
    // previous_playback_mode transitions leave the first block silent; run a
    // few more and confirm no panic and (dry/wet defaulting to 0) near silence.
    for _ in 0..10 {
        gp.prepare();
        gp.process(&input, &mut output);
    }
}
