//! `plaits/dsp/fm/envelope.h` -- a 4-stage (attack/decay/sustain/release)
//! envelope generator, plus the two DX7-specific quirks layered on top of it
//! for operator envelopes (a "vaguely logarithmic" shape and a level-jump
//! threshold for ascending segments) and pitch envelopes (linear segments).
//!
//! [`Envelope::render_at_sample`] additionally lets the envelope be evaluated
//! at an arbitrary point in time rather than stepped sample-by-sample -- used
//! by Plaits' "envelope scrubbing" feature (`sustain` mode), which freezes
//! the *shape* of a triggered envelope and scrubs a playhead across it with a
//! control voltage instead of gating it live.

use super::dx_units::{
    operator_envelope_increment, operator_level, pitch_envelope_increment, pitch_envelope_level,
};

/// Sentinel for [`Envelope::value`]'s `start_level`: "use whatever level the
/// previous stage ended at" (the DX7's stages don't all start from 0).
const PREVIOUS_LEVEL: f32 = -100.0;

/// The last stage index is always the release stage.
const RELEASE: usize = 3;

#[derive(Debug, Clone)]
pub struct Envelope {
    stage: usize,
    phase: f32,
    start: f32,

    increment: [f32; 4],
    level: [f32; 4],
    scale: f32,

    /// DX7 operator envelopes reshape ascending segments (a jump above a
    /// threshold, a "vaguely logarithmic" curve); pitch envelopes don't.
    reshape_ascending_segments: bool,
}

impl Envelope {
    fn new(reshape_ascending_segments: bool) -> Self {
        let mut e = Self {
            stage: 0,
            phase: 0.0,
            start: 0.0,
            increment: [0.0; 4],
            level: [0.0; 4],
            scale: 1.0,
            reshape_ascending_segments,
        };
        e.init(1.0);
        e
    }

    pub fn init(&mut self, scale: f32) {
        self.scale = scale;
        self.stage = RELEASE;
        self.phase = 1.0;
        self.start = 0.0;
        for i in 0..4 {
            self.increment[i] = 0.001;
            self.level[i] = 1.0 / (1 << i) as f32;
        }
        self.level[RELEASE] = 0.0;
    }

    /// Evaluates the envelope `t` samples after it was triggered, without
    /// touching (or depending on) the live `stage`/`phase` a gated render
    /// would have reached by then -- as long as it's still within the
    /// `gate_duration`-long attack/decay/sustain run, or `t - gate_duration`
    /// samples into the release that starts right after.
    pub fn render_at_sample(&self, mut t: f32, gate_duration: f32) -> f32 {
        if t > gate_duration {
            let release_phase = (t - gate_duration) * self.increment[RELEASE];
            return if release_phase >= 1.0 {
                self.level[RELEASE]
            } else {
                self.value(
                    RELEASE,
                    release_phase,
                    self.render_at_sample(gate_duration, gate_duration),
                )
            };
        }

        let mut stage = 0;
        while stage < RELEASE {
            let stage_duration = 1.0 / self.increment[stage];
            if t < stage_duration {
                break;
            }
            t -= stage_duration;
            stage += 1;
        }

        if stage == RELEASE {
            t -= gate_duration;
            if t <= 0.0 {
                // TODO(pichenettes): this should always be true.
                return self.level[RELEASE - 1];
            } else if t * self.increment[RELEASE] > 1.0 {
                return self.level[RELEASE];
            }
        }
        self.value(stage, t * self.increment[stage], PREVIOUS_LEVEL)
    }

    /// Steps the envelope by one sample; `rate` scales the elapsed time
    /// (used for operator envelopes' per-note rate scaling), `ad_scale` and
    /// `release_scale` additionally scale the attack/decay and release
    /// stages respectively (Plaits' `envelope_control` "time-travel" knob).
    pub fn render(&mut self, gate: bool, rate: f32, ad_scale: f32, release_scale: f32) -> f32 {
        if gate {
            if self.stage == RELEASE {
                self.start = self.current_value();
                self.stage = 0;
                self.phase = 0.0;
            }
        } else if self.stage != RELEASE {
            self.start = self.current_value();
            self.stage = RELEASE;
            self.phase = 0.0;
        }

        self.phase += self.increment[self.stage]
            * rate
            * (if self.stage == RELEASE { release_scale } else { ad_scale });
        if self.phase >= 1.0 {
            if self.stage >= RELEASE - 1 {
                self.phase = 1.0;
            } else {
                self.phase = 0.0;
                self.stage += 1;
            }
            self.start = PREVIOUS_LEVEL;
        }

        self.current_value()
    }

