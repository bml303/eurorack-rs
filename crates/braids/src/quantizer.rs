//! `braids/quantizer.h` + `braids/quantizer_scales.h` -- a hysteretic pitch
//! quantizer with a 128-entry codebook.

use stmlib::clip16_sym;

/// A scale definition: `span` is the interval that repeats (usually one octave,
/// `12 << 7`), `notes` are the offsets within it (7.7 semitones).
#[derive(Debug, Clone, Copy)]
pub struct Scale {
    pub span: i16,
    pub notes: &'static [i16],
}

#[derive(Debug, Clone)]
pub struct Quantizer {
    enabled: bool,
    codebook: [i16; 128],
    codeword: i32,
    previous_boundary: i32,
    next_boundary: i32,
}

impl Default for Quantizer {
    fn default() -> Self {
        Self::new()
    }
}

impl Quantizer {
    pub fn new() -> Self {
        let mut q = Self {
            enabled: true,
            codebook: [0; 128],
            codeword: 0,
            previous_boundary: 0,
            next_boundary: 0,
        };
        q.init();
        q
    }

    pub fn init(&mut self) {
        self.enabled = true;
        self.codeword = 0;
        self.previous_boundary = 0;
        self.next_boundary = 0;
        for i in 0..128i16 {
            self.codebook[i as usize] = (i - 64) << 7;
        }
    }

    pub fn configure(&mut self, scale: &Scale) {
        self.configure_raw(scale.notes, scale.span);
    }

    pub fn configure_raw(&mut self, notes: &[i16], span: i16) {
        self.enabled = !notes.is_empty() && span != 0;
        if !self.enabled {
            return;
        }
        let num_notes = notes.len();
        let mut octave = 0i32;
        let mut note = 0usize;
        let root = 0i32;
        for i in 0..64i32 {
            let up = root + notes[note] as i32 + span as i32 * octave;
            let down = root + notes[num_notes - 1 - note] as i32 + (-octave - 1) * span as i32;
            self.codebook[(64 + i) as usize] = clip16_sym(up) as i16;
            self.codebook[(64 - i - 1) as usize] = clip16_sym(down) as i16;
            note += 1;
            if note >= num_notes {
                note = 0;
                octave += 1;
            }
        }
    }

    /// `Process(pitch)` -- no root offset.
    #[inline]
    pub fn process(&mut self, pitch: i32) -> i32 {
        self.process_with_root(pitch, 0)
    }

    /// `Process(pitch, root)`.
    pub fn process_with_root(&mut self, mut pitch: i32, root: i32) -> i32 {
        if !self.enabled {
            return pitch;
        }
        pitch -= root;
        if pitch >= self.previous_boundary && pitch <= self.next_boundary {
            pitch = self.codeword;
        } else {
            // upper_bound over codebook[3..126] for `pitch as i16`.
            let target = pitch as i16;
            let window = &self.codebook[3..126];
            let upper_bound_index = 3 + window.partition_point(|&x| x <= target);
            let lower_bound_index = upper_bound_index as i32 - 2;

            let mut best_distance = 16384i16;
            let mut q = -1i32;
            let mut i = lower_bound_index;
            while i <= upper_bound_index as i32 {
                // C: `int16_t distance = abs(pitch - codebook_[i]);` -- 32-bit
                // subtract and abs, then truncate to i16.
                let distance = (pitch - self.codebook[i as usize] as i32).abs() as i16;
                if distance < best_distance {
                    best_distance = distance;
                    q = i;
                }
                i += 1;
            }
            let q = q as usize;
            self.codeword = self.codebook[q] as i32;
            self.previous_boundary = (9 * self.codebook[q - 1] as i32 + 7 * self.codeword) >> 4;
            self.next_boundary = (9 * self.codebook[q + 1] as i32 + 7 * self.codeword) >> 4;
            pitch = self.codeword;
        }
        pitch + root
    }
}

