//! `yarns/part.{cc,h}` -- `Part`: per-channel note allocation (mono/poly, 9
//! voice-allocation modes), the arpeggiator, and the step sequencer.
//!
//! Deviations from the C++ (beyond "modernise structure only"):
//! - `Part` doesn't hold `Voice*` pointers into `Multi`'s voice array (a
//!   classic C aliasing pattern with no safe Rust equivalent). Instead it
//!   stores which indices of a *caller-supplied* voice array are its own
//!   (`voice_indices`, set by [`Part::allocate_voices`]), and every method
//!   that needs to touch a voice takes `voices: &mut [Voice]` (the *whole*
//!   array Multi owns) as an explicit parameter.
//! - `just_intonation_processor` and `custom_pitch_table_` are single
//!   objects owned by `Multi` and shared by every part in the C++ (the
//!   latter literally a pointer every part is given at startup, into
//!   `Multi::settings_.custom_pitch_table`). Ported the same way as
//!   `mi-marbles`' shared `RandomStream`: bundled into a `TuningContext`
//!   passed as an explicit parameter to every method that (possibly, for
//!   Just Intonation / Custom tuning) needs them, rather than a stored
//!   alias.
//! - `midi_handler`'s echo calls (`OnInternalNoteOn`/`OnInternalNoteOff`) go
//!   through `&mut dyn MidiOut` instead -- see `midi_out.rs`.
//! - The byte-addressed `Set(address, value)`/`Get(address)` API (and the
//!   `PartSetting`/`PART_MIDI_LAST` enum computing offsets via
//!   `sizeof(MidiSettings)`) doesn't have a sound Rust equivalent without
//!   `unsafe` pointer-cast reinterpretation of a `#[repr(C)]` struct as a
//!   byte array, and its only caller in the C++ is the out-of-scope
//!   `settings.cc`/`ui.cc` menu system. Replaced with
//!   [`Part::set_midi_settings`]/[`Part::set_voicing_settings`]/
//!   [`Part::set_sequencer_settings`], which reproduce the specific
//!   side-effecting field transitions `Set` handled (shutting all notes off
//!   on a MIDI-filter change, `touch_voice_allocation`/`touch_voices` on a
//!   voicing change, recomputing `arp_direction` on an arp-direction
//!   change) at whole-struct-replace granularity instead of per-byte.

extern crate alloc;

use alloc::vec::Vec;

use stmlib::note_stack::{NoteEntry, NoteStack, NoteStackPriority};
use stmlib::random::Random;
use stmlib::voice_allocator::{VoiceAllocator, VoiceStealingMode};

use crate::just_intonation_processor::JustIntonationProcessor;
use crate::midi_out::MidiOut;
use crate::resources::{LOOKUP_TABLE_SIGNED_TABLE, LUT_ARPEGGIATOR_PATTERNS, LUT_EUCLIDEAN};
use crate::voice::Voice;

pub const NUM_STEPS: usize = 64;
pub const MAX_NUM_VOICES: usize = 4;
pub const VOICE_ALLOCATION_NOT_FOUND: u8 = 0xff;

const CC_MODULATION_WHEEL_MSB: u8 = 0x01;
const CC_BREATH_CONTROLLER: u8 = 0x02;
const CC_FOOT_PEDAL_MSB: u8 = 0x04;
const CC_HOLD_PEDAL: u8 = 0x40;
const CC_OMNI_MODE_OFF: u8 = 0x7c;
const CC_OMNI_MODE_ON: u8 = 0x7d;
const CC_MONO_MODE_ON: u8 = 0x7e;
const CC_POLY_MODE_ON: u8 = 0x7f;

const CLOCK_DIVISIONS: [u8; 12] = [96, 48, 32, 24, 16, 12, 8, 6, 4, 3, 2, 1];

