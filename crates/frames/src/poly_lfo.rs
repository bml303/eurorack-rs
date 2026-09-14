//! `frames/poly_lfo.{h,cc}` -- a 4-channel wavetable LFO with adjustable
//! inter-channel phase spread, per-channel shape spread across the
//! wavetable, and cross-channel phase coupling.
//!
//! [`PolyLfo::render`]'s `frequency` parameter is documented (and only
//! ever driven, in the firmware) as non-negative -- the UI clamps its own
//! `frame()` value to `[0, 65535]` before calling `Render`. A negative
//! frequency would make [`PolyLfo::frequency_to_phase_increment`]'s
//! internal octave-shift computation go negative too, which has no
//! sensible reinterpretation as a shift amount.

use stmlib::fixed::{crossfade_u8, interpolate_824_u8};

use crate::keyframer::{Keyframer, NUM_CHANNELS};
use crate::resources::{LUT_INCREMENTS, WT_LFO_WAVEFORMS};

const RAINBOW: [[u8; 3]; 17] = [
    [255, 0, 0],
    [255, 32, 0],
    [255, 192, 0],
    [255, 240, 0],
    [240, 255, 0],
    [192, 255, 0],
    [32, 255, 0],
    [0, 255, 0],
    [0, 255, 32],
    [0, 255, 192],
    [0, 255, 255],
    [0, 192, 255],
    [0, 32, 255],
    [0, 0, 255],
    [32, 0, 255],
    [192, 0, 192],
    [255, 0, 128],
];

pub struct PolyLfo {
    shape: u16,
    shape_spread: i16,
    spread: i32,
    coupling: i16,

    value: [i16; NUM_CHANNELS],
    phase: [u32; NUM_CHANNELS],
    level: [u8; NUM_CHANNELS],
    dac_code: [u16; NUM_CHANNELS],
    color: [u8; 3],
}

impl Default for PolyLfo {
    fn default() -> Self {
        Self {
            shape: 0,
            shape_spread: 0,
            spread: 0,
            coupling: 0,
            value: [0; NUM_CHANNELS],
            phase: [0; NUM_CHANNELS],
            level: [0; NUM_CHANNELS],
            dac_code: [0; NUM_CHANNELS],
            color: [0; 3],
        }
    }
}

impl PolyLfo {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn init(&mut self) {
        self.spread = 0;
        self.shape = 0;
        self.shape_spread = 0;
        self.coupling = 0;
        self.value = [0; NUM_CHANNELS];
    }

    pub fn set_shape(&mut self, shape: u16) {
        self.shape = shape;
    }

    pub fn set_shape_spread(&mut self, shape_spread: u16) {
        let x = shape_spread as i32 - 32768;
        self.shape_spread = (x as i16) >> 1;
    }

    pub fn set_spread(&mut self, spread: u16) {
        self.spread = if spread < 32768 {
            let x = spread as i32 - 32768;
            let scaled = -((x.wrapping_mul(x)) >> 15);
            (x.wrapping_add(3i32.wrapping_mul(scaled))) >> 2
        } else {
            spread as i32 - 32768
        };
    }

    pub fn set_coupling(&mut self, coupling: u16) {
        let x = coupling as i32 - 32768;
        let mut scaled = x.wrapping_mul(x) >> 15;
        scaled = if x > 0 { scaled } else { -scaled };
        scaled = (x.wrapping_add(3i32.wrapping_mul(scaled))) >> 2;
        self.coupling = ((scaled >> 4).wrapping_mul(10)) as i16;
    }

    pub fn level(&self, index: usize) -> u8 {
        self.level[index]
    }
    pub fn color(&self) -> [u8; 3] {
        self.color
    }
    pub fn dac_code(&self, index: usize) -> u16 {
        self.dac_code[index]
    }

    pub fn frequency_to_phase_increment(frequency: i32) -> u32 {
        let shifts = frequency / 5040;
        let index = frequency - shifts * 5040;
        let idx = (index >> 5) as usize;
        let a = LUT_INCREMENTS[idx];
        let b = LUT_INCREMENTS[idx + 1];
        let frac = (index & 0x1f) as u32;
        let base = a.wrapping_add(b.wrapping_sub(a).wrapping_mul(frac) >> 5);
        base.wrapping_shl(shifts as u32)
    }

    pub fn render(&mut self, frequency: i32) {
        let rainbow_index = frequency.clamp(0, 65535) as u16;
        let rainbow_a = RAINBOW[(rainbow_index >> 12) as usize];
        let rainbow_b = RAINBOW[(rainbow_index >> 12) as usize + 1];
        for ((c, &a), &b) in self.color.iter_mut().zip(rainbow_a.iter()).zip(rainbow_b.iter()) {
            let a = a as i32;
            let b = b as i32;
            *c = a.wrapping_add(b.wrapping_sub(a).wrapping_mul((rainbow_index & 0x0fff) as i32) >> 12) as u8;
        }

        // Advance phasors.
        let mut frequency = frequency;
        if self.spread >= 0 {
            self.phase[0] = self.phase[0].wrapping_add(Self::frequency_to_phase_increment(frequency));
            let phase_difference = (self.spread as u32).wrapping_shl(15);
            self.phase[1] = self.phase[0].wrapping_add(phase_difference);
            self.phase[2] = self.phase[1].wrapping_add(phase_difference);
            self.phase[3] = self.phase[2].wrapping_add(phase_difference);
        } else {
            for i in 0..NUM_CHANNELS {
                self.phase[i] = self.phase[i].wrapping_add(Self::frequency_to_phase_increment(frequency));
                frequency = frequency.wrapping_sub((5040i32.wrapping_mul(self.spread)) >> 15);
            }
        }

        let sine = &WT_LFO_WAVEFORMS[17 * 257..17 * 257 + 257];

        let mut wavetable_index = self.shape;
        for i in 0..NUM_CHANNELS {
            let mut phase = self.phase[i];
            if self.coupling > 0 {
                let contrib = (self.value[(i + 1) % NUM_CHANNELS] as i32).wrapping_mul(self.coupling as i32);
                phase = phase.wrapping_add(contrib as u32);
            } else {
                let contrib = (self.value[(i + NUM_CHANNELS - 1) % NUM_CHANNELS] as i32).wrapping_mul((-self.coupling) as i32);
                phase = phase.wrapping_add(contrib as u32);
            }

            let table_offset = (wavetable_index >> 12) as usize * 257;
            let table_a = &WT_LFO_WAVEFORMS[table_offset..table_offset + 257];
            let table_b = &WT_LFO_WAVEFORMS[table_offset + 257..table_offset + 514];
            let value = crossfade_u8(table_a, table_b, phase, wavetable_index << 4);
            self.value[i] = interpolate_824_u8(sine, phase);
            self.level[i] = ((value as i32).wrapping_add(32768) >> 8) as u8;
            self.dac_code[i] = Keyframer::convert_to_dac_code((value as i32).wrapping_add(32768) as u16, 0);
            wavetable_index = wavetable_index.wrapping_add(self.shape_spread as u16);
        }
    }
}
