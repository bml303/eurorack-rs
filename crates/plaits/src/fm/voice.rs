//! `plaits/dsp/fm/voice.h` -- one DX7 voice: `NUM_OPERATORS` [`Operator`]s
//! wired together by a [`Patch`]'s algorithm, each with its own envelope and
//! frequency ratio, plus the shared pitch envelope and feedback state.
//!
//! Unlike the C's `Voice<num_operators>`, which stores a `const
//! Algorithms<num_operators>*` set once via [`Voice::init`], this `Voice`
//! takes the (single, shared) compiled [`Algorithms`] table as a parameter to
//! [`Voice::render`] instead -- it's only ever needed there (to look up each
//! operator group's [`RenderCall`](super::algorithms::RenderCall) and, for
//! the brightness modulation, [`Algorithms::is_modulator`]), so there's
//! nothing to gain from every voice owning (or, worse, cloning) a copy of it.
//! The owner ([`super::super::engines::six_op_engine::SixOpEngine`], which
//! has `NUM_SIX_OP_VOICES` of these) computes the table once in its own
//! `init` and passes a reference into each voice's `render` call.

use super::algorithms::Algorithms;
use super::dx_units::{
    amp_mod_sensitivity, frequency_ratio, keyboard_scaling, normalize_velocity, operator_level,
    pow_2_fast, rate_scaling,
};
use super::envelope::{OperatorEnvelope, PitchEnvelope};
use super::operator::{Buffer, Operator};
use super::patch::Patch;
use crate::utils::units::semitones_to_ratio_safe;

#[derive(Debug, Default, Clone, Copy)]
pub struct VoiceParameters {
    /// Freezes the envelopes and evaluates them at [`VoiceParameters::
    /// envelope_control`]'s position along a fixed-length gate instead of
    /// live-gating them -- Plaits' "envelope scrubbing".
    pub sustain: bool,
    pub gate: bool,
    pub note: f32,
    pub velocity: f32,
    pub brightness: f32,
    pub envelope_control: f32,
    pub pitch_mod: f32,
    pub amp_mod: f32,
}

impl VoiceParameters {
    pub fn new() -> Self {
        Self::default()
    }
}

#[derive(Debug)]
pub struct Voice<const NUM_OPERATORS: usize> {
    sample_rate: f32,
    one_hz: f32,
    a0: f32,

    gate: bool,

    operator: [Operator; NUM_OPERATORS],
    operator_envelope: [OperatorEnvelope; NUM_OPERATORS],
    pitch_envelope: PitchEnvelope,
    feedback_state: [f32; 2],

    normalized_velocity: f32,
    note: f32,

    /// Per-operator frequency ratio, or (encoded as its *sign*) an absolute
    /// 1Hz-based frequency for a fixed-frequency operator -- see
    /// [`dx_units::frequency_ratio`](super::dx_units::frequency_ratio).
    ratio: [f32; NUM_OPERATORS],
    level_headroom: [f32; NUM_OPERATORS],
    level: [f32; NUM_OPERATORS],

    patch: Option<Patch>,
    /// Sticky "needs [`Voice::setup`]" bit, set by [`Voice::set_patch`] and
    /// cleared once `setup` has recomputed the envelope/ratio caches below.
    dirty: bool,
}

impl<const NUM_OPERATORS: usize> Voice<NUM_OPERATORS> {
    pub fn new() -> Self {
        Self {
            sample_rate: 0.0,
            one_hz: 0.0,
            a0: 0.0,
            gate: false,
            operator: [Operator::default(); NUM_OPERATORS],
            operator_envelope: core::array::from_fn(|_| OperatorEnvelope::new()),
            pitch_envelope: PitchEnvelope::new(),
            feedback_state: [0.0; 2],
            normalized_velocity: 10.0,
            note: 48.0,
            ratio: [0.0; NUM_OPERATORS],
            level_headroom: [0.0; NUM_OPERATORS],
            level: [0.0; NUM_OPERATORS],
            patch: None,
            dirty: true,
        }
    }

    pub fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.one_hz = 1.0 / sample_rate;
        self.a0 = 55.0 / sample_rate;

        // The envelope/LFO rate tables below were tuned against the DX7's
        // 44.1kHz sample rate; scale so they land on the same real-time rate
        // at whatever rate this port actually runs at.
        let envelope_scale = 44_100.0 * self.one_hz;
        for operator in &mut self.operator {
            operator.reset();
        }
        for envelope in &mut self.operator_envelope {
            envelope.init(envelope_scale);
        }
        self.pitch_envelope.init(envelope_scale);

