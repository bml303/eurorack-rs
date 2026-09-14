//! `stmlib/midi/midi.h` -- decoding of MIDI byte streams: running status,
//! realtime messages interleaved with data bytes, and SysEx framing.
//!
//! The C++ is a template, `MidiStreamParser<Handler>`, calling *static*
//! methods on `Handler` (C++'s compile-time "static polymorphism" -- there's
//! exactly one `Handler` per parser, known at compile time, so no vtable is
//! needed). Ported as a trait, [`MidiEventHandler`], with every method
//! defaulted to a no-op (`CheckChannel`'s default is `true`, matching the
//! C++'s only handler in this workspace, `yarns::MidiHandler::CheckChannel`)
//! so an implementer only overrides what it cares about, plus
//! [`MidiStreamParser::push_byte`] taking `&mut impl MidiEventHandler`
//! explicitly rather than the parser owning/aliasing it.

/// Callbacks a [`MidiStreamParser`] invokes as it decodes a byte stream.
/// Every method defaults to a no-op (`check_channel` to `true`) so an
/// implementer only needs to override the messages it cares about.
#[allow(unused_variables)]
pub trait MidiEventHandler {
    fn note_on(&mut self, channel: u8, note: u8, velocity: u8) {}
    fn note_off(&mut self, channel: u8, note: u8, velocity: u8) {}
    fn aftertouch_note(&mut self, channel: u8, note: u8, velocity: u8) {}
    fn aftertouch_channel(&mut self, channel: u8, velocity: u8) {}
    fn control_change(&mut self, channel: u8, controller: u8, value: u8) {}
    fn program_change(&mut self, channel: u8, program: u8) {}
    fn pitch_bend(&mut self, channel: u8, pitch_bend: u16) {}

    fn sysex_start(&mut self) {}
    fn sysex_byte(&mut self, byte: u8) {}
    fn sysex_end(&mut self) {}

    fn clock(&mut self) {}
    fn start(&mut self) {}
    fn continue_(&mut self) {}
    fn stop(&mut self) {}
    fn reset(&mut self) {}

