//! `yarns/multi.{cc,h}` -- `Multi`: the top-level multi-part MIDI router.
//! Dispatches note/CC/pitch-bend/aftertouch events to the right `Part`s per
//! the current [`Layout`], drives the shared master clock (internal or
//! MIDI), and derives the 4 CV/gate outputs and the built-in demo song.
//!
//! Deviations from the C++ (beyond "modernise structure only") -- see also
//! `part.rs`'s module doc comment, since the same shared-state-as-parameter
//! and `MidiOut`-callback patterns apply here:
//! - `just_intonation_processor` and `settings.custom_pitch_table` (a
//!   single object/array owned by `Multi` and shared by every `Part` in the
//!   C++) are bundled into a `TuningContext` built once per call and passed
//!   to each `Part` method that needs it.
//! - `midi_handler`'s `OnClock`/`OnStart`/`OnStop` echoes go through
//!   `&mut dyn MidiOut`, threaded down from whichever `Multi` method
//!   triggers them, same as `Part`.
//! - `layout_configurator_` (the front-panel MIDI-channel-learn feature) is
//!   dropped entirely -- UI-adjacent, tied to physical button combos.
//!   `RegisterNote`/`StartLearning`/`StopLearning`/`learning()` and the
//!   LED-brightness/learning-mode branch of `GetLedsBrightness` go with it.
//! - `GetLedsBrightness` (front-panel LED brightness) is dropped -- pure UI
//!   output with no DSP relevance.
//! - `Serialize`/`Deserialize`/`SerializeCalibration`/
//!   `DeserializeCalibration` (flash persistence, generically templated
//!   over a `stream_buffer`) and the byte-addressed `Set(address, value)`/
//!   `Get(address)` settings API are dropped, matching `Part`'s equivalent
//!   cut -- see `part.rs`'s doc comment. Replaced with
//!   [`Multi::set_layout`] (the one address that had a side effect,
//!   `change_layout`) and [`Multi::set_tempo`]/[`Multi::set_swing`]
//!   (the two others, updating `internal_clock`).
//! - `HandleRemoteControlCC` and the remote-control branch of
//!   `ControlChange` are dropped: both exist purely to forward CC values
//!   into the out-of-scope global `Settings` menu object
//!   (`yarns::settings.SetFromCC`). `settings_.remote_control_channel` is
//!   kept as a field for structural/config compatibility but has no
//!   behaviour attached.
//! - `paques()` (an Easter egg checking 4 settings fields for exact
//!   hard-coded values) is dropped: no functional relevance, and tied to
//!   values a host wouldn't naturally reach without the settings menu.

use crate::internal_clock::InternalClock;
use crate::just_intonation_processor::JustIntonationProcessor;
use crate::midi_out::MidiOut;
use crate::part::{
    MidiOutMode, MidiSettings, Part, SequencerSettings, SequencerStep, TuningContext,
    VoiceAllocationMode, VoicingSettings, NUM_STEPS, SEQUENCER_STEP_REST,
};
use crate::song::SONG;
use crate::voice::Voice;

pub const NUM_PARTS: usize = 4;
pub const NUM_VOICES: usize = 4;
const MAX_BAR_DURATION: u8 = 32;

const CLOCK_DIVISIONS: [u16; 12] = [96, 48, 32, 24, 16, 12, 8, 6, 4, 3, 2, 1];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum Layout {
    #[default]
    Mono,
    DualMono,
    QuadMono,
    DualPoly,
    QuadPoly,
    DualPolychained,
    QuadPolychained,
    OctalPolychained,
    QuadTriggers,
    QuadVoltages,
    ThreeOne,
}

#[derive(Debug, Clone, Copy)]
#[derive(Default)]
pub struct MultiSettings {
    pub layout: Layout,
    pub clock_tempo: u8,
    pub clock_swing: u8,
    pub clock_input_division: u8,
    pub clock_output_division: u8,
    pub clock_bar_duration: u8,
    pub clock_override: bool,
    pub custom_pitch_table: [i8; 12],
    pub remote_control_channel: u8,
    pub nudge_first_tick: bool,
    pub clock_manual_start: bool,
}

pub struct Multi {
    settings: MultiSettings,

    running: bool,
    started_by_keyboard: bool,
    latched: bool,
    recording: bool,

    internal_clock: InternalClock,
    internal_clock_ticks: u8,
    midi_clock_tick_duration: u16,

    swing_predelay: [i16; 12],
    swing_counter: u8,

    clock_input_prescaler: u8,
    clock_output_prescaler: u8,
    bar_position: u16,
    stop_count_down: u8,

    clock_pulse_counter: u16,
    reset_pulse_counter: u16,

    previous_output_division: u16,
    needs_resync: bool,

    num_active_parts: u8,

    part: [Part; NUM_PARTS],
    voice: [Voice; NUM_VOICES],

    just_intonation_processor: JustIntonationProcessor,

    song_pointer: Option<usize>, // index into SONG.
    song_clock: u32,
    song_delta: u8,
}

