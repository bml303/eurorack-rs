//! `stmlib/algorithms/pattern_predictor.h` -- predicts the period of a
//! quasi-periodic clock/pulse train, tracking both a plain moving average and
//! rhythmic patterns of up to `CANDIDATES - 1` steps, and picks whichever
//! candidate currently has the lowest smoothed prediction error.
//!
//! The C template is `PatternPredictor<history_size, max_candidate_period>`
//! with an implicit `max_candidate_period + 1`-sized error/prediction array;
//! Rust const generics can't express that arithmetic on stable, so `CANDIDATES`
//! here is that `+ 1` already applied (the sole instantiation in the firmware,
//! `PatternPredictor<32, 8>`, becomes `PatternPredictor<32, 9>`).

#[derive(Debug, Clone, Copy)]
pub struct PatternPredictor<const HISTORY_SIZE: usize, const CANDIDATES: usize> {
    history: [u32; HISTORY_SIZE],
    prediction_error: [i32; CANDIDATES],
    predicted_period: [i32; CANDIDATES],
    history_pointer: usize,
}

impl<const HISTORY_SIZE: usize, const CANDIDATES: usize> Default
    for PatternPredictor<HISTORY_SIZE, CANDIDATES>
{
    fn default() -> Self {
        Self {
            history: [0; HISTORY_SIZE],
            prediction_error: [0; CANDIDATES],
            predicted_period: [0; CANDIDATES],
            history_pointer: 0,
        }
    }
}

impl<const HISTORY_SIZE: usize, const CANDIDATES: usize> PatternPredictor<HISTORY_SIZE, CANDIDATES> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn init(&mut self) {
        *self = Self::default();
    }

    /// `Predict(value)`.
    pub fn predict(&mut self, value: i32) -> u32 {
        self.history[self.history_pointer] = value as u32;
        let mut best_period = 0usize;

        for i in 0..CANDIDATES {
            let error = self.predicted_period[i].wrapping_sub(value).wrapping_abs();
            let delta = error.wrapping_sub(self.prediction_error[i]);

            if delta > 0 {
                self.prediction_error[i] = self.prediction_error[i].wrapping_add(delta >> 1);
            } else {
                self.prediction_error[i] = self.prediction_error[i].wrapping_add(delta >> 3);
            }

            if i == 0 {
                self.predicted_period[0] = (value.wrapping_add(self.predicted_period[0])) >> 1;
            } else {
                let t = self.history_pointer + 1 + HISTORY_SIZE - i;
                self.predicted_period[i] = self.history[t % HISTORY_SIZE] as i32;
            }

            if self.prediction_error[i] < self.prediction_error[best_period] {
                best_period = i;
            }
        }

        self.history_pointer = (self.history_pointer + 1) % HISTORY_SIZE;
        self.predicted_period[best_period] as u32
    }
}
