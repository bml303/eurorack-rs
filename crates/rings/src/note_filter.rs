//! `rings/dsp/note_filter.h` -- a median + adaptive-lag filter that turns the
//! noisy pitch CV into stable note data, following sharp edges instantly.

use stmlib::DelayLine;

const N: usize = 4; // Median filter order.

/// `rings::NoteFilter`.
#[derive(Debug, Clone)]
pub struct NoteFilter {
    previous_values: [f32; N],
    note: f32,
    stable_note: f32,
    delayed_stable_note: DelayLine<16>,

    coefficient: f32,
    stable_coefficient: f32,

    fast_coefficient: f32,
    slow_coefficient: f32,
    lag_coefficient: f32,
}

impl Default for NoteFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl NoteFilter {
    pub fn new() -> Self {
        Self {
            previous_values: [69.0; N],
            note: 69.0,
            stable_note: 69.0,
            delayed_stable_note: DelayLine::default(),
            coefficient: 0.0,
            stable_coefficient: 0.0,
            fast_coefficient: 0.0,
            slow_coefficient: 0.0,
            lag_coefficient: 0.0,
        }
    }

    /// `Init(sample_rate, t_fast_edge, t_steady, edge_recovery, edge_avoidance)`.
    pub fn init(
        &mut self,
        sample_rate: f32,
        time_constant_fast_edge: f32,
        time_constant_steady_part: f32,
        edge_recovery_time: f32,
        edge_avoidance_delay: f32,
    ) {
        self.fast_coefficient = 1.0 / (time_constant_fast_edge * sample_rate);
        self.slow_coefficient = 1.0 / (time_constant_steady_part * sample_rate);
        self.lag_coefficient = 1.0 / (edge_recovery_time * sample_rate);

        self.delayed_stable_note.init();
        self.delayed_stable_note
            .set_delay(15usize.min((edge_avoidance_delay * sample_rate) as usize));

        self.stable_note = 69.0;
        self.note = 69.0;
        self.coefficient = self.fast_coefficient;
        self.stable_coefficient = self.slow_coefficient;
        self.previous_values = [69.0; N];
    }

    /// `Process(note, strum) -> note`.
    pub fn process(&mut self, note: f32, strum: bool) -> f32 {
        if (note - self.note).abs() > 0.4 || strum {
            self.stable_note = note;
            self.note = note;
            self.coefficient = self.fast_coefficient;
            self.stable_coefficient = self.slow_coefficient;
            self.previous_values = [note; N];
        } else {
            // Median of the last N raw values.
            self.previous_values.rotate_left(1);
            self.previous_values[N - 1] = note;
            let mut sorted = self.previous_values;
            sorted.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap());
            let median = 0.5 * (sorted[(N - 1) / 2] + sorted[N / 2]);

            // Adaptive lag processor.
            self.note += self.coefficient * (median - self.note);
            self.stable_note += self.stable_coefficient * (self.note - self.stable_note);

            self.coefficient += self.lag_coefficient * (self.slow_coefficient - self.coefficient);
            self.stable_coefficient +=
                self.lag_coefficient * (self.lag_coefficient - self.stable_coefficient);

            self.delayed_stable_note.write(self.stable_note);
        }
        self.note
    }

    #[inline]
    pub fn note(&self) -> f32 {
        self.note
    }

    #[inline]
    pub fn stable_note(&self) -> f32 {
        self.delayed_stable_note.read()
    }
}