impl Default for Multi {
    fn default() -> Self {
        Self {
            settings: MultiSettings::default(),
            running: false,
            started_by_keyboard: false,
            latched: false,
            recording: false,
            internal_clock: InternalClock::default(),
            internal_clock_ticks: 0,
            midi_clock_tick_duration: 0,
            swing_predelay: [0; 12],
            swing_counter: 0,
            clock_input_prescaler: 0,
            clock_output_prescaler: 0,
            bar_position: 0,
            stop_count_down: 0,
            clock_pulse_counter: 0,
            reset_pulse_counter: 0,
            previous_output_division: 0,
            needs_resync: false,
            num_active_parts: 0,
            part: core::array::from_fn(|_| Part::default()),
            voice: core::array::from_fn(|_| Voice::default()),
            just_intonation_processor: JustIntonationProcessor::default(),
            song_pointer: None,
            song_clock: 0,
            song_delta: 0,
        }
    }
}

impl Multi {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn init(&mut self, reset_calibration: bool) {
        self.just_intonation_processor.init();

        self.settings.custom_pitch_table = [0; 12];

        for part in self.part.iter_mut() {
            part.init();
        }
        for voice in self.voice.iter_mut() {
            voice.init(reset_calibration);
        }
        self.running = false;
        self.latched = false;
        self.recording = false;

        // Put the multi in a usable state. Even if these settings will
        // later be overridden with data retrieved from flash (presets, out
        // of scope for this port).
        self.settings.clock_tempo = 120;
        self.settings.clock_swing = 0;
        self.settings.clock_input_division = 1;
        self.settings.clock_output_division = 7;
        self.settings.clock_bar_duration = 4;
        self.settings.clock_override = false;
        self.settings.nudge_first_tick = false;
        self.settings.clock_manual_start = false;

        self.part[0].set_midi_settings(
            &mut self.voice,
            MidiSettings {
                channel: 0,
                min_note: 0,
                max_note: 127,
                min_velocity: 0,
                max_velocity: 127,
                out_mode: MidiOutMode::GeneratedEvents,
            },
        );

        self.part[0].set_voicing_settings(
            &mut self.voice,
            VoicingSettings {
                allocation_priority: 0, // NoteStackPriority::Last
                allocation_mode: VoiceAllocationMode::Mono,
                legato_mode: false,
                portamento: 0,
                pitch_bend_range: 2,
                vibrato_range: 1,
                modulation_rate: 50,
                trigger_duration: 2,
                aux_cv: 1,
                aux_cv_2: 6,
                tuning_transpose: 0,
                tuning_fine: 0,
                tuning_root: 0,
                tuning_system: 0,
                tuning_factor: 0,
                audio_mode: 0,
                ..VoicingSettings::default()
            },
        );

        let mut seq = SequencerSettings {
            clock_division: 7,
            gate_length: 3,
            arp_range: 0,
            arp_direction: crate::part::ArpeggiatorDirection::Up,
            arp_pattern: 0,
            ..SequencerSettings::default()
        };
        seq.step = [SequencerStep::new(SEQUENCER_STEP_REST, 0); NUM_STEPS];
        seq.num_steps = 0;
        self.part[0].set_sequencer_settings(seq);

        self.num_active_parts = 1;
        self.part[0].allocate_voices(&mut self.voice, 0, 1, false);
        self.settings.layout = Layout::Mono;
    }

    pub fn note_on(&mut self, midi_out: &mut dyn MidiOut, channel: u8, note: u8, velocity: u8) -> bool {
        let mut thru = true;
        let mut received = false;
        let mut tuning = TuningContext {
            jip: &mut self.just_intonation_processor,
            custom_pitch_table: &self.settings.custom_pitch_table,
        };
        for i in 0..self.num_active_parts as usize {
            if self.part[i].accepts(channel, note, velocity) {
                received = true;
                thru = self.part[i].note_on(&mut self.voice, &mut tuning, midi_out, channel, note, velocity) && thru;
            }
        }

        if received && !self.running() && self.internal_clock() && !self.settings.clock_manual_start {
            // Start the arpeggiators.
            self.start(true, midi_out);
        }

        self.stop_count_down = 0;

        thru
    }

    pub fn note_off(&mut self, midi_out: &mut dyn MidiOut, channel: u8, note: u8, _velocity: u8) -> bool {
        let mut thru = true;
        let mut has_notes = false;
        let mut tuning = TuningContext {
            jip: &mut self.just_intonation_processor,
            custom_pitch_table: &self.settings.custom_pitch_table,
        };
        for i in 0..self.num_active_parts as usize {
            if self.part[i].accepts_note(channel, note) {
                thru = self.part[i].note_off(&mut self.voice, &mut tuning, midi_out, channel, note) && thru;
            }
            has_notes = has_notes || self.part[i].has_notes();
        }

        if !has_notes && self.internal_clock() && self.started_by_keyboard {
            self.stop_count_down = 12;
        }

        thru
    }

    pub fn control_change(&mut self, channel: u8, controller: u8, value: u8) -> bool {
        let mut thru = true;
        for i in 0..self.num_active_parts as usize {
            if self.part[i].accepts_channel(channel) {
                thru = self.part[i].control_change(&mut self.voice, channel, controller, value) && thru;
            }
        }
        thru
    }

    pub fn pitch_bend(&mut self, channel: u8, pitch_bend: u16) -> bool {
        let mut thru = true;
        for i in 0..self.num_active_parts as usize {
            if self.part[i].accepts_channel(channel) {
                thru = self.part[i].pitch_bend(&mut self.voice, channel, pitch_bend) && thru;
            }
        }
        thru
    }

