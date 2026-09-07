//! `rings/dsp/strummer.h` -- decides, per block, whether a strum event should
//! fire: from an explicit trigger, from a note-CV change, or from an audio
//! onset, with an inter-onset-interval inhibit timer.

use crate::dsp::SAMPLE_RATE;
use crate::onset_detector::OnsetDetector;
use crate::part::PerformanceState;

/// `rings::Strummer`.
#[derive(Debug, Clone)]
pub struct Strummer {
    previous_note: f32,
    inhibit_counter: i32,
    inhibit_timer: i32,
    onset_detector: OnsetDetector,
}

impl Default for Strummer {
    fn default() -> Self {
        Self::new()
    }
}

impl Strummer {
    pub fn new() -> Self {
        Self {
            previous_note: 69.0,
            inhibit_counter: 0,
            inhibit_timer: 0,
            onset_detector: OnsetDetector::new(),
        }
    }

    /// `Init(ioi, sr)` -- `ioi` is the inter-onset interval in seconds, `sr`
    /// the decimated (block) rate in Hz.
    pub fn init(&mut self, ioi: f32, sr: f32) {
        self.onset_detector.init(
            8.0 / SAMPLE_RATE,
            160.0 / SAMPLE_RATE,
            1600.0 / SAMPLE_RATE,
            sr,
            ioi,
        );
        self.inhibit_timer = (ioi * sr) as i32;
        self.inhibit_counter = 0;
        self.previous_note = 69.0;
    }

    /// `Process(in, size, performance_state)` -- `in` is the audio input, or
    /// `None` when nothing is patched to the exciter.
    pub fn process(
        &mut self,
        input: Option<&[f32]>,
        size: usize,
        performance_state: &mut PerformanceState,
    ) {
        let has_onset = match input {
            Some(buf) => self.onset_detector.process(buf, size),
            None => false,
        };
        let note_changed = (performance_state.note - self.previous_note).abs() > 0.4;

        let mut inhibit_timer = self.inhibit_timer;
        if performance_state.internal_strum {
            let has_external_note_cv = !performance_state.internal_note;
            let has_external_exciter = !performance_state.internal_exciter;
            if has_external_note_cv {
                performance_state.strum = note_changed;
            } else if has_external_exciter {
                performance_state.strum = has_onset;
                inhibit_timer *= 4;
            } else {
                performance_state.strum = false;
            }
        }

        if self.inhibit_counter != 0 {
            self.inhibit_counter -= 1;
            performance_state.strum = false;
        } else if performance_state.strum {
            self.inhibit_counter = inhibit_timer;
        }
        self.previous_note = performance_state.note;
    }
}
