//! `peaks/modulations/multistage_envelope.{h,cc}` -- a multi-segment
//! (AD/AR/ADSR/ADSAR/ADAR, with optional looping) envelope generator.
//! Structurally close to `mi-streams::envelope::Envelope`, but this one's
//! release/sustain transition is real (driven by an actual
//! `GateFlags::FALLING` bit), not the provably-dead branch that crate has.

use crate::gate_processor::GateFlags;
use crate::resources::LOOKUP_TABLE_TABLE;
use stmlib::fixed::interpolate_824_u16;

pub const K_MAX_NUM_SEGMENTS: usize = 8;
/// `LUT_ENV_LINEAR`'s index into `LOOKUP_TABLE_TABLE` (this crate's
/// transpiled table order differs from `mi-frames`'/`mi-streams`', where it
/// happened to be 0).
const LUT_ENV_LINEAR_INDEX: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u16)]
pub enum EnvelopeShape {
    #[default]
    Linear,
    Exponential,
    Quartic,
}

#[derive(Debug, Clone, Copy)]
pub struct MultistageEnvelope {
    level: [i16; K_MAX_NUM_SEGMENTS],
    time: [u16; K_MAX_NUM_SEGMENTS],
    shape: [EnvelopeShape; K_MAX_NUM_SEGMENTS],

    segment: i16,
    start_value: i16,
    value: i16,

    phase: u32,
    phase_increment: u32,

    num_segments: u16,
    sustain_point: u16,
    loop_start: u16,
    loop_end: u16,

    hard_reset: bool,
}

impl Default for MultistageEnvelope {
    fn default() -> Self {
        Self {
            level: [0; K_MAX_NUM_SEGMENTS],
            time: [0; K_MAX_NUM_SEGMENTS],
            shape: [EnvelopeShape::Linear; K_MAX_NUM_SEGMENTS],
            segment: 0,
            start_value: 0,
            value: 0,
            phase: 0,
            phase_increment: 0,
            num_segments: 0,
            sustain_point: 0,
            loop_start: 0,
            loop_end: 0,
            hard_reset: false,
        }
    }
}

impl MultistageEnvelope {
    pub fn init(&mut self) {
        self.set_adsr(0, 8192, 16384, 32767);
        self.segment = self.num_segments as i16;
        self.phase = 0;
        self.phase_increment = 0;
        self.start_value = 0;
        self.value = 0;
        self.hard_reset = false;
    }

    pub fn set_time(&mut self, segment: usize, time: u16) {
        self.time[segment] = time;
    }
    pub fn set_level(&mut self, segment: usize, level: i16) {
        self.level[segment] = level;
    }
    pub fn set_num_segments(&mut self, num_segments: u16) {
        self.num_segments = num_segments;
    }
    pub fn set_sustain_point(&mut self, sustain_point: u16) {
        self.sustain_point = sustain_point;
    }

    pub fn set_adsr(&mut self, attack: u16, decay: u16, sustain: u16, release: u16) {
        self.num_segments = 3;
        self.sustain_point = 2;

        self.level = [0, 32767, sustain as i16, 0, 0, 0, 0, 0];
        self.time[0] = attack;
        self.time[1] = decay;
        self.time[2] = release;

        self.shape[0] = EnvelopeShape::Quartic;
        self.shape[1] = EnvelopeShape::Exponential;
        self.shape[2] = EnvelopeShape::Exponential;

        self.loop_start = 0;
        self.loop_end = 0;
    }

    pub fn set_ad(&mut self, attack: u16, decay: u16) {
        self.num_segments = 2;
        self.sustain_point = 0;

        self.level[0] = 0;
        self.level[1] = 32767;
        self.level[2] = 0;

        self.time[0] = attack;
        self.time[1] = decay;

        self.shape[0] = EnvelopeShape::Exponential;
        self.shape[1] = EnvelopeShape::Exponential;

        self.loop_start = 0;
        self.loop_end = 0;
    }

    pub fn set_adr(&mut self, attack: u16, decay: u16, sustain: u16, release: u16) {
        self.num_segments = 3;
        self.sustain_point = 0;

        self.level = [0, 32767, sustain as i16, 0, 0, 0, 0, 0];
        self.time[0] = attack;
        self.time[1] = decay;
        self.time[2] = release;

        self.shape[0] = EnvelopeShape::Linear;
        self.shape[1] = EnvelopeShape::Linear;
        self.shape[2] = EnvelopeShape::Linear;

        self.loop_start = 0;
        self.loop_end = 0;
    }

    pub fn set_ar(&mut self, attack: u16, decay: u16) {
        self.num_segments = 2;
        self.sustain_point = 1;

        self.level[0] = 0;
        self.level[1] = 32767;
        self.level[2] = 0;

        self.time[0] = attack;
        self.time[1] = decay;

        self.shape[0] = EnvelopeShape::Linear;
        self.shape[1] = EnvelopeShape::Linear;

        self.loop_start = 0;
        self.loop_end = 0;
    }

