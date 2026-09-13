//! `plaits/dsp/engine2/six_op_engine.{h,cc}` -- the 6-operator DX7-style FM
//! engine, built on [`crate::fm`]. Registered 3 times in the real firmware
//! at engine slots 2-4 against one shared instance, differentiated only by
//! which of the three bundled SysEx banks (`SYX_BANK_0/1/2`) is loaded --
//! see [`crate::voice::Voice::render_without_postprocessors`].
//!
//! Each of the two [`FmVoice`]s tracks its own patch/LFO/gate so a new note
//! can start on one while the other is still releasing the previous one; the
//! two are alternated round-robin on every gate rising edge (`active_voice`).
//!
//! # Deviations from the C
//!
//! - `SixOpEngine::Render` alternates which of the two voices gets a full
//!   render each call, rendering it for *twice* the block length into a
//!   carry-over accumulator so the other voice's contribution from the
//!   previous call can be mixed in in-between -- a CPU-budget trick for the
//!   embedded target, explicitly presented in the C's own comments as an
//!   alternative to simply rendering both voices in full every block (the
//!   commented-out "naive" version directly above it). **This port defaults
//!   to that naive form** (both voices render their full length every
//!   call) -- a back-of-envelope instruction-count estimate against the
//!   real compiled Cortex-M33 code suggests the naive form's extra CPU cost
//!   fits comfortably in a 48kHz block's budget on that target (roughly 27%
//!   of it, against an estimated ~56% for the *original* 72MHz Cortex-M4F
//!   this trick was written for), so the added complexity doesn't earn its
//!   keep there by default. The `six-op-staggered-rendering` Cargo feature
//!   restores the C's exact scheme (see [`SixOpEngine::render_voices`]) for
//!   targets where it does. Either way it isn't a correctness difference --
//!   the same [`FmVoice`] renders identically, just on a different
//!   schedule. (Naive-by-default cross-checked against the independent Rust
//!   port at <https://github.com/sourcebox/mi-plaits-dsp-rs> (MIT), which
//!   made the same choice.)
//! - The C's `Algorithms<6>` is a `const Algorithms<6>*` every `FMVoice`
//!   holds a pointer to, computed once and shared; since nothing here needs
//!   it outside of rendering, this port keeps a single owned copy on
//!   `SixOpEngine` and passes it into each voice's `render` call instead --
//!   see the `crate::fm::voice` module doc.

extern crate alloc;

use alloc::boxed::Box;
use alloc::vec;
use core::cell::RefCell;

use crate::engine::{trigger_state, Engine, EngineParameters, PostProcessingSettings};
use crate::fm::algorithms::Algorithms;
use crate::fm::lfo::Lfo;
use crate::fm::patch::{Patch, SYX_SIZE};
use crate::fm::voice::{Voice, VoiceParameters};
use crate::utils::hysteresis_quantizer::HysteresisQuantizer2;
use crate::utils::soft_clip;

const NUM_OPERATORS: usize = 6;
const NUM_ALGORITHMS: usize = 32;
const NUM_VOICES: usize = 2;
const NUM_PATCHES_PER_BANK: usize = 32;

/// How many blocks' worth of scratch space [`SixOpEngine::temp_buffer`] and
/// each [`FmVoice`]'s own scratch buffers need. Staggered rendering renders
/// one voice `NUM_VOICES` blocks ahead every `NUM_VOICES`th call (see
/// [`SixOpEngine::render_voices`]), so it needs `NUM_VOICES` blocks of
/// headroom; the naive default renders exactly one block at a time.
#[cfg(feature = "six-op-staggered-rendering")]
const RENDER_FACTOR: usize = NUM_VOICES;
#[cfg(not(feature = "six-op-staggered-rendering"))]
const RENDER_FACTOR: usize = 1;

#[derive(Debug, Default)]
pub struct SixOpEngine {
    algorithms: Algorithms<NUM_OPERATORS, NUM_ALGORITHMS>,
    patches: [Patch; NUM_PATCHES_PER_BANK],
    patch_index_quantizer: HysteresisQuantizer2,
    voice: [FmVoice; NUM_VOICES],

