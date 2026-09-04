//! `stmlib/dsp/hysteresis_quantizer.h` -- quantize a float to an integer step,
//! with hysteresis around the decision boundary so noise near a threshold
//! doesn't cause chatter.

#[derive(Debug, Clone, Copy, Default)]
pub struct HysteresisQuantizer {
    quantized_value: i32,
}

impl HysteresisQuantizer {
    pub fn init(&mut self) {
        self.quantized_value = 0;
    }

    pub fn process(&mut self, value: f32, num_steps: i32) -> i32 {
        self.process_hys(value, num_steps, 0.25)
    }

    pub fn process_hys(&mut self, value: f32, num_steps: i32, hysteresis: f32) -> i32 {
        self.process_base(0, value, num_steps, hysteresis)
    }

    pub fn process_base(&mut self, base: i32, value: f32, num_steps: i32, hysteresis: f32) -> i32 {
        let mut value = value * (num_steps - 1) as f32;
        value += base as f32;
        let hysteresis_feedback = if value > self.quantized_value as f32 {
            -hysteresis
        } else {
            hysteresis
        };
        let q = (value + hysteresis_feedback + 0.5) as i32;
        let q = q.clamp(0, num_steps - 1);
        self.quantized_value = q;
        q
    }
}

/// `HysteresisQuantizer2` -- the newer, `Init(num_steps, hysteresis, symmetric)`
/// version used by Plaits' engine selector.
#[derive(Debug, Clone, Copy, Default)]
pub struct HysteresisQuantizer2 {
    num_steps: i32,
    hysteresis: f32,
    scale: f32,
    offset: f32,
    quantized_value: i32,
}

impl HysteresisQuantizer2 {
    pub fn init(&mut self, num_steps: i32, hysteresis: f32, symmetric: bool) {
        self.num_steps = num_steps;
        self.hysteresis = hysteresis;
        self.scale = if symmetric { (num_steps - 1) as f32 } else { num_steps as f32 };
        self.offset = if symmetric { 0.0 } else { -0.5 };
        self.quantized_value = 0;
    }

    pub fn process(&mut self, value: f32) -> i32 {
        self.process_base(0, value)
    }

    pub fn process_base(&mut self, base: i32, value: f32) -> i32 {
        let mut value = value * self.scale;
        value += self.offset;
        value += base as f32;
        let hysteresis_sign = if value > self.quantized_value as f32 {
            -1.0
        } else {
            1.0
        };
        let q = (value + hysteresis_sign * self.hysteresis + 0.5) as i32;
        let q = q.clamp(0, self.num_steps - 1);
        self.quantized_value = q;
        q
    }

    #[inline]
    pub fn num_steps(&self) -> i32 {
        self.num_steps
    }

    #[inline]
    pub fn quantized_value(&self) -> i32 {
        self.quantized_value
    }
}
