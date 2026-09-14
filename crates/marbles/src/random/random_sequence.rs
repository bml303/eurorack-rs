//! `marbles/random/random_sequence.h` -- sequence of random values, with
//! "deja vu" looping/replay.
//!
//! Deviation from the C++: the shared PRNG (`RandomStream`) is passed as an
//! explicit `&mut RandomStream` argument to the methods that need it, instead
//! of being stored as a `RandomStream*` field -- see `random_stream.rs`. The
//! three raw pointers into `loop_`/`history_` (`redo_read_ptr_`,
//! `redo_write_ptr_`, `redo_write_history_ptr_`) are likewise stored as plain
//! indices, which also simplifies `Clone` (a pointer copied verbatim from a
//! *different* instance's arrays needs an offset translation in C++; an index
//! is already position-independent).

use super::random_stream::RandomStream;

const DEJA_VU_BUFFER_SIZE: usize = 16;
const HISTORY_BUFFER_SIZE: usize = 16;

const MAX_UINT32: f32 = 4294967296.0;

#[derive(Debug, Clone)]
pub struct RandomSequence {
    loop_buffer: [f32; DEJA_VU_BUFFER_SIZE],
    history: [f32; HISTORY_BUFFER_SIZE],
    loop_write_head: i32,
    length: i32,
    step: i32,

    // Allows going back in the past and getting the same results again from
    // `next_value` calls. Allows the 3 X channels to be locked to the same
    // random loop.
    record_head: i32,
    replay_head: i32,
    replay_start: i32,
    replay_hash: u32,
    replay_shift: u32,

    deja_vu: f32,

    redo_read_idx: usize,
    redo_write_idx: Option<usize>,
    redo_write_history_idx: Option<usize>,
}

impl Default for RandomSequence {
    fn default() -> Self {
        Self {
            loop_buffer: [0.0; DEJA_VU_BUFFER_SIZE],
            history: [0.0; HISTORY_BUFFER_SIZE],
            loop_write_head: 0,
            length: 8,
            step: 0,
            record_head: 0,
            replay_head: -1,
            replay_start: 0,
            replay_hash: 0,
            replay_shift: 0,
            deja_vu: 0.0,
            redo_read_idx: 0,
            redo_write_idx: None,
            redo_write_history_idx: None,
        }
    }
}

impl RandomSequence {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn init(&mut self, random_stream: &mut RandomStream) {
        for slot in self.loop_buffer.iter_mut() {
            *slot = random_stream.get_float();
        }
        self.history = [0.0; HISTORY_BUFFER_SIZE];

        self.loop_write_head = 0;
        self.length = 8;
        self.step = 0;

        self.record_head = 0;
        self.replay_head = -1;
        self.replay_start = 0;
        self.deja_vu = 0.0;
        self.replay_hash = 0;
        self.replay_shift = 0;

        self.redo_read_idx = 0;
        self.redo_write_idx = None;
        self.redo_write_history_idx = None;
    }

    /// `Clone` is a reserved word in Rust -- named `clone_from_sequence` here.
    pub fn clone_from_sequence(&mut self, source: &RandomSequence) {
        self.loop_buffer = source.loop_buffer;
        self.history = source.history;

        self.loop_write_head = source.loop_write_head;
        self.length = source.length;
        self.step = source.step;

        self.record_head = source.record_head;
        self.replay_head = source.replay_head;
        self.replay_start = source.replay_start;
        self.replay_hash = source.replay_hash;
        self.replay_shift = source.replay_shift;

        self.deja_vu = source.deja_vu;

        self.redo_read_idx = source.redo_read_idx;
        self.redo_write_idx = source.redo_write_idx;
        self.redo_write_history_idx = source.redo_write_history_idx;
    }

    pub fn record(&mut self) {
        self.replay_start = self.record_head;
        self.replay_head = -1;
    }

    pub fn replay_pseudo_random(&mut self, hash: u32) {
        self.replay_head = self.replay_start;
        self.replay_hash = hash;
        self.replay_shift = 0;
    }

    pub fn replay_shifted(&mut self, shift: u32) {
        self.replay_head = self.replay_start;
        self.replay_hash = 0;
        self.replay_shift = shift;
    }