    temp_buffer: Box<[f32]>,
    active_voice: usize,

    /// Whose turn it is to get a full (`NUM_VOICES`-blocks-long) render this
    /// call -- see [`SixOpEngine::render_voices`]. Unrelated to
    /// `active_voice`, which tracks which voice the *next note-on* goes to.
    #[cfg(feature = "six-op-staggered-rendering")]
    rendered_voice: usize,
    /// The not-yet-output tail of whichever voice rendered last call,
    /// mixed into this call's first block of output.
    #[cfg(feature = "six-op-staggered-rendering")]
    acc_buffer: Box<[f32]>,
}

impl SixOpEngine {
    pub fn new(block_size: usize) -> Self {
        Self {
            algorithms: Algorithms::new(),
            patches: core::array::from_fn(|_| Patch::new()),
            patch_index_quantizer: HysteresisQuantizer2::new(),
            voice: core::array::from_fn(|_| FmVoice::new(block_size * RENDER_FACTOR)),
            temp_buffer: vec![0.0; block_size * RENDER_FACTOR].into_boxed_slice(),
            active_voice: 0,
            #[cfg(feature = "six-op-staggered-rendering")]
            rendered_voice: 0,
            #[cfg(feature = "six-op-staggered-rendering")]
            acc_buffer: vec![0.0; block_size].into_boxed_slice(),
        }
    }

    /// Unpacks a 4096-byte SysEx bulk dump (32 x 128-byte voice patches) and
    /// makes every voice pick it up again next time its patch is (re)loaded.
    pub fn load_syx_bank(&mut self, bank: &[u8; 4096]) {
        for (i, patch) in self.patches.iter_mut().enumerate() {
            patch.unpack(&bank[i * SYX_SIZE..]);
        }
        for voice in &mut self.voice {
            voice.unload_patch();
        }
    }
}

impl Engine for SixOpEngine {
    fn init(&mut self) {
        self.algorithms.init();
        self.patch_index_quantizer.init(32, 0.005, false);

        for voice in &mut self.voice {
            voice.init(crate::dsp::SAMPLE_RATE);
        }

        self.active_voice = NUM_VOICES - 1;
        #[cfg(feature = "six-op-staggered-rendering")]
        {
            self.rendered_voice = 0;
            self.acc_buffer.fill(0.0);
        }
    }

    fn reset(&mut self) {}

    fn load_user_data(&mut self, user_data: Option<&'static [u8]>) {
        let Some(bank) = user_data else { return };
        for (i, patch) in self.patches.iter_mut().enumerate() {
            patch.unpack(&bank[i * SYX_SIZE..]);
        }
    }

