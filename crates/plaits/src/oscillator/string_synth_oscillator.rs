//! `plaits/dsp/oscillator/string_synth_oscillator.h` -- a mix of 7
//! divide-down-organ sawtooth/square registers (8', 4', 2', 1', each saw and
//! square except 1' square), rendered from a single band-limited phase
//! counter using the identity `Square 16' = 2 Saw 16' - Saw 8'`.

use stmlib::parameter_interpolator::ParameterInterpolator;
use stmlib::polyblep::{next_blep_sample, this_blep_sample};

#[derive(Debug, Clone, Copy, Default)]
pub struct StringSynthOscillator {
    phase: f32,
    next_sample: f32,
    segment: i32,
    frequency: f32,
    saw_8_gain: f32,
    saw_4_gain: f32,
    saw_2_gain: f32,
    saw_1_gain: f32,
}

impl StringSynthOscillator {
    pub fn init(&mut self) {
        *self = Self::default();
        self.frequency = 0.001;
    }

    /// `unshifted_registration` holds the 7 gains
    /// `[saw8, sq8, saw4, sq4, saw2, sq2, saw1]`. Adds into `out`.
    pub fn render(&mut self, frequency: f32, unshifted_registration: &[f32; 7], gain: f32, out: &mut [f32]) {
        let mut frequency = frequency * 8.0;

        // Very high frequencies: shift 1 or 2 octaves down (play the 2nd
        // harmonic of a 4kHz wave instead of the 1st harmonic of an 8kHz one).
        let mut shift = 0usize;
        while frequency > 0.5 {
            shift += 2;
            frequency *= 0.5;
        }
        if shift >= 8 {
            return;
        }

        let mut registration = [0.0f32; 7];
        registration[shift..7].copy_from_slice(&unshifted_registration[..7 - shift]);

        let size = out.len();
        let mut fm = ParameterInterpolator::new(&mut self.frequency, frequency, size);
        let mut saw_8 = ParameterInterpolator::new(
            &mut self.saw_8_gain,
            (registration[0] + 2.0 * registration[1]) * gain,
            size,
        );
        let mut saw_4 = ParameterInterpolator::new(
            &mut self.saw_4_gain,
            (registration[2] - registration[1] + 2.0 * registration[3]) * gain,
            size,
        );
        let mut saw_2 = ParameterInterpolator::new(
            &mut self.saw_2_gain,
            (registration[4] - registration[3] + 2.0 * registration[5]) * gain,
            size,
        );
        let mut saw_1 =
            ParameterInterpolator::new(&mut self.saw_1_gain, (registration[6] - registration[5]) * gain, size);

        let mut next_sample = self.next_sample;

        for o in out.iter_mut() {
            let mut this_sample = next_sample;
            next_sample = 0.0;

            let frequency = fm.next();
            let saw_8_gain = saw_8.next();
            let saw_4_gain = saw_4.next();
            let saw_2_gain = saw_2.next();
            let saw_1_gain = saw_1.next();

            self.phase += frequency;
            let mut next_segment = self.phase as i32;
            if next_segment != self.segment {
                let mut discontinuity = 0.0f32;
                if next_segment == 8 {
                    self.phase -= 8.0;
                    next_segment -= 8;
                    discontinuity -= saw_8_gain;
                }
                if next_segment & 3 == 0 {
                    discontinuity -= saw_4_gain;
                }
                if next_segment & 1 == 0 {
                    discontinuity -= saw_2_gain;
                }
                discontinuity -= saw_1_gain;
                if discontinuity != 0.0 {
                    let fraction = self.phase - next_segment as f32;
                    let t = fraction / frequency;
                    this_sample += this_blep_sample(t) * discontinuity;
                    next_sample += next_blep_sample(t) * discontinuity;
                }
            }
            self.segment = next_segment;

            next_sample += (self.phase - 4.0) * saw_8_gain * 0.125;
            next_sample += (self.phase - (self.segment & 4) as f32 - 2.0) * saw_4_gain * 0.25;
            next_sample += (self.phase - (self.segment & 6) as f32 - 1.0) * saw_2_gain * 0.5;
            next_sample += (self.phase - (self.segment & 7) as f32 - 0.5) * saw_1_gain;
            *o += 2.0 * this_sample;
        }
        self.next_sample = next_sample;
    }
}
