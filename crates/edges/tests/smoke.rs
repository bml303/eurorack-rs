//! Crash / sanity / regression checks for the Edges port.
//!
//! There is no C bit-compare harness: the C `DigitalOscillator` is AVR-only
//! (inline asm + `pgm_read_*`) and its host build would take the *portable*
//! `InterpolateSample` path, which disagrees with the shipping firmware (it
//! indexes the 513-byte tables at `phase >> 8` instead of `phase >> 7`). This
//! port follows the firmware. These tests lock its output and check the
//! musically-observable properties.

use edges::{DigitalOscillator, OscillatorShape, PulseWidth, TimerOscillator, TimerPrescaler};

/// Effective sample rate of the audio ISR (`F_CPU / 665`).
const SAMPLE_RATE: f32 = 48_120.0;
const BLOCK: usize = 16;

fn all_shapes() -> [OscillatorShape; 6] {
    [
        OscillatorShape::Triangle,
        OscillatorShape::NesTriangle,
        OscillatorShape::PitchedNoise,
        OscillatorShape::NesNoiseLong,
        OscillatorShape::NesNoiseShort,
        OscillatorShape::Sine,
    ]
}

fn render(osc: &mut DigitalOscillator, blocks: usize) -> Vec<u16> {
    let mut out = vec![0u16; blocks * BLOCK];
    for chunk in out.chunks_mut(BLOCK) {
        osc.render(chunk);
    }
    out
}

#[test]
fn every_shape_is_in_range_and_makes_sound() {
    for shape in all_shapes() {
        let mut osc = DigitalOscillator::new();
        osc.set_cv_pw(200);
        osc.update_pitch(60 << 7, shape);
        osc.gate(true);

        let buf = render(&mut osc, 400);
        assert!(
            buf.iter().all(|&s| s <= 4095),
            "{shape:?} out of 12-bit range"
        );

        let mean = buf.iter().map(|&s| s as f64).sum::<f64>() / buf.len() as f64;
        let var = buf.iter().map(|&s| (s as f64 - mean).powi(2)).sum::<f64>() / buf.len() as f64;
        assert!(
            var.sqrt() > 5.0,
            "{shape:?} produced (near-)silence: stddev {}",
            var.sqrt()
        );
    }
}

#[test]
fn gate_low_is_silence() {
    let mut osc = DigitalOscillator::new();
    osc.update_pitch(60 << 7, OscillatorShape::Triangle);
    osc.gate(false);
    let buf = render(&mut osc, 8);
    assert!(buf.iter().all(|&s| s == 2048));
}

#[test]
fn triangle_pitch_tracks_the_note() {
    // A4 = MIDI 69 = 440 Hz. Count rising zero-crossings around the DC mean.
    let mut osc = DigitalOscillator::new();
    osc.update_pitch(69 << 7, OscillatorShape::Triangle);
    osc.gate(true);
    // discard a few blocks of startup
    render(&mut osc, 4);
    let buf = render(&mut osc, 3000);

    let mean = buf.iter().map(|&s| s as f64).sum::<f64>() / buf.len() as f64;
    let mut crossings = 0;
    for w in buf.windows(2) {
        if (w[0] as f64) < mean && (w[1] as f64) >= mean {
            crossings += 1;
        }
    }
    let freq = crossings as f32 * SAMPLE_RATE / buf.len() as f32;
    assert!(
        (freq - 440.0).abs() < 12.0,
        "A4 triangle read as {freq:.1} Hz"
    );
}

#[test]
fn octave_doubles_the_frequency() {
    let pitch_of = |note: i16| {
        let mut osc = DigitalOscillator::new();
        osc.update_pitch(note << 7, OscillatorShape::Triangle);
        osc.gate(true);
        render(&mut osc, 4);
        let buf = render(&mut osc, 3000);
        let mean = buf.iter().map(|&s| s as f64).sum::<f64>() / buf.len() as f64;
        let c = buf
            .windows(2)
            .filter(|w| (w[0] as f64) < mean && (w[1] as f64) >= mean)
            .count();
        c as f32 * SAMPLE_RATE / buf.len() as f32
    };
    let ratio = pitch_of(60) / pitch_of(48);
    assert!((ratio - 2.0).abs() < 0.1, "octave ratio {ratio}");
}

