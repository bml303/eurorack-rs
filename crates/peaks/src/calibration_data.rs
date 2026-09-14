//! `peaks/calibration_data.{h,cc}` -- converts a signed output sample into a
//! DAC code, with a per-channel calibration offset. `Save` (flash write) is
//! dropped, matching every other crate's persistence cut -- a host manages
//! `dac_offset` persistence itself and re-applies it via `set_dac_offset`.

#[derive(Debug, Clone, Copy, Default)]
pub struct CalibrationData {
    dac_offset: [i16; 2],
}

impl CalibrationData {
    pub fn init(&mut self) {
        self.dac_offset = [0; 2];
    }

    pub fn dac_code(&self, channel: usize, value: i16) -> u16 {
        let mut shifted_value: i32 = 32767 - value as i32;
        shifted_value += self.dac_offset[channel] as i32;
        shifted_value.clamp(0, 65535) as u16
    }

    pub fn set_dac_offset(&mut self, channel: usize, offset: i16) {
        self.dac_offset[channel] = offset;
    }
}
