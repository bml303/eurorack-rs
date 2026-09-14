//! A host-provided sink for the MIDI events `Part`/`Multi` generate
//! themselves -- arpeggiator/sequencer note on/off, polychaining forwarding,
//! and clock start/stop/tick echo. In the C++ these are static calls
//! straight into `MidiHandler` (`midi_handler.OnInternalNoteOn(...)`, etc.),
//! which owns the actual byte-stream output buffers and SysEx machinery.
//! `MidiHandler` itself is out of scope for this port (see the crate's
//! `PORTING.md`), so `Part`/`Multi` take `&mut dyn MidiOut` wherever the C++
//! would reach for `midi_handler` -- a host wires this to its own MIDI
//! output however it sees fit.

pub trait MidiOut {
    fn internal_note_on(&mut self, channel: u8, note: u8, velocity: u8);
    fn internal_note_off(&mut self, channel: u8, note: u8);
    fn clock_tick(&mut self);
    fn start(&mut self);
    fn stop(&mut self);
}

/// A `MidiOut` that discards every event -- for hosts that don't need
/// generated-event MIDI thru or polychaining.
#[derive(Debug, Default, Clone, Copy)]
pub struct NullMidiOut;

impl MidiOut for NullMidiOut {
    fn internal_note_on(&mut self, _channel: u8, _note: u8, _velocity: u8) {}
    fn internal_note_off(&mut self, _channel: u8, _note: u8) {}
    fn clock_tick(&mut self) {}
    fn start(&mut self) {}
    fn stop(&mut self) {}
}