    /// A data byte received with no preceding status byte (`running_status
    /// == 0`) -- malformed input, a dropped status byte, or a stream that
    /// started mid-message.
    fn bozo_byte(&mut self, byte: u8) {}
    /// Every byte, including realtime bytes, before any parsing -- mainly
    /// useful for MIDI-thru byte forwarding.
    fn raw_byte(&mut self, byte: u8) {}
    /// The complete status + data bytes of a channel message, plus whether
    /// [`MidiEventHandler::check_channel`] accepted it (`1`) or not (`0`).
    fn raw_midi_data(&mut self, status: u8, data: &[u8], accepted_channel: u8) {}
    /// Whether a channel message on `channel` should be processed at all.
    /// Returning `false` still calls [`MidiEventHandler::raw_midi_data`]
    /// (with `accepted_channel = 0`) but skips the specific
    /// note/CC/etc. callback.
    fn check_channel(&mut self, channel: u8) -> bool {
        true
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct MidiStreamParser {
    running_status: u8,
    data: [u8; 3],
    data_size: u8,          // Number of non-status bytes received.
    expected_data_size: u8, // Expected number of non-status bytes.
}

impl MidiStreamParser {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push_byte<H: MidiEventHandler>(&mut self, handler: &mut H, byte: u8) {
        // Active sensing messages are filtered at the source, the hard way...
        if byte == 0xfe {
            return;
        }
        handler.raw_byte(byte);
        // Realtime messages are immediately passed through, and do not
        // modify the state of the parser.
        if byte >= 0xf8 {
            self.message_received(handler, byte);
        } else {
            if byte >= 0x80 {
                let hi = byte & 0xf0;
                let lo = byte & 0x0f;
                self.data_size = 0;
                self.expected_data_size = 1;
                match hi {
                    0x80 | 0x90 | 0xa0 | 0xb0 => self.expected_data_size = 2,
                    0xc0 | 0xd0 => {} // default data size of 1.
                    0xe0 => self.expected_data_size = 2,
                    0xf0 => {
                        if lo > 0 && lo < 3 {
                            self.expected_data_size = 2;
                        } else if lo >= 4 {
                            self.expected_data_size = 0;
                        }
                    }
                    _ => {}
                }
                if byte == 0xf7 {
                    if self.running_status == 0xf0 {
                        handler.sysex_end();
                    }
                    self.running_status = 0;
                } else if byte == 0xf0 {
                    self.running_status = 0xf0;
                    handler.sysex_start();
                } else {
                    self.running_status = byte;
                }
            } else {
                self.data[self.data_size as usize] = byte;
                self.data_size += 1;
            }
            if self.data_size >= self.expected_data_size {
                let running_status = self.running_status;
                self.message_received(handler, running_status);
                self.data_size = 0;
                if self.running_status > 0xf0 {
                    self.expected_data_size = 0;
                    self.running_status = 0;
                }
            }
        }
    }

    fn message_received<H: MidiEventHandler>(&mut self, handler: &mut H, status: u8) {
        if status == 0 {
            handler.bozo_byte(self.data[0]);
        }

        let hi = status & 0xf0;
        let lo = status & 0x0f;

        // If this is a channel-specific message, check first that the
        // receiver is tuned to this channel.
        if hi != 0xf0 && !handler.check_channel(lo) {
            handler.raw_midi_data(status, &self.data[..self.data_size as usize], 0);
            return;
        }
        if status != 0xf0 && status != 0xf7 {
            handler.raw_midi_data(status, &self.data[..self.data_size as usize], 1);
        }
        match hi {
            0x80 => handler.note_off(lo, self.data[0], self.data[1]),
            0x90 => {
                if self.data[1] != 0 {
                    handler.note_on(lo, self.data[0], self.data[1]);
                } else {
                    handler.note_off(lo, self.data[0], 0);
                }
            }
            0xa0 => handler.aftertouch_note(lo, self.data[0], self.data[1]),
            0xb0 => handler.control_change(lo, self.data[0], self.data[1]),
            0xc0 => handler.program_change(lo, self.data[0]),
            0xd0 => handler.aftertouch_channel(lo, self.data[0]),
            0xe0 => handler.pitch_bend(lo, ((self.data[1] as u16) << 7) + self.data[0] as u16),
            0xf0 => match lo {
                0x0 => handler.sysex_byte(self.data[0]),
                // 0x1..=0x6: MTC quarter-frame, song position/select, tune
                // request -- unimplemented in the C++ too
                // ("TODO(pichenettes): implement this if it makes sense.").
                0x8 => handler.clock(),
                0xa => handler.start(),
                0xb => handler.continue_(),
                0xc => handler.stop(),
                0xf => handler.reset(),
                _ => {}
            },
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate alloc;

    use super::*;

    #[derive(Default)]
    struct Recorder {
        events: alloc::vec::Vec<alloc::string::String>,
    }

    impl MidiEventHandler for Recorder {
        fn note_on(&mut self, channel: u8, note: u8, velocity: u8) {
            self.events.push(alloc::format!("on {channel} {note} {velocity}"));
        }
        fn note_off(&mut self, channel: u8, note: u8, velocity: u8) {
            self.events.push(alloc::format!("off {channel} {note} {velocity}"));
        }
        fn clock(&mut self) {
            self.events.push("clock".into());
        }
    }

    fn push_all(parser: &mut MidiStreamParser, handler: &mut Recorder, bytes: &[u8]) {
        for &b in bytes {
            parser.push_byte(handler, b);
        }
    }

    #[test]
    fn running_status_repeats_the_last_status_byte() {
        let mut parser = MidiStreamParser::new();
        let mut rec = Recorder::default();
        // Note on channel 0, then two more note-on messages using running
        // status (no repeated 0x90 byte).
        push_all(&mut parser, &mut rec, &[0x90, 60, 100, 64, 90, 67, 80]);
        assert_eq!(
            rec.events,
            ["on 0 60 100", "on 0 64 90", "on 0 67 80"]
        );
    }

    #[test]
    fn note_on_with_zero_velocity_is_a_note_off() {
        let mut parser = MidiStreamParser::new();
        let mut rec = Recorder::default();
        push_all(&mut parser, &mut rec, &[0x90, 60, 0]);
        assert_eq!(rec.events, ["off 0 60 0"]);
    }

    #[test]
    fn realtime_bytes_interleave_without_disturbing_running_status() {
        let mut parser = MidiStreamParser::new();
        let mut rec = Recorder::default();
        // A note-on split across two pushes, with a clock byte (0xf8)
        // injected in between the status and its data bytes -- the clock
        // must fire immediately without resetting the in-progress message.
        parser.push_byte(&mut rec, 0x90);
        parser.push_byte(&mut rec, 0xf8);
        parser.push_byte(&mut rec, 60);
        parser.push_byte(&mut rec, 100);
        assert_eq!(rec.events, ["clock", "on 0 60 100"]);
    }
}
