//! `plaits/dsp/noise/clocked_noise.h` -- white noise sampled at a target
//! frequency (band-limited via BLEP, like a hard-sync'ed sample & hold), with
//! a bleed of raw noise at high rates.

use stmlib::parameter_interpolator::ParameterInterpolator;
use stmlib::polyblep::{next_blep_sample, this_blep_sample};
use stmlib::Random;

#[derive(Debug, Clone, Copy, Default)]
pub struct ClockedNoise {
    phase: f32,
    sample: f32,
    next_sample: f32,
    frequency: f32,
}

impl ClockedNoise {
    pub fn init(&mut self) {
        self.phase = 0.0;
        self.sample = 0.0;
        self.next_sample = 0.0;
        self.frequency = 0.001;
    }

    pub fn render(&mut self, sync: bool, frequency: f32, out: &mut [f32]) {
        let frequency = frequency.clamp(0.0, 1.0);
        let size = out.len();
        let mut fm = ParameterInterpolator::new(&mut self.frequency, frequency, size);

        let mut next_sample = self.next_sample;
        let mut sample = self.sample;

        if sync {
            self.phase = 1.0;
        }

        for o in out.iter_mut() {
            let mut this_sample = next_sample;
            next_sample = 0.0;

            let frequency = fm.next();
            let raw_sample = Random::get_float() * 2.0 - 1.0;
            let raw_amount = (4.0 * (frequency - 0.25)).clamp(0.0, 1.0);

            self.phase += frequency;

            if self.phase >= 1.0 {
                self.phase -= 1.0;
                let t = self.phase / frequency;
                let new_sample = raw_sample;
                let discontinuity = new_sample - sample;
                this_sample += discontinuity * this_blep_sample(t);
                next_sample += discontinuity * next_blep_sample(t);
                sample = new_sample;
            }
            next_sample += sample;
            *o = this_sample + raw_amount * (raw_sample - this_sample);
        }
        self.next_sample = next_sample;
        self.sample = sample;
    }
}
