//! `clouds/dsp/correlator.{h,cc}` -- finds stretch/shift splice points by
//! maximising cross-correlation of sample *sign bits*, so 32 samples match in
//! one XOR + popcount. Integer-exact translation of the C.

/// Words of headroom for the source window (`kMaxWSOLASize / 32`, generous).
const SOURCE_WORDS: usize = 4096 / 32 + 32;
/// The destination window is read at up to `window_size * 2` bits; size it
/// generously so `EvaluateNextCandidate`'s `destination[offset + i + 1]` can
/// never run off the end.
const DESTINATION_WORDS: usize = 4096 * 2 / 32 + 64;

/// `Correlator`.
pub struct Correlator {
    source: [u32; SOURCE_WORDS],
    destination: [u32; DESTINATION_WORDS],

    offset: i32,
    increment: i32,
    size: i32,
    candidate: i32,

    best_score: u32,
    best_match: i32,

    done: bool,
}

impl Correlator {
    pub fn new() -> Self {
        Self {
            source: [0; SOURCE_WORDS],
            destination: [0; DESTINATION_WORDS],
            offset: 0,
            increment: 0,
            size: 0,
            candidate: 0,
            best_score: 0,
            best_match: 0,
            done: true,
        }
    }

    /// `Init`.
    pub fn init(&mut self) {
        self.offset = 0;
        self.best_match = 0;
        self.done = true;
    }

    #[inline]
    pub fn source_mut(&mut self) -> &mut [u32] {
        &mut self.source
    }

    #[inline]
    pub fn destination_mut(&mut self) -> &mut [u32] {
        &mut self.destination
    }

    /// `StartSearch`.
    pub fn start_search(&mut self, size: i32, offset: i32, increment: i32) {
        self.offset = offset;
        self.increment = increment;
        self.best_score = 0;
        self.best_match = 0;
        self.candidate = 0;
        self.size = size;
        self.done = false;
    }

    /// `best_match()` -- the winning splice offset in samples.
    #[inline]
    pub fn best_match(&self) -> i32 {
        self.offset + (self.best_match.wrapping_mul(self.increment >> 4) >> 12)
    }

    #[inline]
    pub fn done(&self) -> bool {
        self.done
    }

    /// `EvaluateSomeCandidates` -- one budget's worth of the search.
    pub fn evaluate_some_candidates(&mut self) {
        let mut num_candidates = (self.size >> 2) + 16;
        while num_candidates > 0 {
            self.evaluate_next_candidate();
            num_candidates -= 1;
        }
    }

    /// `EvaluateNextCandidate`.
    pub fn evaluate_next_candidate(&mut self) {
        if self.done {
            return;
        }
        let num_words = (self.size >> 5) as usize;
        let offset_words = (self.candidate >> 5) as usize;
        let offset_bits = (self.candidate & 0x1f) as u32;

        let mut xcorr: u32 = 0;
        for i in 0..num_words {
            let source_bits = self.source[i];
            let mut destination_bits: u32 = 0;
            destination_bits |= self.destination[offset_words + i] << offset_bits;
            // C does `destination[i + 1] >> (32 - offset_bits)`. When
            // `offset_bits == 0` (every 32nd candidate, including the first)
            // that is a shift by 32 -- UB in C, but the x86/g++ reference
            // masks the count to 5 bits and shifts by 0. Match that.
            let shift = (32 - offset_bits) & 31;
            destination_bits |= self.destination[offset_words + i + 1] >> shift;
            let mut count = !(source_bits ^ destination_bits);
            count = count.wrapping_sub((count >> 1) & 0x5555_5555);
            count = (count & 0x3333_3333) + ((count >> 2) & 0x3333_3333);
            count = (((count.wrapping_add(count >> 4)) & 0x0f0f_0f0f).wrapping_mul(0x0101_0101)) >> 24;
            xcorr = xcorr.wrapping_add(count);
        }
        if xcorr > self.best_score {
            self.best_match = self.candidate;
            self.best_score = xcorr;
        }
        self.candidate += 1;
        self.done = self.candidate >= self.size;
    }
}

impl Default for Correlator {
    fn default() -> Self {
        Self::new()
    }
}