    fn render(
        &mut self,
        parameters: &EngineParameters,
        out: &mut [f32],
        aux: &mut [f32],
        already_enveloped: bool,
    ) -> bool {
        let patch_index = self.patch_index_quantizer.process(parameters.harmonics * 1.02) as usize;

        if parameters.trigger == trigger_state::UNPATCHED {
            // No envelope/gate patched in: free-run, "scrubbing" through the
            // envelopes with `morph` instead of gating them live. Voice 0
            // drives a shared LFO position for both voices, since neither is
            // really "the" active one in this mode.
            let t = parameters.morph;
            self.voice[0].lfo.scrub(2.0 * crate::dsp::SAMPLE_RATE * t);
            let pitch_mod = self.voice[0].lfo.pitch_mod();
            let amp_mod = self.voice[0].lfo.amp_mod();

            for (i, voice) in self.voice.iter_mut().enumerate() {
                voice.load_patch(self.patches[patch_index].clone());
                voice.parameters = VoiceParameters {
                    sustain: i == 0,
                    gate: false,
                    note: parameters.note,
                    velocity: parameters.accent,
                    brightness: parameters.timbre,
                    envelope_control: t,
                    pitch_mod,
                    amp_mod,
                };
            }
        } else {
            if parameters.trigger == trigger_state::RISING_EDGE {
                self.active_voice = (self.active_voice + 1) % NUM_VOICES;
                self.voice[self.active_voice].load_patch(self.patches[patch_index].clone());
                self.voice[self.active_voice].lfo.reset();
            }

            {
                let active = &mut self.voice[self.active_voice];
                active.parameters.note = parameters.note;
                active.parameters.velocity = parameters.accent;
                active.parameters.envelope_control = parameters.morph;
                active.lfo.step(out.len() as f32);
            }
            let active_patch = self.voice[self.active_voice].patch().cloned();
            let active_pitch_mod = self.voice[self.active_voice].lfo.pitch_mod();
            let active_amp_mod = self.voice[self.active_voice].lfo.amp_mod();

            for (i, voice) in self.voice.iter_mut().enumerate() {
                voice.parameters.brightness = parameters.timbre;
                voice.parameters.sustain = false;
                voice.parameters.gate =
                    parameters.trigger == trigger_state::HIGH && i == self.active_voice;

                if voice.patch() != active_patch.as_ref() {
                    // A voice whose patch differs from the active one keeps
                    // stepping its own LFO on its own modulation, rather than
                    // free-riding on the active voice's -- it's releasing a
                    // *different* patch, which may have entirely different
                    // LFO settings.
                    voice.lfo.step(out.len() as f32);
                    voice.parameters.pitch_mod = voice.lfo.pitch_mod();
                    voice.parameters.amp_mod = voice.lfo.amp_mod();
                } else {
                    voice.parameters.pitch_mod = active_pitch_mod;
                    voice.parameters.amp_mod = active_amp_mod;
                }
            }
        }

        self.render_voices(out, aux);

        already_enveloped
    }

    fn post_processing_settings(&self) -> PostProcessingSettings {
        PostProcessingSettings {
            out_gain: 1.0,
            aux_gain: 1.0,
            already_enveloped: true,
        }
    }
}

impl SixOpEngine {
    /// Renders both voices into `out`/`aux` (both start this call as
    /// whatever the caller last left them; every byte gets overwritten).
    /// Naive form: every voice renders exactly this call's block length,
    /// in full, every call. See the module doc for why this is the default.
    #[cfg(not(feature = "six-op-staggered-rendering"))]
    fn render_voices(&mut self, out: &mut [f32], aux: &mut [f32]) {
        out.fill(0.0);
        for voice in &mut self.voice {
            self.temp_buffer.fill(0.0);
            voice.render(&self.algorithms, &mut self.temp_buffer);
            for (out_sample, temp_sample) in out.iter_mut().zip(self.temp_buffer.iter()) {
                *out_sample = soft_clip(*out_sample + *temp_sample * 0.25);
            }
        }
        aux.copy_from_slice(out);
    }

    /// Renders both voices into `out`/`aux`, staggered: only
    /// `self.rendered_voice` gets a full render this call, for
    /// `NUM_VOICES * out.len()` samples (`out.len()` real ones, plus
    /// `out.len()` more "for next time"), alternating which voice that is
    /// every call. `FmVoice::render` mixes additively onto buffer 0 rather
    /// than overwriting it (every DX7 algorithm's final operator does), so
    /// priming that buffer with `acc_buffer` -- the *other* voice's
    /// leftover half from last call -- before rendering is what mixes the
    /// two voices together; the freshly-rendered second half then becomes
    /// the new `acc_buffer` for next call.
    ///
    /// A voice's envelopes/LFO still see one call's worth of *real* elapsed
    /// time every `NUM_VOICES` calls (`Voice::render`'s `envelope_rate` is
    /// `out.len()`, computed from the doubled buffer this renders into), so
    /// nothing runs at the wrong rate -- it's rendered `NUM_VOICES` blocks
    /// at a time rather than one, not sped up or slowed down. The trade-off
    /// is a `NUM_VOICES - 1`-block-deep pipeline: a voice's `parameters`
    /// (gate, note, ...) are current as of *this* call even when it isn't
    /// this voice's turn to render, so whichever half of its just-computed
    /// audio lands in a future call already reflects that -- but the
    /// audible result for a gate/note change lags by up to that many
    /// blocks on whichever voice isn't rendering this call. This matches
    /// the real firmware's behaviour exactly.
    #[cfg(feature = "six-op-staggered-rendering")]
    fn render_voices(&mut self, out: &mut [f32], aux: &mut [f32]) {
        let size = out.len();
        let doubled = size * NUM_VOICES;
        debug_assert!(
            self.temp_buffer.len() >= doubled,
            "temp_buffer was sized for a smaller block than this render call"
        );

        self.temp_buffer[..size].copy_from_slice(&self.acc_buffer[..size]);
        self.temp_buffer[size..doubled].fill(0.0);

        self.rendered_voice = (self.rendered_voice + 1) % NUM_VOICES;
        self.voice[self.rendered_voice].render(&self.algorithms, &mut self.temp_buffer[..doubled]);

        for (out_sample, temp_sample) in out.iter_mut().zip(self.temp_buffer[..size].iter()) {
            *out_sample = soft_clip(*temp_sample * 0.25);
        }
        aux.copy_from_slice(out);

        self.acc_buffer[..size].copy_from_slice(&self.temp_buffer[size..doubled]);
    }
}