    pub fn aftertouch_note(&mut self, channel: u8, note: u8, velocity: u8) -> bool {
        let mut thru = true;
        for i in 0..self.num_active_parts as usize {
            if self.part[i].accepts_note(channel, note) {
                thru = self.part[i].aftertouch_note(&mut self.voice, channel, note, velocity) && thru;
            }
        }
        thru
    }

    pub fn aftertouch(&mut self, channel: u8, velocity: u8) -> bool {
        let mut thru = true;
        for i in 0..self.num_active_parts as usize {
            if self.part[i].accepts_channel(channel) {
                thru = self.part[i].aftertouch(&mut self.voice, channel, velocity) && thru;
            }
        }
        thru
    }

    /// `Multi::Reset()` -- resets every active part. Renamed from `reset`
    /// to avoid clashing with the CV-gate boolean getter below (the C++
    /// has both `Reset()` and `reset() const`, distinguished only by case,
    /// which Rust can't do).
    pub fn reset_all(&mut self, midi_out: &mut dyn MidiOut) {
        let mut tuning = TuningContext {
            jip: &mut self.just_intonation_processor,
            custom_pitch_table: &self.settings.custom_pitch_table,
        };
        for i in 0..self.num_active_parts as usize {
            self.part[i].reset(&mut self.voice, &mut tuning, midi_out);
        }
    }

    pub fn clock(&mut self, midi_out: &mut dyn MidiOut) {
        if !self.running {
            return;
        }

        let output_division = CLOCK_DIVISIONS[self.settings.clock_output_division as usize];
        let input_division = self.settings.clock_input_division as u16;

        if self.previous_output_division != 0 && output_division != self.previous_output_division {
            self.needs_resync = true;
        }
        self.previous_output_division = output_division;

        // Logic equation for computing a clock output with a 50% duty
        // cycle.
        if output_division > 1 {
            if self.clock_output_prescaler == 0 && self.clock_input_prescaler == 0 {
                self.clock_pulse_counter = 0xffff;
            }
            if self.clock_output_prescaler as u16 >= (output_division >> 1)
                && self.clock_input_prescaler as u16 >= (input_division >> 1)
            {
                self.clock_pulse_counter = 0;
            }
        } else if input_division > 1 {
            self.clock_pulse_counter = if (self.clock_input_prescaler as u16) <= (input_division - 1) >> 1 {
                0xffff
            } else {
                0
            };
        } else {
            // Because no division is used, neither on the output nor on the
            // input, we don't have a sufficiently fast time base to derive
            // a 50% duty cycle output. Instead, we output 5ms pulses.
            self.clock_pulse_counter = 40;
        }

        if self.clock_input_prescaler == 0 {
            midi_out.clock_tick();

            self.swing_counter += 1;
            if self.swing_counter >= 12 {
                self.swing_counter = 0;
            }

            if self.song_pointer.is_some() {
                self.clock_song(midi_out);
            } else if self.internal_clock() {
                self.swing_predelay[self.swing_counter as usize] = 0;
            } else {
                let interval = self.midi_clock_tick_duration as u32;
                self.midi_clock_tick_duration = 0;

                let modulation = if self.swing_counter < 6 { self.swing_counter } else { 12 - self.swing_counter } as u32;
                self.swing_predelay[self.swing_counter as usize] =
                    ((27u32 * modulation * interval * self.settings.clock_swing as u32) >> 13) as i16;
            }

            self.bar_position += 1;
            if self.bar_position >= self.settings.clock_bar_duration as u16 * 24 {
                self.bar_position = 0;
            }
            if self.bar_position == 0 {
                self.reset_pulse_counter = if self.settings.nudge_first_tick { 9 } else { 81 };
                if self.needs_resync {
                    self.clock_output_prescaler = 0;
                    self.needs_resync = false;
                }
            }
            if self.settings.clock_bar_duration > MAX_BAR_DURATION {
                self.bar_position = 1;
            }

            self.clock_output_prescaler += 1;
            if self.clock_output_prescaler as u16 >= output_division {
                self.clock_output_prescaler = 0;
            }
        }

        self.clock_input_prescaler += 1;
        if self.clock_input_prescaler >= self.settings.clock_input_division {
            self.clock_input_prescaler = 0;
        }

        if self.stop_count_down != 0 {
            self.stop_count_down -= 1;
            if self.stop_count_down == 0 && self.started_by_keyboard && self.internal_clock() {
                self.stop(midi_out);
            }
        }
    }

    pub fn start(&mut self, started_by_keyboard: bool, midi_out: &mut dyn MidiOut) {
        if self.running || self.recording {
            return;
        }
        if self.internal_clock() {
            self.internal_clock_ticks = 0;
            self.internal_clock.start(self.settings.clock_tempo as u32, self.settings.clock_swing as u32);
        }
        midi_out.start();

        self.started_by_keyboard = started_by_keyboard;
        self.running = true;
        self.latched = false;
        self.clock_input_prescaler = 0;
        self.clock_output_prescaler = 0;
        self.stop_count_down = 0;
        self.bar_position = u16::MAX;
        self.swing_counter = u8::MAX;
        self.previous_output_division = 0;
        self.needs_resync = false;

        self.swing_predelay = [-1; 12];

        for i in 0..self.num_active_parts as usize {
            self.part[i].start(started_by_keyboard);
        }
        self.song_pointer = None;
        self.midi_clock_tick_duration = 0;
    }