        self.feedback_state = [0.0; 2];
        self.patch = None;
        self.gate = false;
        self.note = 48.0;
        self.normalized_velocity = 10.0;
        self.dirty = true;
    }

    pub fn set_patch(&mut self, patch: Option<Patch>) {
        self.patch = patch;
        self.dirty = true;
    }

    pub fn patch(&self) -> Option<&Patch> {
        self.patch.as_ref()
    }

    pub fn op_level(&self, i: usize) -> f32 {
        self.level[i]
    }

    /// Recomputes the envelope constants and frequency ratios [`Voice::
    /// render`] needs from the current patch, if [`Voice::set_patch`] has
    /// been called since the last time this ran. Returns whether it *did*
    /// anything -- see the CPU-overrun note at its `render` call site.
    fn setup(&mut self) -> bool {
        if !self.dirty {
            return false;
        }
        let Some(patch) = self.patch.as_ref() else {
            return false;
        };

        self.pitch_envelope
            .set(&patch.pitch_envelope.rate, &patch.pitch_envelope.level);

        for i in 0..NUM_OPERATORS {
            let op = &patch.op[i];
            let level = operator_level(op.level);
            self.operator_envelope[i].set(&op.envelope.rate, &op.envelope.level, level);

            // The level increase from keyboard scaling plus velocity scaling
            // must not push the operator above level 99 (TL 0) -- clamp the
            // headroom left for it here, applied in `render` below.
            self.level_headroom[i] = (127 - level) as f32;

            // A fixed-frequency operator's ratio is a 1Hz-relative multiplier
            // rather than a note-relative one; encode that as the ratio's
            // sign (see `f[i]`'s computation in `render`) so both cases share
            // one array instead of a parallel `is_fixed_frequency[]`.
            let sign = if op.mode == 0 { 1.0 } else { -1.0 };
            self.ratio[i] = sign * frequency_ratio(op);
        }

        self.dirty = false;
        true
    }

    /// Renders one block, phase-modulating operators per `algorithm`
    /// (looked up once per contiguous chain via `algorithms`, shared across
    /// every voice using the same operator count -- see the module doc).
    /// `buffers[0]` determines the block size; every buffer must be at
    /// least that long. See [`super::operator::Buffer`] for why a buffer can
    /// legitimately be aliased between two slots.
    pub fn render<const NUM_ALGORITHMS: usize>(
        &mut self,
        algorithms: &Algorithms<NUM_OPERATORS, NUM_ALGORITHMS>,
        parameters: &VoiceParameters,
        buffers: &[Buffer; 4],
    ) {
        if self.setup() {
            // A patch just became dirty; skip rendering this block rather
            // than pay for both a setup and a full render in the time this
            // block's real-time deadline allows. A clean 0.5ms blank when
            // switching patches, rather than a glitchy overrun.
            return;
        }
        let Some(patch) = self.patch.as_ref() else {
            return;
        };

        let size = buffers[0].borrow().len();
        let envelope_rate = size as f32;
        let ad_scale = pow_2_fast::<1>((0.5 - parameters.envelope_control) * 8.0);
        let r_scale = pow_2_fast::<1>(-f32::abs(parameters.envelope_control - 0.3) * 8.0);
        let gate_duration = 1.5 * self.sample_rate;
        let envelope_sample = gate_duration * parameters.envelope_control;

        // Apply the LFO and pitch-envelope modulations.
        let pitch_envelope = if parameters.sustain {
            self.pitch_envelope.render_at_sample(envelope_sample, gate_duration)
        } else {
            self.pitch_envelope.render(parameters.gate, envelope_rate, ad_scale, r_scale)
        };
        let pitch_mod = pitch_envelope + parameters.pitch_mod;
        let f0 = self.a0 * 0.25 * semitones_to_ratio_safe(parameters.note - 9.0 + pitch_mod * 12.0);

        // Sample the note and velocity (which affect scaling, not just pitch)
        // only on a fresh trigger, or continuously in free-running (sustain)
        // mode.
        let note_on = parameters.gate && !self.gate;
        self.gate = parameters.gate;
        if note_on || parameters.sustain {
            self.normalized_velocity = normalize_velocity(parameters.velocity);
            self.note = parameters.note;
        }

        if note_on && patch.reset_phase {
            for operator in &mut self.operator {
                operator.phase = 0;
            }
        }

        let mut f = [0.0f32; NUM_OPERATORS];
        let mut a = [0.0f32; NUM_OPERATORS];
        for i in 0..NUM_OPERATORS {
            let op = &patch.op[i];

            f[i] = self.ratio[i] * if self.ratio[i] < 0.0 { -self.one_hz } else { f0 };

            let rate_scale = rate_scaling(self.note, op.rate_scaling);
            let mut level = if parameters.sustain {
                self.operator_envelope[i].render_at_sample(envelope_sample, gate_duration)
            } else {
                self.operator_envelope[i].render(
                    parameters.gate,
                    envelope_rate * rate_scale,
                    ad_scale,
                    r_scale,
                )
            };
            let kb_scaling = keyboard_scaling(self.note, &op.keyboard_scaling);
            let velocity_scaling = self.normalized_velocity * op.velocity_sensitivity as f32;
            let brightness = if algorithms.is_modulator(patch.algorithm, i) {
                (parameters.brightness - 0.5) * 32.0
            } else {
                0.0
            };
            level += 0.125 * (kb_scaling + velocity_scaling + brightness).min(self.level_headroom[i]);
            self.level[i] = level;

            let sensitivity = amp_mod_sensitivity(op.amp_mod_sensitivity);
            let log_level_mod = sensitivity * parameters.amp_mod - 1.0;
            let level_mod = 1.0 - pow_2_fast::<2>(6.4 * log_level_mod);
            a[i] = pow_2_fast::<2>(-14.0 + level * level_mod);
        }

        let mut i = 0;
        while i < NUM_OPERATORS {
            let call = algorithms.render_call(patch.algorithm, i);
            let Some(render_fn) = call.render_fn else {
                // Every operator position gets a `RenderCall` at `compile`
                // time; a missing one would mean the opcode tables or the
                // compiler have a bug, not a legitimate "nothing to render"
                // case -- see the `debug_assert` in `Algorithms::compile`.
                debug_assert!(false, "operator {i} of algorithm {} has no render call", patch.algorithm);
                break;
            };
            render_fn(
                &mut self.operator[i..i + call.n],
                &f[i..],
                &a[i..],
                &mut self.feedback_state,
                patch.feedback,
                &buffers[call.input_index],
                &buffers[call.output_index],
            );
            i += call.n;
        }
    }
}

impl<const NUM_OPERATORS: usize> Default for Voice<NUM_OPERATORS> {
    fn default() -> Self {
        Self::new()
    }
}