/// `braids/quantizer_scales.h` -- the 50 built-in scales, in firmware order.
pub static SCALES: &[Scale] = &[
    Scale {
        span: 0,
        notes: &[],
    },
    Scale {
        span: 12 << 7,
        notes: &[0, 128, 256, 384, 512, 640, 768, 896, 1024, 1152, 1280, 1408],
    },
    Scale {
        span: 12 << 7,
        notes: &[0, 256, 512, 640, 896, 1152, 1408],
    },
    Scale {
        span: 12 << 7,
        notes: &[0, 256, 384, 640, 896, 1152, 1280],
    },
    Scale {
        span: 12 << 7,
        notes: &[0, 128, 384, 640, 896, 1024, 1280],
    },
    Scale {
        span: 12 << 7,
        notes: &[0, 256, 512, 768, 896, 1152, 1408],
    },
    Scale {
        span: 12 << 7,
        notes: &[0, 256, 512, 640, 896, 1152, 1280],
    },
    Scale {
        span: 12 << 7,
        notes: &[0, 256, 384, 640, 896, 1024, 1280],
    },
    Scale {
        span: 12 << 7,
        notes: &[0, 128, 384, 640, 768, 1024, 1280],
    },
    Scale {
        span: 12 << 7,
        notes: &[0, 384, 512, 896, 1152, 1280],
    },
    Scale {
        span: 12 << 7,
        notes: &[0, 384, 640, 768, 896, 1280],
    },
    Scale {
        span: 12 << 7,
        notes: &[0, 256, 512, 896, 1152],
    },
    Scale {
        span: 12 << 7,
        notes: &[0, 384, 640, 896, 1280],
    },
    Scale {
        span: 12 << 7,
        notes: &[0, 128, 384, 512, 640, 896, 1024, 1280],
    },
    Scale {
        span: 12 << 7,
        notes: &[0, 128, 640, 896, 1024],
    },
    Scale {
        span: 12 << 7,
        notes: &[0, 128, 384, 896, 1024],
    },
    Scale {
        span: 12 << 7,
        notes: &[0, 256, 384, 768, 896, 1024, 1408],
    },
    Scale {
        span: 12 << 7,
        notes: &[0, 128, 512, 640, 896, 1024, 1408],
    },
    Scale {
        span: 12 << 7,
        notes: &[0, 128, 512, 640, 896, 1024, 1280],
    },
    Scale {
        span: 12 << 7,
        notes: &[0, 256, 512, 768, 1024, 1280],
    },
    Scale {
        span: 12 << 7,
        notes: &[0, 115, 261, 376, 522, 637, 783, 899, 1014, 1160, 1275, 1421],
    },
    Scale {
        span: 12 << 7,
        notes: &[0, 128, 256, 384, 448, 640, 768, 896, 1024, 1152, 1280, 1344],
    },
    Scale {
        span: 12 << 7,
        notes: &[0, 128, 256, 384, 448, 640, 768, 896, 1024, 1152, 1280, 1408],
    },
    Scale {
        span: 12 << 7,
        notes: &[0, 128, 256, 384, 448, 640, 768, 896, 1024, 1088, 1280, 1408],
    },
    Scale {
        span: 12 << 7,
        notes: &[0, 115, 494, 637, 899, 1014, 1393],
    },
    Scale {
        span: 12 << 7,
        notes: &[0, 143, 637, 899, 1042],
    },
    Scale {
        span: 12 << 7,
        notes: &[0, 143, 494, 755, 1132, 1393],
    },
    Scale {
        span: 12 << 7,
        notes: &[0, 115, 494, 755, 899, 1014, 1393],
    },
    Scale {
        span: 12 << 7,
        notes: &[0, 143, 494, 755, 899, 1042, 1393],
    },
    Scale {
        span: 12 << 7,
        notes: &[0, 261, 494, 637, 899, 1160, 1393],
    },
    Scale {
        span: 12 << 7,
        notes: &[0, 261, 522, 783, 899, 1160, 1421],
    },
    Scale {
        span: 12 << 7,
        notes: &[0, 233, 376, 637, 899, 1132, 1275],
    },
    Scale {
        span: 12 << 7,
        notes: &[0, 261, 404, 637, 899, 1160, 1303],
    },
    Scale {
        span: 12 << 7,
        notes: &[0, 261, 376, 637, 899, 1014, 1275],
    },
    Scale {
        span: 12 << 7,
        notes: &[0, 261, 494, 637, 899, 1132, 1275],
    },
    Scale {
        span: 12 << 7,
        notes: &[0, 261, 494, 637, 899, 1160, 1275, 1421],
    },
    Scale {
        span: 12 << 7,
        notes: &[0, 261, 376, 637, 899, 1132, 1275, 1393],
    },
    Scale {
        span: 12 << 7,
        notes: &[0, 115, 376, 637, 1132, 1275],
    },
    Scale {
        span: 12 << 7,
        notes: &[0, 261, 376, 637, 899, 1393],
    },
    Scale {
        span: 12 << 7,
        notes: &[0, 494, 637, 899, 1014, 1275],
    },
    Scale {
        span: 12 << 7,
        notes: &[0, 261, 755, 899, 1132, 1275],
    },
    Scale {
        span: 12 << 7,
        notes: &[0, 261, 376, 637, 899, 1160, 1275],
    },
    Scale {
        span: 12 << 7,
        notes: &[0, 261, 494, 637, 899, 1014, 1393],
    },
    Scale {
        span: 12 << 7,
        notes: &[0, 261, 522, 637, 1014, 1275],
    },
    Scale {
        span: 12 << 7,
        notes: &[0, 115, 637, 899, 1275],
    },
    Scale {
        span: 12 << 7,
        notes: &[0, 115, 376, 899, 1275],
    },
    Scale {
        span: 12 << 7,
        notes: &[0, 376, 637, 899, 1275],
    },
    Scale {
        span: 12 << 7,
        notes: &[0, 376, 637, 755, 1014],
    },
    Scale {
        span: 12 << 7,
        notes: &[0, 376, 494, 637, 1132, 1275],
    },
];