    pub fn continue_(&mut self, midi_out: &mut dyn MidiOut) {
        self.start(false, midi_out);
    }

    pub fn stop(&mut self, midi_out: &mut dyn MidiOut) {
        if !self.running() {
            return;
        }
        let mut tuning = TuningContext {
            jip: &mut self.just_intonation_processor,
            custom_pitch_table: &self.settings.custom_pitch_table,
        };
        for i in 0..self.num_active_parts as usize {
            self.part[i].stop(&mut self.voice, &mut tuning, midi_out);
        }
        midi_out.stop();
        self.clock_pulse_counter = 0;
        self.reset_pulse_counter = 0;
        self.stop_count_down = 0;
        self.running = false;
        self.latched = false;
        self.started_by_keyboard = false;
        self.song_pointer = None;
    }

    pub fn start_recording(&mut self, part: usize, midi_out: &mut dyn MidiOut) {
        if !self.recording {
            // Do not record while the arpeggiator is running!
            if self.started_by_keyboard && self.running() {
                self.stop(midi_out);
            }
            self.part[part].start_recording();
            let channel = self.part[part].midi_settings().channel;
            for i in 0..self.num_active_parts as usize {
                if self.part[i].midi_settings().channel == channel
                    || channel == 0x10
                    || self.part[i].midi_settings().channel == 0x10
                {
                    self.part[i].set_transposable(false);
                }
            }
            self.recording = true;
        }
    }

    pub fn stop_recording(&mut self, part: usize) {
        if self.recording {
            self.part[part].stop_recording();
            for i in 0..self.num_active_parts as usize {
                self.part[i].set_transposable(true);
            }
            self.recording = false;
        }
    }

    pub fn latch(&mut self) {
        if !self.latched {
            for i in 0..self.num_active_parts as usize {
                self.part[i].latch();
            }
            self.latched = true;
        }
    }

    pub fn unlatch(&mut self) {
        if self.latched {
            for i in 0..self.num_active_parts as usize {
                self.part[i].unlatch();
            }
            self.latched = false;
        }
    }

    pub fn push_it_note_on(&mut self, midi_out: &mut dyn MidiOut, mut note: u8) {
        let mask: u8 = if self.recording { 0x80 } else { 0 };
        let mut tuning = TuningContext {
            jip: &mut self.just_intonation_processor,
            custom_pitch_table: &self.settings.custom_pitch_table,
        };
        for i in 0..self.num_active_parts as usize {
            if self.settings.layout == Layout::QuadTriggers {
                note = self.part[i].midi_settings().min_note;
            }
            if !self.recording || self.part[i].recording() {
                let tx_channel = self.part[i].tx_channel();
                self.part[i].note_on(&mut self.voice, &mut tuning, midi_out, tx_channel | mask, note, 127);
            }
        }
        if !self.running() && self.internal_clock() {
            // Start the arpeggiators.
            self.start(true, midi_out);
        }
    }

    pub fn push_it_note_off(&mut self, midi_out: &mut dyn MidiOut, mut note: u8) {
        let mask: u8 = if self.recording { 0x80 } else { 0 };
        let mut has_notes = false;
        {
            let mut tuning = TuningContext {
                jip: &mut self.just_intonation_processor,
                custom_pitch_table: &self.settings.custom_pitch_table,
            };
            for i in 0..self.num_active_parts as usize {
                if self.settings.layout == Layout::QuadTriggers {
                    note = self.part[i].midi_settings().min_note;
                }
                if !self.recording || self.part[i].recording() {
                    let tx_channel = self.part[i].tx_channel();
                    self.part[i].note_off(&mut self.voice, &mut tuning, midi_out, tx_channel | mask, note);
                }
                has_notes = has_notes || self.part[i].has_notes();
            }
        }
        if !has_notes && self.internal_clock() {
            self.stop(midi_out);
        }
    }

    pub fn touch(&mut self) {
        // `Stop()` in the C++ needs a `MidiOut`; a host calling `touch()`
        // (typically right after loading a preset) is expected to have
        // already stopped transport itself, or to accept the `OnStop()`
        // echo being skipped here. `update_layout`/`part.touch` don't
        // depend on it either way.
        self.running = false;
        self.latched = false;
        self.started_by_keyboard = false;

        self.internal_clock.set_tempo(self.settings.clock_tempo as u32);
        self.update_layout();

        for i in 0..NUM_PARTS {
            self.part[i].touch(&mut self.voice);
        }
    }

    pub fn refresh_internal_clock(&mut self) {
        if self.running() && self.internal_clock() && self.internal_clock.process() {
            self.internal_clock_ticks += 1;
        }
    }

    pub fn process_internal_clock_events(&mut self, midi_out: &mut dyn MidiOut) {
        while self.internal_clock_ticks != 0 {
            self.clock(midi_out);
            self.internal_clock_ticks -= 1;
        }
    }

    pub fn render_audio(&mut self) {
        for voice in self.voice.iter_mut() {
            voice.render_audio();
        }
    }

