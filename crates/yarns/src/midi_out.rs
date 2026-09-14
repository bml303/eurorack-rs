//! A host-provided sink for the MIDI events `Part`/`Multi` generate
//! themselves -- arpeggiator/sequencer note on/off, polychaining forwarding,
//! clock start/stop/tick echo -- and, since `midi_dispatch.rs`, the
//! selective per-message thru echo `yarns::MidiHandler`'s dispatch functions
//! did (`if (multi.NoteOn(...) && !multi.direct_thru()) { Send3(...); }`,
//! and the equivalent for CC/pitch-bend/aftertouch/program-change). In the
//! C++ these are static calls straight into `MidiHandler`, which owns the
//! actual byte-stream output buffers and SysEx machinery. `MidiHandler`'s
//! buffering/SysEx pieces are out of scope for this port (see the crate's
//! `PORTING.md`), so `Part`/`Multi`/`midi_dispatch` take `&mut dyn MidiOut`
//! wherever the C++ would reach for `midi_handler` -- a host wires this to
//! its own MIDI output however it sees fit. Every method defaults to a
//! no-op, so a host only implements the messages it cares about forwarding.

#[allow(unused_variables)]
pub trait MidiOut {
    fn note_on(&mut self, channel: u8, note: u8, velocity: u8) {}
    fn note_off(&mut self, channel: u8, note: u8) {}
    fn aftertouch_note(&mut self, channel: u8, note: u8, velocity: u8) {}
    fn aftertouch_channel(&mut self, channel: u8, velocity: u8) {}
    fn control_change(&mut self, channel: u8, controller: u8, value: u8) {}
    fn program_change(&mut self, channel: u8, program: u8) {}
    fn pitch_bend(&mut self, channel: u8, pitch_bend: u16) {}

    /// Called for arpeggiator/sequencer-generated notes and polychaining
    /// forwarding (`Part`'s internal note generation) -- as opposed to
    /// [`MidiOut::note_on`]/[`MidiOut::note_off`], called for the
    /// selective per-message thru of a *directly played* note. Both send
    /// the identical wire bytes; kept as separate methods since a host may
    /// want to tell the two apart (e.g. to avoid re-echoing a note it just
    /// received verbatim).
    fn internal_note_on(&mut self, channel: u8, note: u8, velocity: u8) {}
    fn internal_note_off(&mut self, channel: u8, note: u8) {}

    fn clock_tick(&mut self) {}
    fn start(&mut self) {}
    fn stop(&mut self) {}

    /// A raw incoming byte that should be forwarded verbatim when
    /// `Multi::direct_thru()` is true (`yarns::MidiHandler::RawByte`).
    /// Never called for `0xf8`/`0xfa`/`0xfc` (clock/start/stop), which
    /// already reach the host through
    /// [`MidiOut::clock_tick`]/[`MidiOut::start`]/[`MidiOut::stop`]
    /// regardless of `direct_thru` -- forwarding them here too would send
    /// them twice.
    fn raw_byte_thru(&mut self, byte: u8) {}
}

/// A `MidiOut` that discards every event -- for hosts that don't need
/// generated-event MIDI thru or polychaining.
#[derive(Debug, Default, Clone, Copy)]
pub struct NullMidiOut;

impl MidiOut for NullMidiOut {}
