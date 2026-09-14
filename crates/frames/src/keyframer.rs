//! `frames/keyframer.{h,cc}` -- the keyframe interpolator: a sorted list of
//! (timestamp, 4-channel value) keyframes, evaluated at an arbitrary
//! timestamp by locating the surrounding pair and easing between them.

use crate::resources::{LOOKUP_TABLE_TABLE, LUT_EXPONENTIAL, LUT_RESPONSE_BALANCE, LUT_VCA_LINEAR};

pub const NUM_CHANNELS: usize = 4;
pub const MAX_NUM_KEYFRAME: usize = 64;
pub const NUM_PALETTE_ENTRIES: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum EasingCurve {
    Step,
    #[default]
    Linear,
    InQuartic,
    OutQuartic,
    Sine,
    Bounce,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ChannelSettings {
    pub easing_curve: EasingCurve,
    pub response: u8,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Keyframe {
    pub timestamp: u16,
    pub id: u16,
    pub values: [u16; NUM_CHANNELS],
}

const PALETTE: [[u8; 3]; NUM_PALETTE_ENTRIES] = [
    [255, 0, 0],
    [255, 64, 0],
    [255, 255, 0],
    [64, 255, 0],
    [0, 255, 64],
    [0, 0, 255],
    [255, 0, 255],
    [255, 0, 64],
];

pub struct Keyframer {
    keyframes: [Keyframe; MAX_NUM_KEYFRAME],
    settings: [ChannelSettings; NUM_CHANNELS],
    num_keyframes: u16,
    id_counter: u16,
    extra_settings: u32,
    dc_offset_frame_modulation: i32,

    position: i16,
    nearest_keyframe: i16,

    dac_code: [u16; NUM_CHANNELS],
    levels: [u16; NUM_CHANNELS],
    immediate: [u16; NUM_CHANNELS],

    color: [u8; 3],
}

impl Default for Keyframer {
    fn default() -> Self {
        Self {
            keyframes: [Keyframe::default(); MAX_NUM_KEYFRAME],
            settings: [ChannelSettings::default(); NUM_CHANNELS],
            num_keyframes: 0,
            id_counter: 0,
            extra_settings: 0,
            dc_offset_frame_modulation: 32767,
            position: -1,
            nearest_keyframe: -1,
            dac_code: [0; NUM_CHANNELS],
            levels: [0; NUM_CHANNELS],
            immediate: [0; NUM_CHANNELS],
            color: [0; 3],
        }
    }
}

impl Keyframer {
    pub fn new() -> Self {
        Self::default()
    }

    /// The C's `Init` loads persisted settings from flash (out of scope,
    /// like every other crate's `LoadSettings`/storage layer) and falls
    /// back to these defaults only when that load fails; here it's always
    /// the defaults.
    pub fn init(&mut self) {
        for s in self.settings.iter_mut() {
            s.easing_curve = EasingCurve::Linear;
            s.response = 0;
        }
        self.extra_settings = 0;
        self.dc_offset_frame_modulation = 32767;
        self.clear();
    }

    /// `Save`/`Calibrate` minus the flash write -- a host manages its own
    /// persistence and re-applies these on the next `init`-equivalent.
    pub fn set_extra_settings(&mut self, extra_settings: u32) {
        self.extra_settings = extra_settings;
    }
    pub fn calibrate(&mut self, dc_offset_frame_modulation: i32) {
        self.dc_offset_frame_modulation = dc_offset_frame_modulation;
    }

    pub fn clear(&mut self) {
        self.keyframes = [Keyframe::default(); MAX_NUM_KEYFRAME];
        self.num_keyframes = 0;
        self.id_counter = 0;
    }

    /// `std::lower_bound` by timestamp: the first index whose timestamp is
    /// not less than `timestamp` (or `num_keyframes()` if none is).
    fn find_keyframe(&self, timestamp: u16) -> u16 {
        if self.num_keyframes == 0 {
            return 0;
        }
        self.keyframes[..self.num_keyframes as usize].partition_point(|k| k.timestamp < timestamp) as u16
    }

    pub fn set_immediate(&mut self, channel: usize, value: u16) {
        self.immediate[channel] = value;
    }

    pub fn dac_code(&self, channel: usize) -> u16 {
        self.dac_code[channel]
    }
    pub fn level(&self, channel: usize) -> u16 {
        self.levels[channel]
    }
    pub fn color(&self) -> [u8; 3] {
        self.color
    }

    pub fn mutable_settings(&mut self, channel: usize) -> &mut ChannelSettings {
        &mut self.settings[channel]
    }
    pub fn settings(&self, channel: usize) -> &ChannelSettings {
        &self.settings[channel]
    }

    pub fn mutable_keyframe(&mut self, index: usize) -> &mut Keyframe {
        &mut self.keyframes[index]
    }
    pub fn keyframe(&self, index: usize) -> &Keyframe {
        &self.keyframes[index]
    }

    pub fn num_keyframes(&self) -> u16 {
        self.num_keyframes
    }
    pub fn position(&self) -> i16 {
        self.position
    }
    pub fn nearest_keyframe(&self) -> i16 {
        self.nearest_keyframe
    }
    pub fn extra_settings(&self) -> u32 {
        self.extra_settings
    }
    pub fn dc_offset_frame_modulation(&self) -> i32 {
        self.dc_offset_frame_modulation
    }

    pub fn find_nearest_keyframe(&self, timestamp: u16, tolerance: u16) -> i16 {
        if self.num_keyframes == 0 {
            return -1;
        }
        let index = self.find_keyframe(timestamp);
        let search_start = if index != 0 { index - 1 } else { 0 };
        let search_end = if index < self.num_keyframes - 1 { index + 2 } else { self.num_keyframes };
        for i in search_start..search_end {
            let t = self.keyframes[i as usize].timestamp;
            let distance = t as i32 - timestamp as i32;
            if distance < tolerance as i32 && -distance < tolerance as i32 {
                return i as i16;
            }
        }
        -1
    }

    pub fn add_keyframe(&mut self, timestamp: u16, values: &[u16; NUM_CHANNELS]) -> bool {
        if self.num_keyframes as usize == MAX_NUM_KEYFRAME {
            return false;
        }

        let insertion_point = self.find_keyframe(timestamp);
        // `||` short-circuits exactly as in the C++, so the array read on
        // the right never runs when `insertion_point >= num_keyframes_`.
        if insertion_point >= self.num_keyframes || self.keyframes[insertion_point as usize].timestamp != timestamp {
            let mut i = self.num_keyframes as i32 - 1;
            while i >= insertion_point as i32 {
                self.keyframes[(i + 1) as usize] = self.keyframes[i as usize];
                i -= 1;
            }
            self.keyframes[insertion_point as usize].timestamp = timestamp;
            self.keyframes[insertion_point as usize].id = self.id_counter;
            self.id_counter = self.id_counter.wrapping_add(1);
            self.num_keyframes += 1;
        }
        self.keyframes[insertion_point as usize].values = *values;
        true
    }

    pub fn remove_keyframe(&mut self, timestamp: u16) -> bool {
        if self.num_keyframes == 0 {
            return false;
        }
        let splice_point = self.find_keyframe(timestamp);
        // The C reads `keyframes_[splice_point]` unconditionally here; when
        // `splice_point == num_keyframes_` (timestamp past every existing
        // keyframe) and the list is full (64/64), that's one past the
        // array's last valid index. `find_keyframe` landing at
        // `num_keyframes_` already means "no keyframe has this timestamp",
        // so bail out first instead of reading past the end.
        if splice_point >= self.num_keyframes {
            return false;
        }
        if self.keyframes[splice_point as usize].timestamp != timestamp {
            return false;
        }

        for i in splice_point..self.num_keyframes - 1 {
            self.keyframes[i as usize] = self.keyframes[(i + 1) as usize];
        }
        self.num_keyframes -= 1;
        true
    }

    pub fn evaluate(&mut self, timestamp: u16) {
        if self.num_keyframes == 0 {
            self.levels = self.immediate;
            self.color = [0xff; 3];
            self.position = -1;
            self.nearest_keyframe = -1;
        } else {
            let position = self.find_keyframe(timestamp);
            self.position = position as i16;

            // Check for the areas before the first keyframe, and after the
            // last keyframe.
            if position == 0 || position == self.num_keyframes {
                let source = self.keyframes[if position == 0 { 0 } else { self.num_keyframes as usize - 1 }];
                self.levels = source.values;
                self.color = PALETTE[(source.id as usize) & (NUM_PALETTE_ENTRIES - 1)];
            } else {
                // This is where the real interpolation takes place.
                let a = self.keyframes[position as usize - 1];
                let b = self.keyframes[position as usize];
                let mut scale = timestamp.wrapping_sub(a.timestamp) as u32;
                scale <<= 16;
                scale /= b.timestamp.wrapping_sub(a.timestamp) as u32;
                for i in 0..NUM_CHANNELS {
                    let from = a.values[i] as i32;
                    let to = b.values[i] as i32;
                    self.levels[i] = Self::easing(from, to, scale, self.settings[i].easing_curve);
                }
                let a_palette = PALETTE[(a.id as usize) & (NUM_PALETTE_ENTRIES - 1)];
                let b_palette = PALETTE[(b.id as usize) & (NUM_PALETTE_ENTRIES - 1)];
                for ((c, &a_color), &b_color) in self.color.iter_mut().zip(a_palette.iter()).zip(b_palette.iter()) {
                    let a_color = a_color as i32;
                    let b_color = b_color as i32;
                    *c = a_color.wrapping_add(b_color.wrapping_sub(a_color).wrapping_mul(scale as i32) >> 16) as u8;
                }
            }

            // The C reads `keyframes_[position]` unconditionally here too
            // (outside the `if`/`else` above), which is the same
            // one-past-the-end read as `remove_keyframe` when `position ==
            // num_keyframes_` and the list is full. There's no keyframe
            // after the last one in that case, so `nearest_keyframe_` is
            // just `position` -- skip the (undefined) "is there a closer
            // keyframe after this one" comparison entirely instead of
            // reading past the array.
            self.nearest_keyframe = if position < self.num_keyframes {
                let t_this = timestamp.wrapping_sub(if position == 0 { 0 } else { self.keyframes[position as usize - 1].timestamp });
                let t_next = self.keyframes[position as usize].timestamp.wrapping_sub(timestamp);
                if t_next < t_this {
                    position as i16 + 1
                } else {
                    position as i16
                }
            } else {
                position as i16
            };
        }

        for i in 0..NUM_CHANNELS {
            self.dac_code[i] = Self::convert_to_dac_code(self.levels[i], self.settings[i].response);
        }
    }

    pub fn convert_to_dac_code(gain: u16, response: u8) -> u16 {
        // Exponential response is easy, straight to the 2164.
        let exponential = 65535i32 - gain as i32;

        // Use a lookup table and interpolation to linearize the 2164.
        let idx = (gain >> 6) as usize;
        let a = LUT_VCA_LINEAR[idx] as i32;
        let b = LUT_VCA_LINEAR[idx + 1] as i32;
        let frac = (((gain as u32) << 10) & 0xffff) as i32;
        let linear = a.wrapping_add(b.wrapping_sub(a).wrapping_mul(frac) >> 16);

        // Blend linear and exponential responses.
        let balance = LUT_RESPONSE_BALANCE[response as usize] as i32;
        (linear.wrapping_add(exponential.wrapping_sub(linear).wrapping_mul(balance) >> 15) >> 4) as u16
    }

    fn easing(from: i32, to: i32, scale: u32, curve: EasingCurve) -> u16 {
        let shaped_scale: i32 = if curve == EasingCurve::Step {
            if scale < 32768 {
                0
            } else {
                65535
            }
        } else if curve as u8 >= EasingCurve::InQuartic as u8 {
            let table = LOOKUP_TABLE_TABLE[curve as usize - EasingCurve::InQuartic as usize];
            // `scale` can legitimately be exactly 65536 (interpolating
            // right up to a keyframe's own timestamp, i.e. evaluating
            // exactly at a keyframe boundary -- reachable in ordinary use,
            // and the maximum `scale` ever takes: `timestamp <=
            // b.timestamp` always holds, so the ratio computed in
            // `evaluate` never exceeds 1.0). At that point `scale >> 6` is
            // 1024, the table's true last valid index -- give `scale_a`
            // that value verbatim (clamping it down to 1023 like `scale_b`
            // below would read the *wrong* entry, not just an unused one).
            // `scale_b` (`table[idx + 1]`) would then be one past the end;
            // the C reads that OOB byte too, but at this exact boundary the
            // fractional weight below is always zero, so its value never
            // affects the result -- reusing the last entry there instead
            // reproduces the identical result without the OOB read.
            let idx = ((scale >> 6) as usize).min(table.len() - 1);
            let scale_a = table[idx] as i32;
            let scale_b = table[(idx + 1).min(table.len() - 1)] as i32;
            let frac = ((scale << 10) & 0xffff) as i32;
            scale_a.wrapping_add(((scale_b.wrapping_sub(scale_a) >> 1).wrapping_mul(frac)) >> 15)
        } else {
            scale as i32
        };
        from.wrapping_add(to.wrapping_sub(from).wrapping_mul(shaped_scale >> 1) >> 15) as u16
    }

    /// Sample animation (0 to 65535 and back to 0) used for animating an
    /// LED when editing the easing curve or response.
    pub fn sample_animation(&self, channel: usize, tick: u16, easing_on: bool) -> u16 {
        let from = if tick > 32768 { 65535 } else { 0 };
        let to = if tick > 32768 { 0 } else { 65535 };
        let scale = ((tick as u32) << 1) & 0xffff;
        let curve = if easing_on { self.settings[channel].easing_curve } else { EasingCurve::Linear };
        let mut sample = Self::easing(from, to, scale, curve);
        if !easing_on {
            let linear = sample as i32;
            let exponential = LUT_EXPONENTIAL[(sample >> 8) as usize] as i32;
            let balance = LUT_RESPONSE_BALANCE[self.settings[channel].response as usize] as i32;
            sample = linear.wrapping_add(exponential.wrapping_sub(linear).wrapping_mul(balance) >> 15) as u16;
        }
        sample
    }
}