    fn current_value(&self) -> f32 {
        self.value(self.stage, self.phase, self.start)
    }

    fn value(&self, stage: usize, mut phase: f32, start_level: f32) -> f32 {
        let mut from = if start_level == PREVIOUS_LEVEL {
            self.level[(stage + 4 - 1) % 4]
        } else {
            start_level
        };
        let mut to = self.level[stage];

        if self.reshape_ascending_segments && from < to {
            from = from.max(6.7);
            to = to.max(6.7);
            phase *= (2.5 - phase) * 0.666667;
        }

        phase * (to - from) + from
    }
}

/// A DX7 operator's amplitude envelope: rate/level pairs decoded from a
/// patch's 0-99 byte values, plus the ascending-segment reshaping quirk.
#[derive(Debug, Clone)]
pub struct OperatorEnvelope(Envelope);

impl OperatorEnvelope {
    pub fn new() -> Self {
        Self(Envelope::new(true))
    }

    pub fn init(&mut self, scale: f32) {
        self.0.init(scale);
    }

    pub fn render_at_sample(&self, t: f32, gate_duration: f32) -> f32 {
        self.0.render_at_sample(t, gate_duration)
    }

    pub fn render(&mut self, gate: bool, rate: f32, ad_scale: f32, release_scale: f32) -> f32 {
        self.0.render(gate, rate, ad_scale, release_scale)
    }

    pub fn set(&mut self, rate: &[u8; 4], level: &[u8; 4], global_level: u8) {
        for i in 0..4 {
            let level_scaled = operator_level(level[i]) as i32;
            let level_scaled = (level_scaled & !1) + global_level as i32 - 133; // 125 ?
            self.0.level[i] = 0.125 * if level_scaled < 1 { 0.5 } else { level_scaled as f32 };
        }

        for i in 0..4 {
            let mut increment = operator_envelope_increment(rate[i]);
            let mut from = self.0.level[(i + 4 - 1) % 4];
            let mut to = self.0.level[i];

            if from == to {
                // Quirk: for plateaux, the increment is scaled.
                increment *= 0.6;
                if i == 0 && level[i] == 0 {
                    // Quirk: the attack plateau is faster.
                    increment *= 20.0;
                }
            } else if from < to {
                from = from.max(6.7);
                to = to.max(6.7);
                if from == to {
                    // Quirk: because of the jump, the attack might disappear.
                    increment = 1.0;
                } else {
                    // Quirk: because of the weird shape, the rate is adjusted.
                    increment *= 7.2 / (to - from);
                }
            } else {
                increment *= 1.0 / (from - to);
            }
            self.0.increment[i] = increment * self.0.scale;
        }
    }
}

impl Default for OperatorEnvelope {
    fn default() -> Self {
        Self::new()
    }
}

/// A DX7 patch's shared pitch envelope (applied to every operator's pitch,
/// on top of its own frequency ratio).
#[derive(Debug, Clone)]
pub struct PitchEnvelope(Envelope);

impl PitchEnvelope {
    pub fn new() -> Self {
        Self(Envelope::new(false))
    }

    pub fn init(&mut self, scale: f32) {
        self.0.init(scale);
    }

    pub fn render_at_sample(&self, t: f32, gate_duration: f32) -> f32 {
        self.0.render_at_sample(t, gate_duration)
    }

    pub fn render(&mut self, gate: bool, rate: f32, ad_scale: f32, release_scale: f32) -> f32 {
        self.0.render(gate, rate, ad_scale, release_scale)
    }

    pub fn set(&mut self, rate: &[u8; 4], level: &[u8; 4]) {
        for i in 0..4 {
            self.0.level[i] = pitch_envelope_level(level[i]);
        }
        for i in 0..4 {
            let from = self.0.level[(i + 4 - 1) % 4];
            let to = self.0.level[i];
            let mut increment = pitch_envelope_increment(rate[i]);
            if from != to {
                increment *= 1.0 / f32::abs(from - to);
            } else if i != 3 {
                increment = 0.2;
            }
            self.0.increment[i] = increment * self.0.scale;
        }
    }
}

impl Default for PitchEnvelope {
    fn default() -> Self {
        Self::new()
    }
}
