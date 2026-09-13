//! `tides2/ramp_generator.h` -- generates up to `NUM_CHANNELS` related ramps
//! (phase counters), in lockstep or with various frequency/slope ratios
//! between channels.
//!
//! The C makes `RampMode`/`OutputMode`/`Range`/`use_ramp` template parameters
//! of `Step`, instantiated into a function-pointer table so the firmware pays
//! for exactly the specialisations it uses (and can place the hot ones in
//! RAM). That's a code-size/placement optimisation, not a semantic
//! requirement, so this port collapses it back to runtime `match`es --
//! `use_ramp` becomes `ramp: Option<f32>` (the two were always 1:1 at the C
//! call sites: `Step<..., true>(f0, pw, GATE_FLAG_LOW, ramp[i])` vs.
//! `Step<..., false>(f0, pw, gate_flags[i], 0.0)`).

use stmlib::gate_flags::GateFlags;

use crate::ratio::Ratio;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RampMode {
    Ad,
    Looping,
    Ar,
}

pub const RAMP_MODE_LAST: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    Gates,
    Amplitude,
    SlopePhase,
    Frequency,
}

pub const OUTPUT_MODE_LAST: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Range {
    Control,
    Audio,
}

pub const RANGE_LAST: usize = 2;

pub struct RampGenerator<const NUM_CHANNELS: usize> {
    next_ratio: [Ratio; NUM_CHANNELS],

    master_phase: f32,
    wrap_counter: [i32; NUM_CHANNELS],

    phase: [f32; NUM_CHANNELS],
    frequency: [f32; NUM_CHANNELS],
    ratio: [Ratio; NUM_CHANNELS],
}

impl<const NUM_CHANNELS: usize> Default for RampGenerator<NUM_CHANNELS> {
    fn default() -> Self {
        let mut g = RampGenerator {
            next_ratio: [Ratio::default(); NUM_CHANNELS],
            master_phase: 0.0,
            wrap_counter: [0; NUM_CHANNELS],
            phase: [0.0; NUM_CHANNELS],
            frequency: [0.0; NUM_CHANNELS],
            ratio: [Ratio::default(); NUM_CHANNELS],
        };
        g.init();
        g
    }
}

impl<const NUM_CHANNELS: usize> RampGenerator<NUM_CHANNELS> {
    pub fn new() -> Self {
        Self::default()
    }

    #[inline]
    pub fn phase(&self, index: usize) -> f32 {
        self.phase[index]
    }

    #[inline]
    pub fn frequency(&self, index: usize) -> f32 {
        self.frequency[index]
    }

    pub fn init(&mut self) {
        self.master_phase = 0.0;
        self.phase = [0.0; NUM_CHANNELS];
        self.frequency = [0.0; NUM_CHANNELS];
        self.wrap_counter = [0; NUM_CHANNELS];

        let r = Ratio { ratio: 1.0, q: 1 };
        self.ratio = [r; NUM_CHANNELS];
        self.next_ratio = [r; NUM_CHANNELS];
    }

    #[inline]
    pub fn set_next_ratio(&mut self, next_ratio: &[Ratio]) {
        self.next_ratio[..NUM_CHANNELS].copy_from_slice(&next_ratio[..NUM_CHANNELS]);
    }