/// One of [`SixOpEngine`]'s two independently-gated FM voices: the
/// polyphonic-adjacent state (current patch, LFO position, gate) that sits
/// around a bare [`Voice`].
#[derive(Debug, Default)]
struct FmVoice {
    lfo: Lfo,
    voice: Voice<NUM_OPERATORS>,
    parameters: VoiceParameters,

    temp_buffer_1: Box<[f32]>,
    temp_buffer_2: Box<[f32]>,
    temp_buffer_3: Box<[f32]>,
}

impl FmVoice {
    fn new(block_size: usize) -> Self {
        Self {
            lfo: Lfo::new(),
            voice: Voice::new(),
            parameters: VoiceParameters::new(),
            temp_buffer_1: vec![0.0; block_size].into_boxed_slice(),
            temp_buffer_2: vec![0.0; block_size].into_boxed_slice(),
            temp_buffer_3: vec![0.0; block_size].into_boxed_slice(),
        }
    }

    fn init(&mut self, sample_rate: f32) {
        self.voice.init(sample_rate);
        self.lfo.init(sample_rate);
        self.parameters = VoiceParameters {
            sustain: false,
            gate: false,
            note: 48.0,
            velocity: 0.5,
            brightness: 0.5,
            envelope_control: 0.5,
            pitch_mod: 0.0,
            amp_mod: 0.0,
        };
    }

    fn patch(&self) -> Option<&Patch> {
        self.voice.patch()
    }

    /// No-ops if `patch` is already this voice's current patch (by value --
    /// the C compares by *pointer*, since every voice reads patches out of
    /// the same shared bank array, so switching back to an already-loaded
    /// index is always a no-op there too; a value comparison only diverges
    /// from that if two distinct bank slots happen to hold byte-identical
    /// patches, in which case treating them as the same patch is harmless).
    /// Crucially, this -- not [`Voice::set_patch`] -- is the only place a
    /// patch change reaches `self.voice`: `set_patch` unconditionally marks
    /// the voice dirty (forcing an envelope-config recompute on the next
    /// render), so calling it every render rather than only on an actual
    /// change would defeat the whole point of that dirty flag.
    fn load_patch(&mut self, patch: Patch) {
        if self.patch() == Some(&patch) {
            return;
        }
        self.lfo.set(&patch.modulations);
        self.voice.set_patch(Some(patch));
    }

    fn unload_patch(&mut self) {
        self.voice.set_patch(None);
    }

    fn render(&mut self, algorithms: &Algorithms<NUM_OPERATORS, NUM_ALGORITHMS>, out: &mut [f32]) {
        if self.patch().is_none() {
            return;
        }

        let buffers = [
            RefCell::new(out),
            RefCell::new(&mut *self.temp_buffer_1),
            RefCell::new(&mut *self.temp_buffer_2),
            RefCell::new(&mut *self.temp_buffer_3),
        ];
        self.voice.render(algorithms, &self.parameters, &buffers);
    }
}