    pub fn refresh(&mut self, midi_out: &mut dyn MidiOut) {
        if self.clock_pulse_counter != 0 {
            self.clock_pulse_counter -= 1;
        }
        if self.reset_pulse_counter != 0 {
            self.reset_pulse_counter -= 1;
        }

        self.midi_clock_tick_duration = self.midi_clock_tick_duration.wrapping_add(1);
        for i in 0..12 {
            if self.swing_predelay[i] == 0 {
                let mut tuning = TuningContext {
                    jip: &mut self.just_intonation_processor,
                    custom_pitch_table: &self.settings.custom_pitch_table,
                };
                for j in 0..self.num_active_parts as usize {
                    self.part[j].clock(&mut self.voice, &mut tuning, midi_out);
                }
            }
            if self.swing_predelay[i] >= 0 {
                self.swing_predelay[i] -= 1;
            }
        }

        for voice in self.voice.iter_mut() {
            voice.refresh();
        }
    }

    /// Sets the tempo (`Set(MULTI_CLOCK_TEMPO, ...)` in the C++, which also
    /// pushes the new value into `internal_clock`).
    pub fn set_tempo(&mut self, tempo: u8) {
        self.settings.clock_tempo = tempo;
        self.internal_clock.set_tempo(tempo as u32);
    }

    /// Sets the swing amount (`Set(MULTI_CLOCK_SWING, ...)` in the C++).
    pub fn set_swing(&mut self, swing: u8) {
        self.settings.clock_swing = swing;
        self.internal_clock.set_swing(swing as u32);
    }

    /// Sets the output layout, reproducing `Set(MULTI_LAYOUT, ...)`'s
    /// `ChangeLayout` side effect.
    pub fn set_layout(&mut self, layout: Layout) {
        let old_layout = self.settings.layout;
        if layout == old_layout {
            return;
        }
        self.settings.layout = layout;
        self.change_layout(old_layout, layout);
    }

    pub fn get_cv_gate(&self, cv: &mut [u16; 4], gate: &mut [bool; 4]) {
        match self.settings.layout {
            Layout::Mono | Layout::DualPolychained => {
                cv[0] = self.voice[0].note_dac_code();
                cv[1] = self.voice[0].velocity_dac_code();
                cv[2] = self.voice[0].aux_cv_dac_code();
                cv[3] = self.voice[0].aux_cv_dac_code_2();
                gate[0] = self.voice[0].gate();
                gate[1] = self.voice[0].trigger();
                gate[2] = self.clock_signal();
                gate[3] = self.reset_or_playing_flag();
            }
            Layout::DualMono => {
                cv[0] = self.voice[0].note_dac_code();
                cv[1] = self.voice[1].note_dac_code();
                cv[2] = self.voice[0].aux_cv_dac_code();
                cv[3] = self.voice[1].aux_cv_dac_code();
                gate[0] = self.voice[0].gate();
                gate[1] = self.voice[1].gate();
                gate[2] = self.clock_signal();
                gate[3] = self.reset_or_playing_flag();
            }
            Layout::DualPoly | Layout::QuadPolychained => {
                cv[0] = self.voice[0].note_dac_code();
                cv[1] = self.voice[1].note_dac_code();
                cv[2] = self.voice[0].aux_cv_dac_code();
                cv[3] = self.voice[1].aux_cv_dac_code_2();
                gate[0] = self.voice[0].gate();
                gate[1] = self.voice[1].gate();
                gate[2] = self.clock_signal();
                gate[3] = self.reset_or_playing_flag();
            }
            Layout::QuadMono | Layout::QuadPoly | Layout::OctalPolychained | Layout::ThreeOne => {
                cv[0] = self.voice[0].note_dac_code();
                cv[1] = self.voice[1].note_dac_code();
                cv[2] = self.voice[2].note_dac_code();
                cv[3] = self.voice[3].note_dac_code();
                gate[0] = self.voice[0].gate();
                gate[1] = self.voice[1].gate();
                if self.settings.clock_override {
                    gate[2] = self.clock_signal();
                    gate[3] = self.reset_or_playing_flag();
                } else {
                    gate[2] = self.voice[2].gate();
                    gate[3] = self.voice[3].gate();
                }
            }
            Layout::QuadTriggers => {
                cv[0] = self.voice[0].trigger_dac_code();
                cv[1] = self.voice[1].trigger_dac_code();
                cv[2] = self.voice[2].trigger_dac_code();
                cv[3] = self.voice[3].trigger_dac_code();
                gate[0] = self.voice[0].trigger() && !self.voice[1].gate();
                gate[1] = self.voice[0].trigger() && self.voice[1].gate();
                gate[2] = self.clock_signal();
                gate[3] = self.reset_or_playing_flag();
            }
            Layout::QuadVoltages => {
                cv[0] = self.voice[0].aux_cv_dac_code();
                cv[1] = self.voice[1].aux_cv_dac_code();
                cv[2] = self.voice[2].aux_cv_dac_code();
                cv[3] = self.voice[3].aux_cv_dac_code();
                gate[0] = self.voice[0].gate();
                gate[1] = self.voice[1].gate();
                if self.settings.clock_override {
                    gate[2] = self.clock_signal();
                    gate[3] = self.reset_or_playing_flag();
                } else {
                    gate[2] = self.voice[2].gate();
                    gate[3] = self.voice[3].gate();
                }
            }
        }
    }