    /// `RampGenerator::Step<ramp_mode, output_mode, range, use_ramp>`.
    /// `pw` must have length `NUM_CHANNELS` when `output_mode ==
    /// OutputMode::Frequency` (or `SlopePhase` under `RampMode::Ar`), and
    /// length 1 otherwise -- exactly as the C's `pw`/`per_channel_pw` split at
    /// the call site.
    #[allow(clippy::too_many_arguments)]
    pub fn step(
        &mut self,
        ramp_mode: RampMode,
        output_mode: OutputMode,
        range: Range,
        f0: f32,
        pw: &[f32],
        gate_flags: GateFlags,
        ramp: Option<f32>,
    ) {
        let n = if output_mode == OutputMode::Frequency
            || (output_mode == OutputMode::SlopePhase && ramp_mode == RampMode::Ar)
        {
            NUM_CHANNELS
        } else {
            1
        };

        if ramp_mode == RampMode::Ad {
            if gate_flags.contains(GateFlags::RISING) {
                self.phase[..n].fill(0.0);
            }

            for i in 0..n {
                self.frequency[i] = (f0 * self.next_ratio[i].ratio).min(0.25);
                if let Some(r) = ramp {
                    self.phase[i] = r * self.next_ratio[i].ratio;
                } else {
                    self.phase[i] += self.frequency[i];
                }
                self.phase[i] = self.phase[i].min(1.0);
            }
        }

        if ramp_mode == RampMode::Ar {
            if output_mode == OutputMode::SlopePhase {
                self.frequency[..n].fill(f0);
            } else {
                for i in 0..n {
                    self.frequency[i] = (f0 * self.next_ratio[i].ratio).min(0.25);
                }
            }

            let should_ramp_up = if let Some(r) = ramp {
                r < 0.5
            } else {
                gate_flags.contains(GateFlags::HIGH)
            };

            let clip_at = if should_ramp_up { 0.5 } else { 1.0 };
            for i in 0..n {
                if self.phase[i] < 0.5 && !should_ramp_up {
                    self.phase[i] = 0.5;
                } else if self.phase[i] > 0.5 && should_ramp_up {
                    self.phase[i] = 0.0;
                }
                let this_pw = if output_mode == OutputMode::Frequency { pw[0] } else { pw[i] };
                let slope = if self.phase[i] < 0.5 {
                    0.5 / (1.0e-6 + this_pw)
                } else {
                    0.5 / (1.0 + 1.0e-6 - this_pw)
                };
                self.phase[i] += self.frequency[i] * slope;
                self.phase[i] = self.phase[i].min(clip_at);
            }
        }

        if ramp_mode == RampMode::Looping {
            if range == Range::Audio && output_mode == OutputMode::Frequency {
                // Do not attempt to lock the phase of all outputs. This allows
                // smooth frequency changes.
                let mut reset = false;
                if gate_flags.contains(GateFlags::RISING) {
                    self.phase[..n].fill(0.0);
                    reset = true;
                }
                for i in 0..n {
                    self.frequency[i] = (f0 * self.next_ratio[i].ratio).min(0.25);
                }
                if !reset {
                    for i in 0..n {
                        self.phase[i] += self.frequency[i];
                        if self.phase[i] >= 1.0 {
                            self.phase[i] -= 1.0;
                        }
                    }
                }
            } else if let Some(r) = ramp {
                for i in 0..n {
                    self.frequency[i] = (f0 * self.ratio[i].ratio).min(0.25);
                }
                if r < self.master_phase {
                    for i in 0..n {
                        self.wrap_counter[i] += 1;
                        if self.wrap_counter[i] >= self.ratio[i].q {
                            self.ratio[i] = self.next_ratio[i];
                            self.wrap_counter[i] = 0;
                        }
                    }
                }
                self.master_phase = r;

                for i in 0..n {
                    let mult_phase = (self.master_phase + self.wrap_counter[i] as f32) * self.ratio[i].ratio;
                    self.phase[i] = mult_phase - (mult_phase as i32) as f32;
                }
            } else {
                let mut reset = false;
                if gate_flags.contains(GateFlags::RISING) {
                    self.master_phase = 0.0;
                    self.ratio[..n].copy_from_slice(&self.next_ratio[..n]);
                    self.wrap_counter[..n].fill(0);
                    reset = true;
                }
                for i in 0..n {
                    self.frequency[i] = (f0 * self.ratio[i].ratio).min(0.25);
                }
                if !reset {
                    self.master_phase += f0;
                }
                if self.master_phase >= 1.0 {
                    self.master_phase -= 1.0;
                    for i in 0..n {
                        self.wrap_counter[i] += 1;
                        if self.wrap_counter[i] >= self.ratio[i].q {
                            self.ratio[i] = self.next_ratio[i];
                            self.wrap_counter[i] = 0;
                        }
                    }
                }

                for i in 0..n {
                    let mult_phase = (self.master_phase + self.wrap_counter[i] as f32) * self.ratio[i].ratio;
                    self.phase[i] = mult_phase - (mult_phase as i32) as f32;
                }
            }
        }
    }
}