// Index into `LOOKUP_TABLE_SIGNED_TABLE` where the 12-tone scale tables
// start (`LUT_SCALE_PYTHAGOREAN` is entry 0 of that table -- see
// `resources.rs`; the raga tables that follow it aren't individually named
// here since `tune` only ever needs the base offset).
const LUT_SCALE_PYTHAGOREAN: usize = 0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ArpeggiatorDirection {
    #[default]
    Up,
    Down,
    UpDown,
    Random,
    AsPlayed,
    Chord,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VoiceAllocationMode {
    #[default]
    Mono,
    Poly,
    PolyCyclic,
    PolyRandom,
    PolyVelocity,
    PolySorted,
    PolyUnison1,
    PolyUnison2,
    PolyStealMostRecent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MidiOutMode {
    #[default]
    Off,
    Thru,
    GeneratedEvents,
}

/// `TuningSystem` -- kept as plain byte constants rather than a Rust enum,
/// since `tune` compares/offsets it arithmetically (`tuning_system >
/// TUNING_SYSTEM_JUST_INTONATION`, `tuning_system - TUNING_SYSTEM_PYTHAGOREAN`
/// as a table index) exactly like `oscillator::audio_mode` and
/// `voice::trigger_shape` do.
pub mod tuning_system {
    pub const EQUAL: u8 = 0;
    pub const JUST_INTONATION: u8 = 1;
    pub const PYTHAGOREAN: u8 = 2;
    pub const QUARTER_EB: u8 = 3;
    pub const QUARTER_E: u8 = 4;
    pub const QUARTER_EA: u8 = 5;
    pub const RAGA_1: u8 = 6;
    pub const RAGA_27: u8 = RAGA_1 + 26;
    pub const CUSTOM: u8 = RAGA_27 + 1;
}

/// The tuning state `Multi` owns and shares across every `Part` -- the
/// just-intonation note-history tuner, and the 12-entry custom scale table.
/// See the module doc comment.
pub struct TuningContext<'a> {
    pub jip: &'a mut JustIntonationProcessor,
    pub custom_pitch_table: &'a [i8; 12],
}

#[derive(Debug, Clone, Copy)]
#[derive(Default)]
pub struct MidiSettings {
    pub channel: u8,
    pub min_note: u8,
    pub max_note: u8,
    pub min_velocity: u8,
    pub max_velocity: u8,
    pub out_mode: MidiOutMode,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct VoicingSettings {
    pub allocation_mode: VoiceAllocationMode,
    pub allocation_priority: u8,
    pub portamento: u8,
    pub legato_mode: bool,
    pub pitch_bend_range: u8,
    pub vibrato_range: u8,
    pub modulation_rate: u8,
    pub tuning_transpose: i8,
    pub tuning_fine: i8,
    pub tuning_root: i8,
    pub tuning_system: u8,
    pub trigger_duration: u8,
    pub trigger_scale: bool,
    pub trigger_shape: u8,
    pub aux_cv: usize,
    pub audio_mode: u8,
    pub aux_cv_2: usize,
    pub tuning_factor: u8,
}

pub const SEQUENCER_STEP_REST: u8 = 0x80;
pub const SEQUENCER_STEP_TIE: u8 = 0x81;

#[derive(Debug, Clone, Copy)]
pub struct SequencerStep {
    // BYTE 0: 0x00 to 0x7f: note; 0x80: rest; 0x81: tie.
    // BYTE 1: 7 bits of velocity + 1 bit for the slide flag.
    pub data: [u8; 2],
}

impl Default for SequencerStep {
    fn default() -> Self {
        Self::new(SEQUENCER_STEP_REST, 0)
    }
}

impl SequencerStep {
    pub const fn new(data_0: u8, data_1: u8) -> Self {
        Self { data: [data_0, data_1] }
    }

    pub fn has_note(&self) -> bool {
        self.data[0] & 0x80 == 0
    }
    pub fn is_rest(&self) -> bool {
        self.data[0] == SEQUENCER_STEP_REST
    }
    pub fn is_tie(&self) -> bool {
        self.data[0] == SEQUENCER_STEP_TIE
    }
    pub fn note(&self) -> u8 {
        self.data[0] & 0x7f
    }
    pub fn is_slid(&self) -> bool {
        self.data[1] & 0x80 != 0
    }
    pub fn velocity(&self) -> u8 {
        self.data[1] & 0x7f
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SequencerSettings {
    pub clock_division: u8,
    pub gate_length: u8,
    pub arp_range: u8,
    pub arp_direction: ArpeggiatorDirection,
    pub arp_pattern: u8,
    pub euclidean_length: u8,
    pub euclidean_fill: u8,
    pub euclidean_rotate: u8,
    pub num_steps: u8,
    pub step: [SequencerStep; NUM_STEPS],
}

impl Default for SequencerSettings {
    fn default() -> Self {
        Self {
            clock_division: 0,
            gate_length: 0,
            arp_range: 0,
            arp_direction: ArpeggiatorDirection::default(),
            arp_pattern: 0,
            euclidean_length: 0,
            euclidean_fill: 0,
            euclidean_rotate: 0,
            num_steps: 0,
            step: [SequencerStep::default(); NUM_STEPS],
        }
    }
}

impl SequencerSettings {
    pub fn first_note(&self) -> i16 {
        for i in 0..self.num_steps as usize {
            if self.step[i].has_note() {
                return self.step[i].note() as i16;
            }
        }
        60
    }
}

#[inline]
fn note_stack_priority(byte: u8) -> NoteStackPriority {
    match byte {
        1 => NoteStackPriority::Low,
        2 => NoteStackPriority::High,
        3 => NoteStackPriority::First,
        _ => NoteStackPriority::Last,
    }
}

/// `ratio_table[]` from `part.cc`'s `Tune`.
fn tuning_ratio(tuning_factor: u8) -> (i32, i32) {
    const RATIO_TABLE: [(i32, i32); 14] = [
        (1, 1),
        (0, 1),
        (1, 8),
        (1, 4),
        (3, 8),
        (1, 2),
        (5, 8),
        (3, 4),
        (7, 8),
        (1, 1),
        (5, 4),
        (3, 2),
        (2, 1),
        (51095, 65536),
    ];
    RATIO_TABLE[tuning_factor as usize]
}

#[derive(Clone)]
pub struct Part {
    midi: MidiSettings,
    voicing: VoicingSettings,
    seq: SequencerSettings,

    voice_indices: [usize; MAX_NUM_VOICES],
    num_voices: usize,
    polychained: bool,

    ignore_note_off_messages: bool,
    release_latched_keys_on_next_note_on: bool,

    pressed_keys: NoteStack<13>,
    generated_notes: NoteStack<13>, // by sequencer or arpeggiator.
    mono_allocator: NoteStack<13>,
    poly_allocator: VoiceAllocator<{ MAX_NUM_VOICES * 2 }>,
    active_note: [u8; MAX_NUM_VOICES],
    cyclic_allocation_note_counter: u8,

    arp_seq_prescaler: u8,

    arp_step: u8,
    arp_note: i8,
    arp_octave: i8,
    arp_direction: i8,

    seq_running: bool,
    seq_recording: bool,
    seq_overdubbing: bool,
    seq_step: u8,
    seq_rec_step: u8,

    gate_length_counter: u16,
    lfo_counter: u16,

    has_siblings: bool,
    transposable: bool,
}

impl Default for Part {
    fn default() -> Self {
        Self {
            midi: MidiSettings::default(),
            voicing: VoicingSettings::default(),
            seq: SequencerSettings::default(),
            voice_indices: [0; MAX_NUM_VOICES],
            num_voices: 0,
            polychained: false,
            ignore_note_off_messages: false,
            release_latched_keys_on_next_note_on: false,
            pressed_keys: NoteStack::default(),
            generated_notes: NoteStack::default(),
            mono_allocator: NoteStack::default(),
            poly_allocator: VoiceAllocator::default(),
            active_note: [VOICE_ALLOCATION_NOT_FOUND; MAX_NUM_VOICES],
            cyclic_allocation_note_counter: 0,
            arp_seq_prescaler: 0,
            arp_step: 0,
            arp_note: 0,
            arp_octave: 0,
            arp_direction: 1,
            seq_running: false,
            seq_recording: false,
            seq_overdubbing: false,
            seq_step: 0,
            seq_rec_step: 0,
            gate_length_counter: 0,
            lfo_counter: 0,
            has_siblings: false,
            transposable: true,
        }
    }
}

impl Part {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn init(&mut self) {
        self.pressed_keys.init();
        self.mono_allocator.init();
        self.poly_allocator.init();
        self.generated_notes.init();
        self.active_note = [VOICE_ALLOCATION_NOT_FOUND; MAX_NUM_VOICES];
        self.num_voices = 0;
        self.polychained = false;
        self.ignore_note_off_messages = false;
        self.seq_recording = false;
        self.seq_running = false;
        self.release_latched_keys_on_next_note_on = false;
        self.transposable = true;
    }

    /// `AllocateVoices(Voice* voice, num_voices, polychain)`. `base_index`
    /// is the index into the caller's voice array that the C's `voice`
    /// pointer would have pointed at (see the module doc comment).
    pub fn allocate_voices(&mut self, voices: &mut [Voice], base_index: usize, num_voices: usize, polychain: bool) {
        self.all_notes_off(voices);

        self.num_voices = num_voices.min(MAX_NUM_VOICES);
        self.polychained = polychain;
        for i in 0..self.num_voices {
            self.voice_indices[i] = base_index + i;
        }
        self.poly_allocator.clear();
        self.poly_allocator
            .set_size((self.num_voices * if polychain { 2 } else { 1 }) as u8);
        self.touch_voices(voices);
    }

    #[inline]
    fn voice<'a>(&self, voices: &'a mut [Voice], i: usize) -> &'a mut Voice {
        &mut voices[self.voice_indices[i]]
    }

    pub fn tx_channel(&self) -> u8 {
        if self.midi.channel == 0x10 { 0 } else { self.midi.channel }
    }

    pub fn direct_thru(&self) -> bool {
        self.midi.out_mode == MidiOutMode::Thru && !self.polychained
    }

    pub fn find_voice_for_note(&self, note: u8) -> u8 {
        for i in 0..self.num_voices {
            if self.active_note[i] == note {
                return i as u8;
            }
        }
        VOICE_ALLOCATION_NOT_FOUND
    }

    pub fn accepts_channel(&self, channel: u8) -> bool {
        if !(self.transposable || self.seq_recording) {
            return false;
        }
        self.midi.channel == 0x10 || self.midi.channel == channel
    }

    pub fn accepts_note(&self, channel: u8, note: u8) -> bool {
        if !self.accepts_channel(channel) {
            return false;
        }
        if self.midi.min_note <= self.midi.max_note {
            note >= self.midi.min_note && note <= self.midi.max_note
        } else {
            note <= self.midi.max_note || note >= self.midi.min_note
        }
    }

    pub fn accepts(&self, channel: u8, note: u8, velocity: u8) -> bool {
        self.accepts_note(channel, note)
            && velocity >= self.midi.min_velocity
            && velocity <= self.midi.max_velocity
    }

    pub fn has_notes(&self) -> bool {
        self.pressed_keys.size() != 0
    }

    pub fn recording(&self) -> bool {
        self.seq_recording
    }
    pub fn overdubbing(&self) -> bool {
        self.seq_overdubbing
    }
    pub fn recording_step(&self) -> u8 {
        self.seq_rec_step
    }
    pub fn num_steps(&self) -> u8 {
        self.seq.num_steps
    }
    pub fn set_recording_step(&mut self, n: u8) {
        self.seq_rec_step = n;
    }

    pub fn midi_settings(&self) -> &MidiSettings {
        &self.midi
    }
    pub fn voicing_settings(&self) -> &VoicingSettings {
        &self.voicing
    }
    pub fn sequencer_settings(&self) -> &SequencerSettings {
        &self.seq
    }
    pub fn mutable_sequencer_settings(&mut self) -> &mut SequencerSettings {
        &mut self.seq
    }

    /// Replaces the MIDI filter settings. The C++'s `Set(address, value)`
    /// shuts all notes off whenever any of `channel`/`min_note`/`max_note`/
    /// `min_velocity`/`max_velocity` changes, to avoid stuck notes; this
    /// does the same at whole-struct granularity.
    pub fn set_midi_settings(&mut self, voices: &mut [Voice], midi: MidiSettings) {
        self.midi = midi;
        self.all_notes_off(voices);
    }

    /// Replaces the voicing settings, reproducing the C++'s
    /// `TouchVoiceAllocation` (on an `allocation_mode` change) vs
    /// `TouchVoices` (on any other voicing change) distinction.
    pub fn set_voicing_settings(&mut self, voices: &mut [Voice], voicing: VoicingSettings) {
        let allocation_mode_changed = voicing.allocation_mode != self.voicing.allocation_mode;
        self.voicing = voicing;
        if allocation_mode_changed {
            self.touch_voice_allocation(voices);
        } else {
            self.touch_voices(voices);
        }
    }

    /// Replaces the sequencer settings, recomputing the derived
    /// `arp_direction` sign the C++'s `Set` updates on
    /// `PART_SEQUENCER_ARP_DIRECTION`.
    pub fn set_sequencer_settings(&mut self, seq: SequencerSettings) {
        self.seq = seq;
        self.arp_direction = if self.seq.arp_direction == ArpeggiatorDirection::Down {
            -1
        } else {
            1
        };
    }

    pub fn touch(&mut self, voices: &mut [Voice]) {
        self.touch_voices(voices);
        self.touch_voice_allocation(voices);
    }

    pub fn latch(&mut self) {
        self.ignore_note_off_messages = true;
        self.release_latched_keys_on_next_note_on = true;
    }

    pub fn unlatch(&mut self) {
        self.ignore_note_off_messages = false;
        self.release_latched_keys_on_next_note_on = true;
    }

    pub fn set_siblings(&mut self, has_siblings: bool) {
        self.has_siblings = has_siblings;
    }

    pub fn set_transposable(&mut self, transposable: bool) {
        self.transposable = transposable;
    }

    /// The return value indicates whether the message can be forwarded to
    /// the MIDI out (soft-thru). For example, when the arpeggiator is on,
    /// note on/off can return `false` to make sure that the chord that
    /// triggers the arpeggiator does not find its way to the MIDI out.
    /// Instead it will be sent note by note within `internal_note_on`/
    /// `internal_note_off`.
    ///
    /// Also note that channel/keyrange/velocity range filtering is not
    /// applied here: it's up to the caller to call `accepts()` first.
    #[allow(clippy::too_many_arguments)]
    pub fn note_on(
        &mut self,
        voices: &mut [Voice],
        tuning: &mut TuningContext,
        midi_out: &mut dyn MidiOut,
        channel: u8,
        note: u8,
        velocity: u8,
    ) -> bool {
        let sent_from_step_editor = channel & 0x80 != 0;

        if self.release_latched_keys_on_next_note_on {
            let still_latched = self.ignore_note_off_messages;

            // Releasing all latched keys will generate "fake" NoteOff
            // messages. We should not ignore them.
            self.ignore_note_off_messages = false;
            self.release_latched_notes(voices, tuning, midi_out);

            self.release_latched_keys_on_next_note_on = still_latched;
            self.ignore_note_off_messages = still_latched;
        }
        self.pressed_keys.note_on(note, velocity);

        if (!(self.seq.num_steps != 0 && self.seq_running) && self.seq.arp_range == 0)
            || sent_from_step_editor
        {
            self.internal_note_on(voices, tuning, midi_out, note, velocity);
        }

        if self.seq_recording && !sent_from_step_editor {
            self.record_step(SequencerStep::new(note, velocity));
        }
        self.midi.out_mode == MidiOutMode::Thru && !self.polychained
    }

    pub fn note_off(
        &mut self,
        voices: &mut [Voice],
        tuning: &mut TuningContext,
        midi_out: &mut dyn MidiOut,
        channel: u8,
        note: u8,
    ) -> bool {
        let sent_from_step_editor = channel & 0x80 != 0;

        if self.ignore_note_off_messages {
            for i in 1..=self.pressed_keys.max_size() {
                // Flag the note so that it is removed once the sustain
                // pedal is released.
                let e = self.pressed_keys.mutable_note(i);
                if e.note == note && e.velocity != 0 {
                    e.velocity |= 0x80;
                }
            }
        } else {
            self.pressed_keys.note_off(note);

            if (!(self.seq.num_steps != 0 && self.seq_running) && self.seq.arp_range == 0)
                || sent_from_step_editor
            {
                self.internal_note_off(voices, tuning, midi_out, note);
            }
        }
        self.midi.out_mode == MidiOutMode::Thru && !self.polychained
    }

    pub fn control_change(&mut self, voices: &mut [Voice], channel: u8, controller: u8, value: u8) -> bool {
        match controller {
            CC_MODULATION_WHEEL_MSB | CC_BREATH_CONTROLLER | CC_FOOT_PEDAL_MSB => {
                for i in 0..self.num_voices {
                    self.voice(voices, i).control_change(controller, value);
                }
            }
            CC_OMNI_MODE_OFF => {
                self.midi.channel = channel;
            }
            CC_OMNI_MODE_ON => {
                self.midi.channel = 0x10;
            }
            CC_MONO_MODE_ON => {
                self.voicing.allocation_mode = VoiceAllocationMode::Mono;
                self.touch_voice_allocation(voices);
            }
            CC_POLY_MODE_ON => {
                self.voicing.allocation_mode = VoiceAllocationMode::Poly;
                self.touch_voice_allocation(voices);
            }
            CC_HOLD_PEDAL => {
                if value >= 64 {
                    self.ignore_note_off_messages = true;
                } else {
                    self.ignore_note_off_messages = false;
                    // The C++ also calls `ReleaseLatchedNotes` here; that
                    // needs a `TuningContext`/`MidiOut` this method doesn't
                    // have (it can recurse into `NoteOff`, which can retune
                    // and forward polychained notes). The host must call
                    // `release_latched_notes_on_hold_release` right after
                    // this returns, for the same (controller, value).
                }
            }
            0x70 => self.record_step_tie(),
            0x71 => self.record_step_rest(),
            0x78 | 0x7b => self.all_notes_off(voices),
            0x79 => self.reset_all_controllers(voices),
            _ => {}
        }
        self.midi.out_mode != MidiOutMode::Off
    }

    /// See the comment on the `CC_HOLD_PEDAL` arm of [`Part::control_change`]:
    /// call this right after a `control_change` call for controller `0x40`
    /// (hold pedal) with `value < 64`.
    pub fn release_latched_notes_on_hold_release(
        &mut self,
        voices: &mut [Voice],
        tuning: &mut TuningContext,
        midi_out: &mut dyn MidiOut,
    ) {
        self.release_latched_notes(voices, tuning, midi_out);
    }

    pub fn pitch_bend(&mut self, voices: &mut [Voice], _channel: u8, pitch_bend: u16) -> bool {
        for i in 0..self.num_voices {
            self.voice(voices, i).pitch_bend(pitch_bend);
        }

        if self.seq_recording && !(8192 - 2048..=8192 + 2048).contains(&pitch_bend) {
            self.seq.step[self.seq_rec_step as usize].data[1] |= 0x80;
        }

        self.midi.out_mode != MidiOutMode::Off
    }

    pub fn aftertouch_note(
        &mut self,
        voices: &mut [Voice],
        channel: u8,
        note: u8,
        velocity: u8,
    ) -> bool {
        if self.voicing.allocation_mode != VoiceAllocationMode::Mono {
            let voice_index = if self.voicing.allocation_mode == VoiceAllocationMode::Poly
                || self.voicing.allocation_mode == VoiceAllocationMode::PolyStealMostRecent
            {
                self.poly_allocator.find(note)
            } else {
                self.find_voice_for_note(note)
            };
            if (voice_index as u32) < self.poly_allocator.size() as u32 {
                self.voice(voices, voice_index as usize).aftertouch(velocity);
            }
        } else {
            self.aftertouch(voices, channel, velocity);
        }
        self.midi.out_mode != MidiOutMode::Off
    }

    pub fn aftertouch(&mut self, voices: &mut [Voice], _channel: u8, velocity: u8) -> bool {
        for i in 0..self.num_voices {
            self.voice(voices, i).aftertouch(velocity);
        }
        self.midi.out_mode != MidiOutMode::Off
    }

    pub fn reset(&mut self, voices: &mut [Voice], tuning: &mut TuningContext, midi_out: &mut dyn MidiOut) {
        self.stop(voices, tuning, midi_out);
        for i in 0..self.num_voices {
            self.voice(voices, i).note_off();
            self.voice(voices, i).reset_all_controllers();
        }
    }

    pub fn clock(&mut self, voices: &mut [Voice], tuning: &mut TuningContext, midi_out: &mut dyn MidiOut) {
        if self.arp_seq_prescaler == 0 {
            if self.seq.num_steps != 0 && self.seq_running {
                self.clock_sequencer(voices, tuning, midi_out);
            } else if self.seq.arp_range != 0 {
                self.clock_arpeggiator(voices, tuning, midi_out);
            }
        }

        if self.gate_length_counter == 0 && self.generated_notes.size() != 0 {
            self.stop_sequencer_arpeggiator_notes(voices, tuning, midi_out);
        } else {
            self.gate_length_counter = self.gate_length_counter.wrapping_sub(1);
        }

        self.arp_seq_prescaler += 1;
        if self.arp_seq_prescaler >= CLOCK_DIVISIONS[self.seq.clock_division as usize] {
            self.arp_seq_prescaler = 0;
        }

        if self.voicing.modulation_rate >= 100 {
            let num_ticks = CLOCK_DIVISIONS[(self.voicing.modulation_rate - 100) as usize] as u32;
            let expected_phase = (self.lfo_counter as u32 % num_ticks) * 65536 / num_ticks;
            for i in 0..self.num_voices {
                self.voice(voices, i).tap_lfo(expected_phase << 16);
            }
        }
        self.lfo_counter = self.lfo_counter.wrapping_add(1);
    }

    pub fn start(&mut self, started_by_keyboard: bool) {
        self.arp_seq_prescaler = 0;

        self.seq_step = 0;
        self.seq_running = !started_by_keyboard;

        self.release_latched_keys_on_next_note_on = false;
        self.ignore_note_off_messages = false;

        if self.seq.arp_direction == ArpeggiatorDirection::Down {
            self.arp_note = self.pressed_keys.size() as i8 - 1;
            self.arp_octave = self.seq.arp_range as i8 - 1;
            self.arp_direction = -1;
        } else {
            self.arp_note = 0;
            self.arp_octave = 0;
            self.arp_direction = 1;
        }
        self.arp_step = 0;

        self.lfo_counter = 0;

        self.generated_notes.clear();
    }

    pub fn stop(&mut self, voices: &mut [Voice], tuning: &mut TuningContext, midi_out: &mut dyn MidiOut) {
        self.seq_running = false;
        self.stop_sequencer_arpeggiator_notes(voices, tuning, midi_out);
        self.all_notes_off(voices);
    }

    pub fn stop_recording(&mut self) {
        self.seq_recording = false;
    }

    pub fn start_recording(&mut self) {
        if self.seq_recording {
            return;
        }
        self.seq_recording = true;
        self.seq_rec_step = 0;
        self.seq_overdubbing = self.seq.num_steps != 0 && self.seq_running;
        if !self.seq_overdubbing {
            self.seq.step = [SequencerStep::new(SEQUENCER_STEP_REST, 0); NUM_STEPS];
            self.seq.num_steps = 0;
        }
    }

    pub fn record_step(&mut self, step: SequencerStep) {
        if self.seq_recording {
            self.seq.step[self.seq_rec_step as usize].data[0] = step.data[0];
            self.seq.step[self.seq_rec_step as usize].data[1] |= step.data[1];
            self.seq_rec_step += 1;
            let last_step = if self.seq_overdubbing { self.seq.num_steps } else { NUM_STEPS as u8 };
            // Extend sequence.
            if !self.seq_overdubbing && self.seq_rec_step > self.seq.num_steps {
                self.seq.num_steps = self.seq_rec_step;
            }
            // Wrap to first step.
            if self.seq_rec_step >= last_step {
                self.seq_rec_step = 0;
            }
        }
    }

    pub fn record_step_tie(&mut self) {
        self.record_step(SequencerStep::new(SEQUENCER_STEP_TIE, 0));
    }

    pub fn record_step_rest(&mut self) {
        self.record_step(SequencerStep::new(SEQUENCER_STEP_REST, 0));
    }

    pub fn modify_note_at_current_step(&mut self, note: u8) {
        if self.seq_recording {
            self.seq.step[self.seq_rec_step as usize].data[0] = note;
        }
    }

    fn stop_sequencer_arpeggiator_notes(
        &mut self,
        voices: &mut [Voice],
        tuning: &mut TuningContext,
        midi_out: &mut dyn MidiOut,
    ) {
        while self.generated_notes.size() != 0 {
            let note = self.generated_notes.sorted_note(0).note;
            self.generated_notes.note_off(note);
            self.internal_note_off(voices, tuning, midi_out, note);
        }
    }

    fn clock_sequencer(&mut self, voices: &mut [Voice], tuning: &mut TuningContext, midi_out: &mut dyn MidiOut) {
        let step = self.seq.step[self.seq_step as usize];

        if step.has_note() {
            let mut note = step.note() as i16;
            if self.pressed_keys.size() != 0 && self.transposable {
                // When we play a monophonic sequence, we can make the guess
                // that root note = first note. But this is not the case
                // when we are playing several sequences at the same time.
                // In this case, we use root note = 60.
                let root_note: i16 = if !self.has_siblings { self.seq.first_note() } else { 60 };
                note += self.pressed_keys.most_recent_note().note as i16 - root_note;
                while note > 127 {
                    note -= 12;
                }
                while note < 0 {
                    note += 12;
                }
            }
            let note = note as u8;
            if !step.is_slid() {
                self.stop_sequencer_arpeggiator_notes(voices, tuning, midi_out);
                self.internal_note_on(voices, tuning, midi_out, note, step.velocity());
            } else {
                self.internal_note_on(voices, tuning, midi_out, note, step.velocity());
                self.stop_sequencer_arpeggiator_notes(voices, tuning, midi_out);
            }
            self.generated_notes.note_on(note, step.velocity());
            self.gate_length_counter = self.seq.gate_length as u16;
        }
        self.seq_step += 1;
        if self.seq_step >= self.seq.num_steps {
            self.seq_step = 0;
        }
        let next_step = self.seq.step[self.seq_step as usize];
        if next_step.is_tie() || next_step.is_slid() {
            // The next step contains a "sustain" message, or a slid note.
            // Extend the duration of the current note.
            self.gate_length_counter = self
                .gate_length_counter
                .wrapping_add(CLOCK_DIVISIONS[self.seq.clock_division as usize] as u16);
        }
    }

    fn clock_arpeggiator(&mut self, voices: &mut [Voice], tuning: &mut TuningContext, midi_out: &mut dyn MidiOut) {
        let mut pattern_mask: u32 = 1 << self.arp_step;

        let mut pattern = LUT_ARPEGGIATOR_PATTERNS[self.seq.arp_pattern as usize] as u32;
        let mut pattern_length: u32 = 16;

        if self.seq.euclidean_length != 0 {
            pattern_length = self.seq.euclidean_length as u32;
            pattern_mask = 1 << ((self.arp_step as u32 + self.seq.euclidean_rotate as u32) % pattern_length);
            // Read euclidean pattern from ROM.
            let offset = (self.seq.euclidean_length as usize - 1) * 32;
            pattern = LUT_EUCLIDEAN[offset + self.seq.euclidean_fill as usize];
        }

        let num_notes = self.pressed_keys.size();
        if (pattern_mask & pattern) != 0 && num_notes != 0 {
            // Update arpeggiator note/octave counter.
            if num_notes == 1 && self.seq.arp_range == 1 {
                // This is a corner case for the Up/down pattern code. Get it
                // out of the way.
                self.arp_note = 0;
                self.arp_octave = 0;
            } else if self.seq.arp_direction == ArpeggiatorDirection::Random {
                let random = Random::get_sample() as u16;
                self.arp_octave = ((random & 0xff) % self.seq.arp_range as u16) as i8;
                self.arp_note = ((random >> 8) % num_notes as u16) as i8;
            } else {
                let mut wrapped = true;
                while wrapped {
                    if self.arp_note >= num_notes as i8 || self.arp_note < 0 {
                        self.arp_octave += self.arp_direction;
                        self.arp_note = if self.arp_direction > 0 { 0 } else { num_notes as i8 - 1 };
                    }
                    wrapped = false;
                    if self.arp_octave >= self.seq.arp_range as i8 || self.arp_octave < 0 {
                        self.arp_octave = if self.arp_direction > 0 { 0 } else { self.seq.arp_range as i8 - 1 };
                        if self.seq.arp_direction == ArpeggiatorDirection::UpDown {
                            self.arp_direction = -self.arp_direction;
                            self.arp_note = if self.arp_direction > 0 { 1 } else { num_notes as i8 - 2 };
                            self.arp_octave = if self.arp_direction > 0 { 0 } else { self.seq.arp_range as i8 - 1 };
                            wrapped = true;
                        }
                    }
                }
            }

            // Kill pending notes (if any).
            self.stop_sequencer_arpeggiator_notes(voices, tuning, midi_out);

            // Trigger arpeggiated note or chord.
            if self.seq.arp_direction != ArpeggiatorDirection::Chord {
                let arpeggio_note: NoteEntry = if self.seq.arp_direction == ArpeggiatorDirection::AsPlayed {
                    *self.pressed_keys.played_note(self.arp_note as u8)
                } else {
                    *self.pressed_keys.sorted_note(self.arp_note as u8)
                };
                let mut note = arpeggio_note.note as i16;
                let velocity = arpeggio_note.velocity & 0x7f;
                note += 12 * self.arp_octave as i16;
                while note > 127 {
                    note -= 12;
                }
                let note = note as u8;
                self.generated_notes.note_on(note, velocity);
                self.internal_note_on(voices, tuning, midi_out, note, velocity);
            } else {
                for i in 0..num_notes {
                    let chord_note = *self.pressed_keys.played_note(i);
                    let velocity = chord_note.velocity & 0x7f;
                    self.generated_notes.note_on(chord_note.note, velocity);
                    self.internal_note_on(voices, tuning, midi_out, chord_note.note, velocity);
                }
            }
            self.arp_note += self.arp_direction;
            self.gate_length_counter = self.seq.gate_length as u16;
        }

        self.arp_step += 1;
        if self.arp_step as u32 >= pattern_length {
            self.arp_step = 0;
        }
    }

    fn reset_all_controllers(&mut self, voices: &mut [Voice]) {
        self.ignore_note_off_messages = false;
        for i in 0..self.num_voices {
            self.voice(voices, i).reset_all_controllers();
        }
    }

    pub fn all_notes_off(&mut self, voices: &mut [Voice]) {
        self.poly_allocator.clear_notes();
        self.mono_allocator.clear();
        self.pressed_keys.clear();
        for i in 0..self.num_voices {
            self.voice(voices, i).note_off();
        }
        self.active_note = [VOICE_ALLOCATION_NOT_FOUND; MAX_NUM_VOICES];
        self.release_latched_keys_on_next_note_on = false;
        self.ignore_note_off_messages = false;
    }

    fn release_latched_notes(&mut self, voices: &mut [Voice], tuning: &mut TuningContext, midi_out: &mut dyn MidiOut) {
        let tx_channel = self.tx_channel();
        let max = self.pressed_keys.max_size();
        let mut latched_notes: Vec<u8> = Vec::new();
        for i in 1..=max {
            let e = self.pressed_keys.note(i);
            if e.velocity & 0x80 != 0 {
                latched_notes.push(e.note);
            }
        }
        for note in latched_notes {
            self.note_off(voices, tuning, midi_out, tx_channel, note);
        }
    }

    fn dispatch_sorted_notes(&mut self, voices: &mut [Voice], tuning: &mut TuningContext, unison: bool) {
        let n = self.mono_allocator.size();
        for i in 0..self.num_voices {
            let index: u8 = if unison && (n as usize) < self.num_voices {
                if n != 0 { (i as u16 * n as u16 / self.num_voices as u16) as u8 } else { 0xff }
            } else if i < self.mono_allocator.size() as usize {
                i as u8
            } else {
                0xff
            };
            if index != 0xff {
                let note_entry = *self
                    .mono_allocator
                    .note_by_priority(note_stack_priority(self.voicing.allocation_priority), index);
                let tuned = self.tune(tuning, note_entry.note as i16);
                let gate_on = self.voice(voices, i).gate_on();
                self.voice(voices, i)
                    .note_on(tuned, note_entry.velocity, self.voicing.portamento, !gate_on);
                self.active_note[i] = note_entry.note;
            } else {
                self.voice(voices, i).note_off();
                self.active_note[i] = VOICE_ALLOCATION_NOT_FOUND;
            }
        }
    }

    fn internal_note_on(
        &mut self,
        voices: &mut [Voice],
        tuning: &mut TuningContext,
        midi_out: &mut dyn MidiOut,
        note: u8,
        velocity: u8,
    ) {
        if self.midi.out_mode == MidiOutMode::GeneratedEvents && !self.polychained {
            midi_out.internal_note_on(self.tx_channel(), note, velocity);
        }

        if self.voicing.allocation_mode == VoiceAllocationMode::Mono {
            let priority = note_stack_priority(self.voicing.allocation_priority);
            let before = *self.mono_allocator.note_by_priority(priority, 0);
            self.mono_allocator.note_on(note, velocity);
            let after = *self.mono_allocator.note_by_priority(priority, 0);
            // Check if the note that has been played should be triggered
            // according to selected voice priority rules.
            if before.note != after.note {
                let legato = self.mono_allocator.size() > 1;
                let tuned = self.tune(tuning, after.note as i16);
                for i in 0..self.num_voices {
                    let portamento = if self.voicing.legato_mode && !legato { 0 } else { self.voicing.portamento };
                    let trigger = !self.voicing.legato_mode || !legato;
                    self.voice(voices, i).note_on(tuned, after.velocity, portamento, trigger);
                }
            }
        } else if self.voicing.allocation_mode == VoiceAllocationMode::PolySorted
            || self.voicing.allocation_mode == VoiceAllocationMode::PolyUnison1
            || self.voicing.allocation_mode == VoiceAllocationMode::PolyUnison2
        {
            self.mono_allocator.note_on(note, velocity);
            self.dispatch_sorted_notes(voices, tuning, self.voicing.allocation_mode != VoiceAllocationMode::PolySorted);
        } else {
            let mut voice_index: u8 = 0;
            match self.voicing.allocation_mode {
                VoiceAllocationMode::Poly => {
                    voice_index = self.poly_allocator.note_on_with_mode(note, VoiceStealingMode::Lru);
                }
                VoiceAllocationMode::PolyStealMostRecent => {
                    voice_index = self.poly_allocator.note_on_with_mode(note, VoiceStealingMode::Mru);
                }
                VoiceAllocationMode::PolyCyclic => {
                    if self.cyclic_allocation_note_counter as usize >= self.num_voices {
                        self.cyclic_allocation_note_counter = 0;
                    }
                    voice_index = self.cyclic_allocation_note_counter;
                    self.cyclic_allocation_note_counter += 1;
                }
                VoiceAllocationMode::PolyRandom => {
                    voice_index = ((Random::get_word() >> 24) as usize % self.num_voices) as u8;
                }
                VoiceAllocationMode::PolyVelocity => {
                    voice_index = ((velocity as u16 * self.num_voices as u16) >> 7) as u8;
                }
                _ => {}
            }

            if (voice_index as usize) < self.num_voices {
                // Prevent the same note from being simultaneously played on
                // two channels.
                self.kill_all_instances_of_note(voices, note);
                let tuned = self.tune(tuning, note as i16);
                self.voice(voices, voice_index as usize)
                    .note_on(tuned, velocity, self.voicing.portamento, true);
                self.active_note[voice_index as usize] = note;
            } else {
                // Polychaining forwarding.
                midi_out.internal_note_on(self.tx_channel(), note, velocity);
            }
        }
    }

    fn kill_all_instances_of_note(&mut self, voices: &mut [Voice], note: u8) {
        loop {
            let index = self.find_voice_for_note(note);
            if index != VOICE_ALLOCATION_NOT_FOUND {
                self.voice(voices, index as usize).note_off();
                self.active_note[index as usize] = VOICE_ALLOCATION_NOT_FOUND;
            } else {
                break;
            }
        }
    }

    fn internal_note_off(
        &mut self,
        voices: &mut [Voice],
        tuning: &mut TuningContext,
        midi_out: &mut dyn MidiOut,
        note: u8,
    ) {
        if self.midi.out_mode == MidiOutMode::GeneratedEvents && !self.polychained {
            midi_out.internal_note_off(self.tx_channel(), note);
        }

        if self.voicing.tuning_system == tuning_system::JUST_INTONATION {
            tuning.jip.note_off(note);
        }

        if self.voicing.allocation_mode == VoiceAllocationMode::Mono {
            let priority = note_stack_priority(self.voicing.allocation_priority);
            let before = *self.mono_allocator.note_by_priority(priority, 0);
            self.mono_allocator.note_off(note);
            let after = *self.mono_allocator.note_by_priority(priority, 0);
            if self.mono_allocator.size() == 0 {
                // No key is pressed, we just close the gate.
                for i in 0..self.num_voices {
                    self.voice(voices, i).note_off();
                }
            } else if before.note != after.note {
                // Removing the note gives priority to another note that is
                // still being pressed. Slide to this note (or retrigger if
                // legato mode is off).
                let tuned = self.tune(tuning, after.note as i16);
                let trigger = !self.voicing.legato_mode;
                for i in 0..self.num_voices {
                    self.voice(voices, i).note_on(tuned, after.velocity, self.voicing.portamento, trigger);
                }
            }
        } else if self.voicing.allocation_mode == VoiceAllocationMode::PolySorted
            || self.voicing.allocation_mode == VoiceAllocationMode::PolyUnison1
            || self.voicing.allocation_mode == VoiceAllocationMode::PolyUnison2
        {
            self.mono_allocator.note_off(note);
            self.kill_all_instances_of_note(voices, note);
            if self.voicing.allocation_mode == VoiceAllocationMode::PolyUnison1 {
                self.dispatch_sorted_notes(voices, tuning, true);
            }
        } else {
            let voice_index = if self.voicing.allocation_mode == VoiceAllocationMode::Poly
                || self.voicing.allocation_mode == VoiceAllocationMode::PolyStealMostRecent
            {
                self.poly_allocator.note_off(note)
            } else {
                self.find_voice_for_note(note)
            };
            if (voice_index as usize) < self.num_voices {
                self.voice(voices, voice_index as usize).note_off();
                self.active_note[voice_index as usize] = VOICE_ALLOCATION_NOT_FOUND;
            } else {
                midi_out.internal_note_off(self.tx_channel(), note);
            }
        }
    }

    fn touch_voice_allocation(&mut self, voices: &mut [Voice]) {
        self.all_notes_off(voices);
        self.reset_all_controllers(voices);
    }

    fn touch_voices(&mut self, voices: &mut [Voice]) {
        self.voicing.aux_cv = self.voicing.aux_cv.min(7);
        self.voicing.aux_cv_2 = self.voicing.aux_cv_2.min(7);
        for i in 0..self.num_voices {
            let v = self.voice(voices, i);
            v.set_pitch_bend_range(self.voicing.pitch_bend_range);
            v.set_modulation_rate(self.voicing.modulation_rate);
            v.set_vibrato_range(self.voicing.vibrato_range);
            v.set_trigger_duration(self.voicing.trigger_duration);
            v.set_trigger_scale(self.voicing.trigger_scale);
            v.set_trigger_shape(self.voicing.trigger_shape);
            v.set_aux_cv(self.voicing.aux_cv);
            v.set_aux_cv_2(self.voicing.aux_cv_2);
            v.set_audio_mode(self.voicing.audio_mode);
            v.set_tuning(self.voicing.tuning_transpose, self.voicing.tuning_fine);
        }
    }

    /// `Tune(int16_t midi_note)`. In Just Intonation mode this has a side
    /// effect on the shared tuner (it records the note into its history),
    /// which is why every caller threads a `&mut TuningContext` through.
    fn tune(&self, tuning: &mut TuningContext, midi_note: i16) -> i16 {
        let note = midi_note;
        let mut pitch: i32 = (note as i32) << 7;
        let mut pitch_class = ((note + 240) % 12) as usize;

        if self.voicing.tuning_system == tuning_system::JUST_INTONATION {
            pitch = tuning.jip.note_on(note as u8) as i32;
        } else if self.voicing.tuning_system == tuning_system::CUSTOM {
            pitch += tuning.custom_pitch_table[pitch_class] as i32;
        } else if self.voicing.tuning_system > tuning_system::JUST_INTONATION {
            let note2 = note - self.voicing.tuning_root as i16;
            pitch_class = ((note2 + 240) % 12) as usize;
            let table_index =
                LUT_SCALE_PYTHAGOREAN + (self.voicing.tuning_system - tuning_system::PYTHAGOREAN) as usize;
            pitch += LOOKUP_TABLE_SIGNED_TABLE[table_index][pitch_class] as i32;
        }

        let root: i32 = ((self.voicing.tuning_root as i32) + 60) << 7;
        let mut scaled_pitch = pitch;
        scaled_pitch -= root;
        let (p, q) = tuning_ratio(self.voicing.tuning_factor);
        scaled_pitch = scaled_pitch * p / q;
        scaled_pitch += root;
        scaled_pitch.clamp(0, 16383) as i16
    }
}
