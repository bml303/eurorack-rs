//! `streams/filter_controller.h` -- the "plain filter" processor function:
//! just derives a frequency from `excite`, gain always zero.

#[derive(Debug, Clone, Copy, Default)]
pub struct FilterController {
    target_frequency_amount: i32,
    target_frequency_offset: i32,
    frequency_amount: i32,
    frequency_offset: i32,
}

impl FilterController {
    pub fn init(&mut self) {
        self.frequency_offset = 0;
        self.frequency_amount = 0;
    }

    pub fn process(&mut self, _audio: i16, excite: i16, gain: &mut u16, frequency: &mut u16) {
        // Smooth frequency amount parameters.
        self.frequency_amount = self.frequency_amount.wrapping_add((self.target_frequency_amount - self.frequency_amount) >> 8);
        self.frequency_offset = self.frequency_offset.wrapping_add((self.target_frequency_offset - self.frequency_offset) >> 8);

        let f = self.frequency_offset.wrapping_add((excite as i32).wrapping_mul(self.frequency_amount) >> 14);
        *gain = 0;
        *frequency = f.clamp(0, 65535) as u16;
    }

    pub fn configure(&mut self, _alternate: bool, parameters: &[i32; 2], _globals: Option<&[i32; 4]>) {
        let mut amount = parameters[1];
        amount -= 32768;
        amount = amount.wrapping_mul(amount) >> 15;
        self.target_frequency_amount = if parameters[1] < 32768 { -amount } else { amount };
        self.target_frequency_offset = parameters[0];
    }
}
