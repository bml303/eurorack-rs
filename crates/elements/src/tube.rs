//! `elements/dsp/tube.{h,cc}` -- a one-delay-line waveguide reed/tube model,
//! used by [`Voice`](crate::Voice) as the "blow" exciter's body resonance when
//! the blow level is pushed past unity.

use alloc::boxed::Box;

use stmlib::constrain;

const TUBE_DELAY_SIZE: usize = 2048;

/// `elements::Tube`.
#[derive(Debug, Clone)]
pub struct Tube {
    delay_ptr: i32,
    zero_state: f32,
    pole_state: f32,
    delay_line: Box<[f32; TUBE_DELAY_SIZE]>,
}

impl Default for Tube {
    fn default() -> Self {
        Self::new()
    }
}

impl Tube {
    pub fn new() -> Self {
        Self {
            delay_ptr: 0,
            zero_state: 0.0,
            pole_state: 0.0,
            delay_line: Box::new([0.0; TUBE_DELAY_SIZE]),
        }
    }

    /// `Init`.
    pub fn init(&mut self) {
        self.zero_state = 0.0;
        self.pole_state = 0.0;
        self.delay_ptr = 0;
        self.delay_line.fill(0.0);
    }

    /// `Process` -- adds `gain * envelope * lp(tube_output)` into `input_output`.
    pub fn process(
        &mut self,
        frequency: f32,
        mut envelope: f32,
        damping: f32,
        timbre: f32,
        input_output: &mut [f32],
        gain: f32,
    ) {
        let mut delay = 1.0 / frequency;
        while delay >= TUBE_DELAY_SIZE as f32 {
            delay *= 0.5;
        }
        let delay_integral = delay as usize;
        let delay_fractional = delay - delay_integral as f32;

        if envelope >= 1.0 {
            envelope = 1.0;
        }

        let damping = 3.6 - damping * 1.8;
        let mut lpf_coefficient = frequency * (1.0 + timbre * timbre * 256.0);
        if lpf_coefficient >= 0.995 {
            lpf_coefficient = 0.995;
        }

        let mut d = self.delay_ptr;
        for s in input_output.iter_mut() {
            let breath = *s * damping + 0.8;
            let a = self.delay_line[(d as usize + delay_integral) % TUBE_DELAY_SIZE];
            let b = self.delay_line[(d as usize + delay_integral + 1) % TUBE_DELAY_SIZE];
            let input = a + (b - a) * delay_fractional;
            let pressure_delta = -0.95 * (input * envelope + self.zero_state) - breath;
            self.zero_state = input;

            let reed = pressure_delta * -0.2 + 0.8;
            let out = constrain(pressure_delta * reed + breath, -5.0, 5.0);

            self.delay_line[d as usize] = out * 0.5;

            d -= 1;
            if d < 0 {
                d = TUBE_DELAY_SIZE as i32 - 1;
            }
            self.pole_state += lpf_coefficient * (out - self.pole_state);
            *s += gain * envelope * self.pole_state;
        }
        self.delay_ptr = d;
    }
}