    pub fn set_adsar(&mut self, attack: u16, decay: u16, sustain: u16, release: u16) {
        self.num_segments = 4;
        self.sustain_point = 2;

        self.level = [0, 32767, sustain as i16, 32767, 0, 0, 0, 0];
        self.time[0] = attack;
        self.time[1] = decay;
        self.time[2] = attack;
        self.time[3] = release;

        self.shape = [EnvelopeShape::Linear; K_MAX_NUM_SEGMENTS];

        self.loop_start = 0;
        self.loop_end = 0;
    }

    pub fn set_adar(&mut self, attack: u16, decay: u16, sustain: u16, release: u16) {
        self.num_segments = 4;
        self.sustain_point = 0;

        self.level = [0, 32767, sustain as i16, 32767, 0, 0, 0, 0];
        self.time[0] = attack;
        self.time[1] = decay;
        self.time[2] = attack;
        self.time[3] = release;

        self.shape = [EnvelopeShape::Linear; K_MAX_NUM_SEGMENTS];

        self.loop_start = 0;
        self.loop_end = 0;
    }

    pub fn set_ad_loop(&mut self, attack: u16, decay: u16) {
        self.num_segments = 2;
        self.sustain_point = 0;

        self.level[0] = 0;
        self.level[1] = 32767;
        self.level[2] = 0;

        self.time[0] = attack;
        self.time[1] = decay;

        self.shape[0] = EnvelopeShape::Linear;
        self.shape[1] = EnvelopeShape::Linear;

        self.loop_start = 0;
        self.loop_end = 2;
    }

    pub fn set_adr_loop(&mut self, attack: u16, decay: u16, sustain: u16, release: u16) {
        self.num_segments = 3;
        self.sustain_point = 0;

        self.level = [0, 32767, sustain as i16, 0, 0, 0, 0, 0];
        self.time[0] = attack;
        self.time[1] = decay;
        self.time[2] = release;

        self.shape[0] = EnvelopeShape::Linear;
        self.shape[1] = EnvelopeShape::Linear;
        self.shape[2] = EnvelopeShape::Linear;

        self.loop_start = 0;
        self.loop_end = 3;
    }

    pub fn set_adar_loop(&mut self, attack: u16, decay: u16, sustain: u16, release: u16) {
        self.num_segments = 4;
        self.sustain_point = 0;

        self.level = [0, 32767, sustain as i16, 32767, 0, 0, 0, 0];
        self.time[0] = attack;
        self.time[1] = decay;
        self.time[2] = attack;
        self.time[3] = release;

        self.shape = [EnvelopeShape::Linear; K_MAX_NUM_SEGMENTS];

        self.loop_start = 0;
        self.loop_end = 4;
    }

    pub fn set_hard_reset(&mut self, hard_reset: bool) {
        self.hard_reset = hard_reset;
    }

    pub fn configure(&mut self, parameter: &[u16; 4], control_mode: crate::gate_processor::ControlMode) {
        if control_mode == crate::gate_processor::ControlMode::Half {
            self.set_ad(parameter[0], parameter[1]);
        } else {
            self.set_adsr(parameter[0], parameter[1], parameter[2] >> 1, parameter[3]);
        }
        if self.segment > self.num_segments as i16 {
            self.segment = 0;
            self.phase = 0;
            self.value = 0;
        }
    }

    pub fn process(&mut self, gate_flags: &[GateFlags], out: &mut [i16]) {
        for (&gate_flag, o) in gate_flags.iter().zip(out.iter_mut()) {
            if gate_flag.contains(GateFlags::RISING) {
                self.start_value = if self.segment == self.num_segments as i16 || self.hard_reset { self.level[0] } else { self.value };
                self.segment = 0;
                self.phase = 0;
            } else if gate_flag.contains(GateFlags::FALLING) && self.sustain_point != 0 {
                self.start_value = self.value;
                self.segment = self.sustain_point as i16;
                self.phase = 0;
            } else if self.phase < self.phase_increment {
                self.start_value = self.level[self.segment as usize + 1];
                self.segment += 1;
                self.phase = 0;
                if self.segment == self.loop_end as i16 {
                    self.segment = self.loop_start as i16;
                }
            }

            let done = self.segment == self.num_segments as i16;
            let sustained = self.sustain_point != 0 && self.segment == self.sustain_point as i16 && gate_flag.contains(GateFlags::HIGH);

            self.phase_increment = if sustained || done { 0 } else { crate::resources::LUT_ENV_INCREMENTS[(self.time[self.segment as usize] >> 8) as usize] };

            let a = self.start_value as i32;
            let b = self.level[self.segment as usize + 1] as i32;
            let t = interpolate_824_u16(LOOKUP_TABLE_TABLE[LUT_ENV_LINEAR_INDEX + self.shape[self.segment as usize] as usize], self.phase);
            self.value = a.wrapping_add((b.wrapping_sub(a)).wrapping_mul((t >> 1) as i32) >> 15) as i16;
            self.phase = self.phase.wrapping_add(self.phase_increment);
            *o = self.value;
        }
    }
}