    fn get_replay_value(&self) -> f32 {
        let h = (self.replay_head as i64 - 1 - self.replay_shift as i64
            + 2 * HISTORY_BUFFER_SIZE as i64)
            % HISTORY_BUFFER_SIZE as i64;
        let h = h as usize;
        if self.replay_hash == 0 {
            self.history[h]
        } else {
            let word = (self.history[h] * MAX_UINT32) as u32;
            let word = (word ^ self.replay_hash)
                .wrapping_mul(1664525)
                .wrapping_add(1013904223);
            word as f32 / MAX_UINT32
        }
    }

    /// `RewriteValue(x)` returns what the most recent call to `next_value`
    /// would have returned if its second argument were `x` instead. This is
    /// used to "rewrite history" when the module acquires data from an
    /// external source (ASR, randomizer or quantizer mode).
    pub fn rewrite_value(&mut self, value: f32) -> f32 {
        if self.replay_head >= 0 {
            return self.get_replay_value();
        }

        if let Some(idx) = self.redo_write_idx {
            self.loop_buffer[idx] = 1.0 + value;
        }
        let mut result = self.loop_buffer[self.redo_read_idx];
        if result >= 1.0 {
            result -= 1.0;
        } else {
            result = 0.5;
        }
        if let Some(idx) = self.redo_write_history_idx {
            self.history[idx] = result;
        }
        result
    }

    pub fn next_value(&mut self, random_stream: &mut RandomStream, deterministic: bool, value: f32) -> f32 {
        if self.replay_head >= 0 {
            self.replay_head = (self.replay_head + 1) % HISTORY_BUFFER_SIZE as i32;
            return self.get_replay_value();
        }

        let p_sqrt = 2.0 * self.deja_vu - 1.0;
        let p = p_sqrt * p_sqrt;
        let mutate = random_stream.get_float() < p;

        if mutate && self.deja_vu <= 0.5 {
            // Generate a new value and put it at the end of the loop.
            let idx = self.loop_write_head as usize;
            self.redo_write_idx = Some(idx);
            self.loop_buffer[idx] = if deterministic {
                1.0 + value
            } else {
                random_stream.get_float()
            };
            self.loop_write_head = (self.loop_write_head + 1) % DEJA_VU_BUFFER_SIZE as i32;
            self.step = self.length - 1;
        } else {
            // Do not generate a new value, just replay the loop or jump
            // randomly through it.
            self.redo_write_idx = None;
            if mutate {
                // implied: deja_vu_ > 0.5
                self.step = (random_stream.get_float() * self.length as f32) as i32;
            } else {
                self.step += 1;
                if self.step >= self.length {
                    self.step = 0;
                }
            }
        }
        let i = self.loop_write_head + DEJA_VU_BUFFER_SIZE as i32 - self.length + self.step;
        self.redo_read_idx = (i as usize) % DEJA_VU_BUFFER_SIZE;
        let mut result = self.loop_buffer[self.redo_read_idx];
        if result >= 1.0 {
            result -= 1.0;
        } else if deterministic {
            // We ask for a deterministic value (shift register), but the loop
            // contains random values. Return 0.5 in this case!
            result = 0.5;
        }
        self.redo_write_history_idx = Some(self.record_head as usize);
        self.history[self.record_head as usize] = result;
        self.record_head = (self.record_head + 1) % HISTORY_BUFFER_SIZE as i32;
        result
    }

    pub fn next_vector(&mut self, random_stream: &mut RandomStream, destination: &mut [f32]) {
        let seed = self.next_value(random_stream, false, 0.0);
        let mut word = (seed * MAX_UINT32) as u32;
        for sample in destination.iter_mut() {
            *sample = word as f32 / MAX_UINT32;
            word = word.wrapping_mul(1664525).wrapping_add(1013904223);
        }
    }

    pub fn set_deja_vu(&mut self, deja_vu: f32) {
        self.deja_vu = deja_vu;
    }

    pub fn set_length(&mut self, length: i32) {
        if !(1..=DEJA_VU_BUFFER_SIZE as i32).contains(&length) {
            return;
        }
        self.length = length;
        self.step %= length;
    }

    pub fn deja_vu(&self) -> f32 {
        self.deja_vu
    }

    pub fn length(&self) -> i32 {
        self.length
    }

    pub fn reset(&mut self) {
        self.step = self.length - 1;
    }
}
