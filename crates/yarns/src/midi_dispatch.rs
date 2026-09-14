//! `yarns/midi_handler.{cc,h}`'s dispatch functions -- the glue between a
//! raw MIDI byte stream and [`Multi`]. Pair a `stmlib::MidiStreamParser`
//! with a [`MidiDispatch`] to get the C++'s full pipeline: bytes in ->
//! `MidiStreamParser` decodes running status/realtime/SysEx framing ->
//! `MidiDispatch` forwards decoded events to `Multi` and echoes the
//! selective per-message MIDI thru the C++'s `MidiHandler::NoteOn`/
//! `ControlChange`/etc. did (`if (multi.NoteOn(...) && !multi.direct_thru())
//! { Send3(...); }`) -- via `&mut dyn MidiOut`, same as `Part`'s internal
//! note generation.
//!
//! What's *not* here, and why (see the crate's `PORTING.md` for the full
//! writeup): `MidiHandler`'s SysEx handling (`SysExStart`/`SysExByte`/
//! `SysExEnd`, all defaulted to no-ops by not overriding them -- see
//! `stmlib::MidiEventHandler`) is entirely calibration/preset-storage
//! protocol, tied to `storage_manager` (out of scope). Its raw byte-queue
//! (`PushByte`/`ProcessInput`, a `RingBuffer<u8, 128>` an ISR fills and the
//! main loop drains into the parser) is host-side plumbing with no DSP
//! content -- a host can call [`stmlib::MidiStreamParser::push_byte`]
//! directly as bytes arrive, or use `stmlib::RingBuffer` itself if it wants
//! the same buffering.
//!
//! `MidiHandler::CheckChannel` always returns `true` in the C++ (channel
//! filtering happens in `Multi`/`Part`, not the parser) -- matches
//! `MidiEventHandler::check_channel`'s default, so `MidiDispatch` doesn't
//! override it.

use stmlib::MidiEventHandler;

use crate::midi_out::MidiOut;
use crate::multi::Multi;

/// See the module doc comment. Borrows both `multi` and `midi_out` for as
/// long as you're feeding it bytes; construct it fresh (it's a 2-word
/// struct of references, essentially free) whenever you have some.
pub struct MidiDispatch<'a> {
    pub multi: &'a mut Multi,
    pub midi_out: &'a mut dyn MidiOut,
}

impl<'a> MidiDispatch<'a> {
    pub fn new(multi: &'a mut Multi, midi_out: &'a mut dyn MidiOut) -> Self {
        Self { multi, midi_out }
    }
}

impl MidiEventHandler for MidiDispatch<'_> {
    fn note_on(&mut self, channel: u8, note: u8, velocity: u8) {
        let thru = self.multi.note_on(self.midi_out, channel, note, velocity);
        if thru && !self.multi.direct_thru() {
            self.midi_out.note_on(channel, note, velocity);
        }
    }

    fn note_off(&mut self, channel: u8, note: u8, velocity: u8) {
        let thru = self.multi.note_off(self.midi_out, channel, note, velocity);
        if thru && !self.multi.direct_thru() {
            self.midi_out.note_off(channel, note);
        }
    }

    fn aftertouch_note(&mut self, channel: u8, note: u8, velocity: u8) {
        let thru = self.multi.aftertouch_note(channel, note, velocity);
        if thru && !self.multi.direct_thru() {
            self.midi_out.aftertouch_note(channel, note, velocity);
        }
    }

    fn aftertouch_channel(&mut self, channel: u8, velocity: u8) {
        let thru = self.multi.aftertouch(channel, velocity);
        if thru && !self.multi.direct_thru() {
            self.midi_out.aftertouch_channel(channel, velocity);
        }
    }

    fn control_change(&mut self, channel: u8, controller: u8, value: u8) {
        let thru = self.multi.control_change(channel, controller, value);
        if thru && !self.multi.direct_thru() {
            self.midi_out.control_change(channel, controller, value);
        }
    }

    fn program_change(&mut self, channel: u8, program: u8) {
        // `Multi` has no `ProgramChange` of its own in the C++ either --
        // `MidiHandler::ProgramChange` checks `direct_thru()` directly.
        if !self.multi.direct_thru() {
            self.midi_out.program_change(channel, program);
        }
    }

    fn pitch_bend(&mut self, channel: u8, pitch_bend: u16) {
        let thru = self.multi.pitch_bend(channel, pitch_bend);
        if thru && !self.multi.direct_thru() {
            self.midi_out.pitch_bend(channel, pitch_bend);
        }
    }

    fn clock(&mut self) {
        if !self.multi.internal_clock() {
            self.multi.clock(self.midi_out);
        }
    }

    fn start(&mut self) {
        if !self.multi.internal_clock() {
            self.multi.start(false, self.midi_out);
        }
    }

    fn continue_(&mut self) {
        if !self.multi.internal_clock() {
            self.multi.continue_(self.midi_out);
        }
    }

    fn stop(&mut self) {
        if !self.multi.internal_clock() {
            self.multi.stop(self.midi_out);
        }
    }

    fn reset(&mut self) {
        self.multi.reset_all(self.midi_out);
    }

    fn raw_byte(&mut self, byte: u8) {
        if self.multi.direct_thru() && byte != 0xfa && byte != 0xf8 && byte != 0xfc {
            self.midi_out.raw_byte_thru(byte);
        }
    }
}