    pub fn get_audio_source(&self, audio_source: &mut [u8; 4]) -> bool {
        const NONE: u8 = 0xff;
        match self.settings.layout {
            Layout::Mono | Layout::DualPolychained => {
                *audio_source = [NONE, NONE, NONE, if self.voice[0].audio_mode() != 0 { 0 } else { NONE }];
                self.voice[0].audio_mode() != 0
            }
            Layout::DualMono | Layout::DualPoly | Layout::QuadPolychained => {
                *audio_source = [
                    NONE,
                    NONE,
                    if self.voice[0].audio_mode() != 0 { 0 } else { NONE },
                    if self.voice[1].audio_mode() != 0 { 1 } else { NONE },
                ];
                self.voice[0].audio_mode() != 0 || self.voice[1].audio_mode() != 0
            }
            Layout::QuadMono | Layout::QuadPoly | Layout::OctalPolychained | Layout::ThreeOne => {
                *audio_source = [
                    if self.voice[0].audio_mode() != 0 { 0 } else { NONE },
                    if self.voice[1].audio_mode() != 0 { 1 } else { NONE },
                    if self.voice[2].audio_mode() != 0 { 2 } else { NONE },
                    if self.voice[3].audio_mode() != 0 { 3 } else { NONE },
                ];
                self.voice.iter().any(|v| v.audio_mode() != 0)
            }
            Layout::QuadTriggers | Layout::QuadVoltages => {
                *audio_source = [NONE; 4];
                false
            }
        }
    }

    fn update_layout(&mut self) {
        // Reset and close all parts and voices (note: `Reset()` needs a
        // `MidiOut`/`TuningContext` in the C++ via `Stop()`; here we just
        // silence the voices directly, matching the *audible* effect
        // without requiring a `MidiOut` in this signature -- callers that
        // need the full `Reset()` echo behavior should call
        // `Part::reset`/`Multi::stop` themselves beforehand.)
        for voice in self.voice.iter_mut() {
            voice.note_off();
        }

        match self.settings.layout {
            Layout::Mono | Layout::DualMono | Layout::QuadMono => {
                let num_parts = match self.settings.layout {
                    Layout::Mono => 1,
                    Layout::DualMono => 2,
                    _ => 4,
                };
                for i in 0..num_parts {
                    self.part[i].allocate_voices(&mut self.voice, i, 1, false);
                }
                self.num_active_parts = num_parts as u8;
            }
            Layout::DualPoly | Layout::QuadPoly | Layout::DualPolychained | Layout::QuadPolychained | Layout::OctalPolychained => {
                let num_voices = if self.settings.layout == Layout::DualPoly || self.settings.layout == Layout::QuadPolychained {
                    2
                } else if self.settings.layout == Layout::DualPolychained {
                    1
                } else {
                    4
                };
                let polychain = self.settings.layout >= Layout::DualPolychained;
                self.part[0].allocate_voices(&mut self.voice, 0, num_voices, polychain);
                self.num_active_parts = 1;
            }
            Layout::QuadTriggers | Layout::QuadVoltages => {
                for i in 0..4 {
                    self.part[i].allocate_voices(&mut self.voice, i, 1, false);
                }
                self.num_active_parts = 4;
            }
            Layout::ThreeOne => {
                self.part[0].allocate_voices(&mut self.voice, 0, 3, false);
                self.part[1].allocate_voices(&mut self.voice, 3, 1, false);
                self.num_active_parts = 2;
            }
        }
    }