#[test]
fn timer_oscillator_frequencies() {
    // Note 24 (32.70 Hz): the LFO-mode boundary -> default CLK_8, period ~61156.
    let mut t = TimerOscillator::new();
    t.update_pitch(36 << 7, PulseWidth::Pw50);
    // note 36 -> 65.41 Hz, dual-slope: F_CPU / (2 * period * div)
    let div = match t.prescaler() {
        TimerPrescaler::Clk1 => 1.0,
        TimerPrescaler::Clk8 => 8.0,
        TimerPrescaler::Clk64 => 64.0,
    };
    let freq = 32_000_000.0 / (2.0 * t.period() as f32 * div);
    assert!((freq - 65.41).abs() < 1.0, "note 36 timer freq {freq:.2}");

    // 50% duty -> value ~= period/2.
    let duty = t.value() as f32 / t.period() as f32;
    assert!((duty - 0.5).abs() < 0.02, "duty {duty:.3}");
}

#[test]
fn timer_render_square_is_a_square() {
    let mut t = TimerOscillator::new();
    t.update_pitch(57 << 7, PulseWidth::Pw50); // A3, 220 Hz
    let mut buf = vec![0u16; 48_000];
    t.render_square(&mut buf, SAMPLE_RATE, true);
    assert!(buf.iter().all(|&s| s == 0 || s == 4095));
    let high = buf.iter().filter(|&&s| s == 4095).count();
    let ratio = high as f32 / buf.len() as f32;
    assert!((ratio - 0.5).abs() < 0.02, "square duty {ratio:.3}");

    let edges = buf.windows(2).filter(|w| w[0] == 0 && w[1] == 4095).count();
    let freq = edges as f32 * SAMPLE_RATE / buf.len() as f32;
    assert!((freq - 220.0).abs() < 2.0, "A3 square {freq:.1} Hz");
}

#[test]
fn sub_follow_is_an_octave_and_a_half_up() {
    let mut parent = TimerOscillator::new();
    parent.update_pitch(48 << 7, PulseWidth::Pw50);
    let mut sub = TimerOscillator::new();
    sub.sub_follow(&parent);
    assert_eq!(sub.period(), parent.period() >> 1);
    assert_eq!(sub.value(), (parent.period() >> 1) >> 1);
}

#[test]
fn output_is_stable_regression() {
    // Lock the exact output so a refactor that changes the arithmetic is caught
    // even without the C toolchain.
    fn checksum(shape: OscillatorShape, note: i16) -> u64 {
        let mut osc = DigitalOscillator::new();
        osc.set_cv_pw(180);
        osc.update_pitch(note << 7, shape);
        osc.gate(true);
        let buf = render(&mut osc, 256);
        buf.iter().fold(1469598103934665603u64, |h, &s| {
            (h ^ s as u64).wrapping_mul(1099511628211)
        })
    }
    let got = [
        checksum(OscillatorShape::Triangle, 60),
        checksum(OscillatorShape::NesTriangle, 48),
        checksum(OscillatorShape::PitchedNoise, 40),
        checksum(OscillatorShape::NesNoiseLong, 40),
        checksum(OscillatorShape::NesNoiseShort, 40),
        checksum(OscillatorShape::Sine, 72),
    ];
    // FNV-1a of 256 blocks per (shape, note), captured from the port.
    const EXPECTED: [u64; 6] = [
        12936347054992100054,
        15267562478164071565,
        336250067178233833,
        6818225070425510898,
        13811842237085587469,
        6889764207703548201,
    ];
    assert_eq!(got, EXPECTED, "digital oscillator output changed");
}
