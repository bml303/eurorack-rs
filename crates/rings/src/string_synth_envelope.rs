//! `rings/dsp/string_synth_envelope.h` -- a tiny rate-based segment envelope
//! (only its AD / AR configs are used) for the string-synth voice groups.

/// Segment interpolation shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EnvelopeShape {
    Linear,
    Quartic,
}

pub const FLAG_RISING_EDGE: u8 = 1;
pub const FLAG_FALLING_EDGE: u8 = 2;
pub const FLAG_GATE: u8 = 4;

/// `rings::StringSynthEnvelope`.
#[derive(Debug, Clone)]
pub struct StringSynthEnvelope {
    level: [f32; 4],
    rate: [f32; 4],
    shape: [EnvelopeShape; 4],

    segment: i32,
    start_value: f32,
    value: f32,
    phase: f32,

    num_segments: i32,
    sustain_point: i32,
}

impl Default for StringSynthEnvelope {
    fn default() -> Self {
        Self::new()
    }
}

impl StringSynthEnvelope {
    pub fn new() -> Self {
        let mut e = Self {
            level: [0.0; 4],
            rate: [0.0; 4],
            shape: [EnvelopeShape::Linear; 4],
            segment: 0,
            start_value: 0.0,
            value: 0.0,
            phase: 0.0,
            num_segments: 0,
            sustain_point: 0,
        };
        e.init();
        e
    }

    /// `Init`.
    pub fn init(&mut self) {
        self.set_ad(0.1, 0.001);
        self.segment = self.num_segments;
        self.phase = 0.0;
        self.start_value = 0.0;
        self.value = 0.0;
    }

    /// `set_ad(attack, decay)`.
    pub fn set_ad(&mut self, attack: f32, decay: f32) {
        self.num_segments = 2;
        self.sustain_point = 0;
        self.level = [0.0, 1.0, 0.0, 0.0];
        self.rate[0] = attack;
        self.rate[1] = decay;
        self.shape[0] = EnvelopeShape::Linear;
        self.shape[1] = EnvelopeShape::Quartic;
    }

    /// `set_ar(attack, decay)`.
    pub fn set_ar(&mut self, attack: f32, decay: f32) {
        self.num_segments = 2;
        self.sustain_point = 1;
        self.level = [0.0, 1.0, 0.0, 0.0];
        self.rate[0] = attack;
        self.rate[1] = decay;
        self.shape[0] = EnvelopeShape::Linear;
        self.shape[1] = EnvelopeShape::Linear;
    }

    /// `Process(flags)`.
    pub fn process(&mut self, flags: u8) -> f32 {
        if flags & FLAG_RISING_EDGE != 0 {
            self.start_value = if self.segment == self.num_segments {
                self.level[0]
            } else {
                self.value
            };
            self.segment = 0;
            self.phase = 0.0;
        } else if flags & FLAG_FALLING_EDGE != 0 && self.sustain_point != 0 {
            self.start_value = self.value;
            self.segment = self.sustain_point;
            self.phase = 0.0;
        } else if self.phase >= 1.0 {
            self.start_value = self.level[self.segment as usize + 1];
            self.segment += 1;
            self.phase = 0.0;
        }

        let done = self.segment == self.num_segments;
        let sustained =
            self.sustain_point != 0 && self.segment == self.sustain_point && flags & FLAG_GATE != 0;

        let seg = self.segment as usize;
        let mut phase_increment = 0.0;
        if !sustained && !done {
            phase_increment = self.rate[seg];
        }
        let mut t = self.phase;
        if self.shape[seg] == EnvelopeShape::Quartic {
            t = 1.0 - t;
            t *= t;
            t *= t;
            t = 1.0 - t;
        }

        self.phase += phase_increment;
        self.value = self.start_value + (self.level[seg + 1] - self.start_value) * t;
        self.value
    }
}