    fn change_layout(&mut self, old_layout: Layout, new_layout: Layout) {
        for voice in self.voice.iter_mut() {
            voice.note_off();
        }

        match new_layout {
            Layout::Mono | Layout::DualMono | Layout::QuadMono => {
                let num_parts = match new_layout {
                    Layout::Mono => 1,
                    Layout::DualMono => 2,
                    _ => 4,
                };

                for i in 0..num_parts {
                    let mut midi = *self.part[i].midi_settings();
                    if old_layout == Layout::QuadTriggers {
                        midi.min_note = 0;
                        midi.max_note = 127;
                    }
                    midi.min_velocity = 0;
                    midi.max_velocity = 127;
                    self.part[i].set_midi_settings(&mut self.voice, midi);

                    let mut voicing = *self.part[i].voicing_settings();
                    voicing.allocation_mode = VoiceAllocationMode::Mono;
                    voicing.allocation_priority = 0; // NoteStackPriority::Last
                    self.part[i].set_voicing_settings(&mut self.voice, voicing);
                }

                // Duplicate uninitialized voices.
                for i in 1..num_parts {
                    let source = i % self.num_active_parts as usize;
                    if i != source {
                        let midi = *self.part[source].midi_settings();
                        let voicing = *self.part[source].voicing_settings();
                        let seq = *self.part[source].sequencer_settings();
                        self.part[i].set_midi_settings(&mut self.voice, midi);
                        self.part[i].set_voicing_settings(&mut self.voice, voicing);
                        self.part[i].set_sequencer_settings(seq);
                    }
                }

                for i in 0..num_parts {
                    self.part[i].allocate_voices(&mut self.voice, i, 1, false);
                }
                self.num_active_parts = num_parts as u8;
            }
            Layout::DualPoly | Layout::QuadPoly | Layout::DualPolychained | Layout::QuadPolychained | Layout::OctalPolychained => {
                let num_voices = if self.settings.layout == Layout::DualPoly || self.settings.layout == Layout::QuadPolychained {
                    2
                } else if self.settings.layout == Layout::DualPolychained {
                    1
                } else {
                    4
                };

                let mut midi = *self.part[0].midi_settings();
                if old_layout == Layout::QuadTriggers {
                    midi.min_note = 0;
                    midi.max_note = 127;
                }
                midi.min_velocity = 0;
                midi.max_velocity = 127;
                self.part[0].set_midi_settings(&mut self.voice, midi);

                let mut voicing = *self.part[0].voicing_settings();
                voicing.allocation_mode = VoiceAllocationMode::Poly;
                voicing.allocation_priority = 0;
                voicing.portamento = 0;
                voicing.legato_mode = false;
                self.part[0].set_voicing_settings(&mut self.voice, voicing);

                let polychain = new_layout >= Layout::DualPolychained;
                self.part[0].allocate_voices(&mut self.voice, 0, num_voices, polychain);
                self.num_active_parts = 1;
            }
            Layout::QuadTriggers => {
                for i in 0..4 {
                    let mut midi = *self.part[i].midi_settings();
                    if old_layout != Layout::QuadTriggers {
                        midi.min_note = 36 + i as u8 * 2;
                        midi.max_note = 36 + i as u8 * 2;
                    }
                    midi.min_velocity = 0;
                    midi.max_velocity = 127;
                    midi.channel = self.part[0].midi_settings().channel;
                    midi.out_mode = self.part[0].midi_settings().out_mode;
                    self.part[i].set_midi_settings(&mut self.voice, midi);

                    let mut voicing = *self.part[i].voicing_settings();
                    voicing.allocation_mode = VoiceAllocationMode::Mono;
                    voicing.allocation_priority = 0;
                    voicing.portamento = 0;
                    voicing.legato_mode = false;
                    self.part[i].set_voicing_settings(&mut self.voice, voicing);
                }

                // Duplicate sequencer settings.
                for i in 1..4 {
                    let source = i % self.num_active_parts as usize;
                    if i != source {
                        let seq = *self.part[source].sequencer_settings();
                        self.part[i].set_sequencer_settings(seq);
                    }
                }

                for i in 0..4 {
                    self.part[i].allocate_voices(&mut self.voice, i, 1, false);
                }
                self.num_active_parts = 4;
            }
            Layout::ThreeOne => {
                let mut midi = *self.part[0].midi_settings();
                if old_layout == Layout::QuadTriggers {
                    midi.min_note = 0;
                    midi.max_note = 127;
                }
                midi.min_velocity = 0;
                midi.max_velocity = 127;
                self.part[0].set_midi_settings(&mut self.voice, midi);

                let mut voicing = *self.part[0].voicing_settings();
                voicing.allocation_mode = VoiceAllocationMode::Poly;
                voicing.allocation_priority = 0;
                voicing.portamento = 0;
                voicing.legato_mode = false;
                self.part[0].set_voicing_settings(&mut self.voice, voicing);
                self.part[0].allocate_voices(&mut self.voice, 0, 3, false);

                let mut midi = *self.part[1].midi_settings();
                if old_layout == Layout::QuadTriggers {
                    midi.min_note = 0;
                    midi.max_note = 127;
                }
                midi.min_velocity = 0;
                midi.max_velocity = 127;
                self.part[1].set_midi_settings(&mut self.voice, midi);

                let mut voicing = *self.part[1].voicing_settings();
                voicing.allocation_mode = VoiceAllocationMode::Mono;
                voicing.allocation_priority = 0;
                voicing.portamento = 0;
                voicing.legato_mode = false;
                self.part[1].set_voicing_settings(&mut self.voice, voicing);
                self.part[1].allocate_voices(&mut self.voice, 3, 1, false);

                self.num_active_parts = 2;
            }
            Layout::QuadVoltages => {
                let num_parts = 4;
                for i in 0..num_parts {
                    let mut midi = *self.part[i].midi_settings();
                    if old_layout == Layout::QuadTriggers {
                        midi.min_note = 0;
                        midi.max_note = 127;
                    }
                    midi.min_velocity = 0;
                    midi.max_velocity = 127;
                    self.part[i].set_midi_settings(&mut self.voice, midi);

                    let mut voicing = *self.part[i].voicing_settings();
                    voicing.allocation_mode = VoiceAllocationMode::Mono;
                    voicing.allocation_priority = 0;
                    self.part[i].set_voicing_settings(&mut self.voice, voicing);
                }

                // Duplicate uninitialized voices.
                for i in 1..num_parts {
                    let source = i % self.num_active_parts as usize;
                    if i != source {
                        let midi = *self.part[source].midi_settings();
                        let voicing = *self.part[source].voicing_settings();
                        let seq = *self.part[source].sequencer_settings();
                        self.part[i].set_midi_settings(&mut self.voice, midi);
                        self.part[i].set_voicing_settings(&mut self.voice, voicing);
                        self.part[i].set_sequencer_settings(seq);
                    }
                }
                for i in 0..num_parts {
                    self.part[i].allocate_voices(&mut self.voice, i, 1, false);
                }
                self.num_active_parts = num_parts as u8;
            }
        }

        let has_siblings = self.num_active_parts > 1;
        for i in 0..self.num_active_parts as usize {
            self.part[i].set_siblings(has_siblings);
        }
    }

