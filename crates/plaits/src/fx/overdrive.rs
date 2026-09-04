//! `plaits/dsp/fx/overdrive.h` -- a soft-clip distortion with drive-dependent
//! pre/post gain staging so the output level stays roughly constant.

use stmlib::fdsp::soft_clip;
use stmlib::parameter_interpolator::ParameterInterpolator;

#[derive(Debug, Clone, Copy, Default)]
pub struct Overdrive {
    pre_gain: f32,
    post_gain: f32,
}

impl Overdrive {
    pub fn init(&mut self) {
        self.pre_gain = 0.0;
        self.post_gain = 0.0;
    }

    pub fn process(&mut self, drive: f32, in_out: &mut [f32]) {
        let drive_2 = drive * drive;
        let pre_gain_a = drive * 0.5;
        let pre_gain_b = drive_2 * drive_2 * drive * 24.0;
        let pre_gain = pre_gain_a + (pre_gain_b - pre_gain_a) * drive_2;
        let drive_squashed = drive * (2.0 - drive);
        let post_gain = 1.0 / soft_clip(0.33 + drive_squashed * (pre_gain - 0.33));

        let size = in_out.len();
        let mut pre_gain_modulation = ParameterInterpolator::new(&mut self.pre_gain, pre_gain, size);
        let mut post_gain_modulation = ParameterInterpolator::new(&mut self.post_gain, post_gain, size);

        for s in in_out.iter_mut() {
            let pre = pre_gain_modulation.next() * *s;
            *s = soft_clip(pre) * post_gain_modulation.next();
        }
    }
}
