//! `marbles/ramp/ramp_generator.h` -- simple ramp generator.

#[derive(Debug, Default, Clone, Copy)]
pub struct RampGenerator {
    phase: f32,
}

impl RampGenerator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn init(&mut self) {
        self.phase = 0.0;
    }

    pub fn render(&mut self, frequency: f32, out: &mut [f32]) {
        for sample in out.iter_mut() {
            self.phase += frequency;
            if self.phase >= 1.0 {
                self.phase -= 1.0;
            }
            *sample = self.phase;
        }
    }
}