    pub fn start_song(&mut self, midi_out: &mut dyn MidiOut) {
        self.set_layout(Layout::QuadMono);
        {
            let mut v = *self.part[0].voicing_settings();
            v.audio_mode = 0x83;
            self.part[0].set_voicing_settings(&mut self.voice, v);
        }
        {
            let mut v = *self.part[1].voicing_settings();
            v.audio_mode = 0x83;
            self.part[1].set_voicing_settings(&mut self.voice, v);
        }
        {
            let mut v = *self.part[2].voicing_settings();
            v.audio_mode = 0x84;
            self.part[2].set_voicing_settings(&mut self.voice, v);
        }
        {
            let mut v = *self.part[3].voicing_settings();
            v.audio_mode = 0x86;
            self.part[3].set_voicing_settings(&mut self.voice, v);
        }
        self.update_layout();
        self.settings.clock_tempo = 140;
        self.stop(midi_out);
        self.start(false, midi_out);

        self.song_pointer = Some(0);
        self.song_clock = 0;
        self.song_delta = 0;
    }

    fn clock_song(&mut self, midi_out: &mut dyn MidiOut) {
        let mut tuning = TuningContext {
            jip: &mut self.just_intonation_processor,
            custom_pitch_table: &self.settings.custom_pitch_table,
        };
        while self.song_clock >= self.song_delta as u32 {
            let mut pointer = self.song_pointer.unwrap();
            if SONG[pointer] == 255 {
                pointer = 0;
            }
            if SONG[pointer] == 254 {
                self.song_delta += 6;
            } else {
                let part = (SONG[pointer] >> 6) as usize;
                let note = SONG[pointer] & 0x3f;
                if note == 0 {
                    self.part[part].all_notes_off(&mut self.voice);
                } else {
                    self.part[part].note_on(&mut self.voice, &mut tuning, midi_out, 0, note + 24, 100);
                }
                self.song_clock = 0;
                self.song_delta = 0;
            }
            pointer += 1;
            self.song_pointer = Some(pointer);
        }
        self.song_clock += 1;
    }

    pub fn set_custom_pitch(&mut self, pitch_class: usize, correction: i8) {
        self.settings.custom_pitch_table[pitch_class] = correction;
    }

    pub fn layout(&self) -> Layout {
        self.settings.layout
    }
    pub fn internal_clock(&self) -> bool {
        self.settings.clock_tempo >= 40
    }
    pub fn tempo(&self) -> u8 {
        self.settings.clock_tempo
    }
    pub fn running(&self) -> bool {
        self.running
    }
    pub fn latched(&self) -> bool {
        self.latched
    }
    pub fn recording(&self) -> bool {
        self.recording
    }
    /// `Multi::clock() const` -- the CV-gate clock signal (distinct from
    /// [`Multi::clock`], the tick-processing action; the C++ tells the two
    /// apart only by case, `Clock()` vs `clock()`, which Rust can't do).
    pub fn clock_signal(&self) -> bool {
        self.clock_pulse_counter > 0
            && (!self.settings.nudge_first_tick || self.settings.clock_bar_duration == 0 || !self.reset())
    }
    pub fn reset(&self) -> bool {
        self.reset_pulse_counter > 0
    }
    pub fn reset_or_playing_flag(&self) -> bool {
        self.reset() || (self.settings.clock_bar_duration == 0 && self.running)
    }

    pub fn part(&self, index: usize) -> &Part {
        &self.part[index]
    }
    pub fn mutable_part(&mut self, index: usize) -> &mut Part {
        &mut self.part[index]
    }

    /// Reconfigures a part's MIDI filter settings. `Part::set_midi_settings`
    /// itself needs `&mut [Voice]` (the whole array `Multi` owns, per
    /// `part.rs`'s module doc comment) which isn't reachable through
    /// `mutable_part` alone -- this is the entry point a host actually
    /// calls.
    pub fn set_part_midi_settings(&mut self, index: usize, midi: MidiSettings) {
        self.part[index].set_midi_settings(&mut self.voice, midi);
    }

    /// Reconfigures a part's voicing settings -- see
    /// `set_part_midi_settings`'s doc comment.
    pub fn set_part_voicing_settings(&mut self, index: usize, voicing: VoicingSettings) {
        self.part[index].set_voicing_settings(&mut self.voice, voicing);
    }
    pub fn voice(&self, index: usize) -> &Voice {
        &self.voice[index]
    }
    pub fn mutable_voice(&mut self, index: usize) -> &mut Voice {
        &mut self.voice[index]
    }
    pub fn settings(&self) -> &MultiSettings {
        &self.settings
    }
    pub fn num_active_parts(&self) -> u8 {
        self.num_active_parts
    }

    /// Returns `true` when no part does anything fancy with the MIDI
    /// stream (arpeggiation, message suppression). A host can use this to
    /// decide whether it needs to reformat/merge/delay its MIDI-out stream,
    /// or can just copy bytes straight through as they arrive.
    pub fn direct_thru(&self) -> bool {
        (0..self.num_active_parts as usize).all(|i| self.part[i].direct_thru())
    }
}
