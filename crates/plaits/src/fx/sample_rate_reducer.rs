//! `plaits/dsp/fx/sample_rate_reducer.h` -- band-limited sample & hold at an
//! arbitrary target rate.
//!
//! `optimized_handling_of_special_cases` is a runtime bool here rather than a
//! template parameter; when set, `size` must still be a multiple of 4 for the
//! `>= 0.25` fast paths, exactly as the C requires.

use stmlib::polyblep::{next_blep_sample, this_blep_sample};

#[derive(Debug, Clone, Copy, Default)]
pub struct SampleRateReducer {
    phase: f32,
    sample: f32,
    previous_sample: f32,
    next_sample: f32,
}

impl SampleRateReducer {
    pub fn init(&mut self) {
        *self = Self::default();
    }

    pub fn process(&mut self, optimized_handling_of_special_cases: bool, frequency: f32, in_out: &mut [f32]) {
        let mut frequency = frequency;
        if optimized_handling_of_special_cases {
            // Fast specialised paths for target rates close to the original
            // rate. Caveats (inherited from the C): `size` must be a multiple
            // of 4, and there's a transition glitch versus the general case,
            // so don't use this branch under frequency modulation.
            if frequency >= 1.0 {
                return;
            } else if frequency >= 0.5 {
                self.process_half(2.0 - 2.0 * frequency, in_out);
                return;
            } else if frequency >= 0.25 {
                self.process_quarter(2.0 - 4.0 * frequency, in_out);
                return;
            }
        } else {
            frequency = frequency.clamp(0.0, 1.0);
        }

        let mut previous_sample = self.previous_sample;
        let mut next_sample = self.next_sample;
        let mut sample = self.sample;
        let mut phase = self.phase;

        for s in in_out.iter_mut() {
            let mut this_sample = next_sample;
            next_sample = 0.0;
            phase += frequency;
            if phase >= 1.0 {
                phase -= 1.0;
                let t = phase / frequency;
                // t = 0: the transition occurred right at this sample.
                // t = 1: the transition occurred at the previous sample.
                let new_sample = previous_sample + (*s - previous_sample) * (1.0 - t);
                let discontinuity = new_sample - sample;
                this_sample += discontinuity * this_blep_sample(t);
                next_sample += discontinuity * next_blep_sample(t);
                sample = new_sample;
            }
            next_sample += sample;
            previous_sample = *s;
            *s = this_sample;
        }
        self.phase = phase;
        self.next_sample = next_sample;
        self.sample = sample;
        self.previous_sample = previous_sample;
    }

    fn process_half(&mut self, amount: f32, in_out: &mut [f32]) {
        let mut i = 0;
        while i < in_out.len() {
            in_out[i + 1] += (in_out[i] - in_out[i + 1]) * amount;
            i += 2;
        }
        let last = in_out[in_out.len() - 1];
        self.sample = last;
        self.next_sample = last;
        self.previous_sample = last;
    }

    fn process_quarter(&mut self, amount: f32, in_out: &mut [f32]) {
        let mut i = 0;
        while i < in_out.len() {
            in_out[i + 1] = in_out[i];
            in_out[i + 2] += (in_out[i] - in_out[i + 2]) * amount;
            in_out[i + 3] = in_out[i + 2];
            i += 4;
        }
        let last = in_out[in_out.len() - 1];
        self.sample = last;
        self.next_sample = last;
        self.previous_sample = last;
    }
}
