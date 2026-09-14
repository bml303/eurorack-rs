//! `streams/audio_cv_meter.h` -- discriminates an ADC signal into audio or
//! CV via zero-crossing rate, and tracks its peak level. Not on
//! [`crate::processor::Processor`]'s own signal path -- the firmware's
//! `ui.cc` uses one per channel for its own audio/CV auto-detection
//! display, which is out of scope here, but the meter itself has no
//! hardware dependency so it's ported as a standalone utility.

#[derive(Debug, Clone, Copy, Default)]
pub struct AudioCvMeter {
    cv: bool,
    peak: i32,
    zero_crossing_interval: i32,
    average_zero_crossing_interval: i32,
    previous_sample: i32,
}

impl AudioCvMeter {
    pub fn init(&mut self) {
        self.peak = 0;
        self.zero_crossing_interval = 0;
        self.average_zero_crossing_interval = 0;
        self.previous_sample = 0;
        self.cv = false;
    }

    pub fn process(&mut self, sample: i32) {
        if (sample >> 1).wrapping_mul(self.previous_sample) < 0 || self.zero_crossing_interval >= 4096 {
            let error = self.zero_crossing_interval.wrapping_sub(self.average_zero_crossing_interval);
            self.average_zero_crossing_interval = self.average_zero_crossing_interval.wrapping_add(error >> 3);
            self.zero_crossing_interval = 0;
        } else {
            self.zero_crossing_interval = self.zero_crossing_interval.wrapping_add(1);
        }

        if self.cv && self.average_zero_crossing_interval < 200 {
            self.cv = false;
        } else if !self.cv && self.average_zero_crossing_interval > 400 {
            self.cv = true;
        }

        self.previous_sample = sample;

        let mut sample = sample;
        if sample < 0 {
            sample = sample.wrapping_neg();
        }
        let error = sample.wrapping_sub(self.peak);
        let coefficient = if error > 0 { 809 } else { 33 };
        self.peak = self.peak.wrapping_add(error.wrapping_mul(coefficient) >> 15);
    }

    pub fn cv(&self) -> bool {
        self.cv
    }
    pub fn peak(&self) -> i32 {
        self.peak
    }
}
