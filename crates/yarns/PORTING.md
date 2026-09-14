# Porting Yarns

**Monophonic / polyphonic MIDI interface**  |  MCU family: `stm32f2`  |  ~10417 lines of hand-written C (excl. resources & drivers)

## Status: ported (except everything out of scope)

Yarns is structurally different from every other crate in this workspace:
it's a MIDI sequencer/router, not an audio voice engine, and a good chunk of
its source is persisted settings and the front-panel UI -- not audio-rate
DSP. Ported across three increments:

- `voice.{cc,h}` -> `src/voice.rs` + `src/oscillator.rs` (`Voice`,
  `Oscillator`) -- pitch with portamento, pitch-bend, vibrato and a
  clock-synced-LFO PLL, gate/trigger envelope (6 trigger shapes), calibrated
  DAC code lookup, and a 5-waveform BLEP oscillator for the "audio" output
  modes.
- `just_intonation_processor.{cc,h}` -> `src/just_intonation_processor.rs`
  (`JustIntonationProcessor`) -- the note-history-weighted just-intonation
  tuner.
- `internal_clock.h` -> `src/internal_clock.rs` (`InternalClock`) -- the
  internal MIDI clock with swing.
- `part.{cc,h}` -> `src/part.rs` (`Part`) -- per-channel note allocation (9
  voice-allocation modes: mono, 5 poly-note-stealing variants, sorted,
  2 unison), the arpeggiator (6 directions, euclidean patterns), and the
  64-step sequencer.
- `multi.{cc,h}` -> `src/multi.rs` (`Multi`) -- routes MIDI to up to 4
  `Part`s across 11 `Layout`s, the shared master clock (internal or MIDI,
  with swing), CV/gate/audio-source derivation for the 4 output jacks, and
  the built-in demo song.
- `multi.cc`'s `song[] = { #include "song/song.h" }` -> `src/song.rs`
  (`SONG`) -- turns out `song/song.h` (2336 lines, initially assumed to be a
  large logic file and scoped as "future work" in this crate's first
  increment) is pure numeric data, one byte per line -- trivial to
  transpile once actually opened. `Multi::clock_song`, the ~20 lines of
  *logic* that walks it, is ported in `multi.rs`.
- `stmlib/midi/midi.h`'s `MidiStreamParser<Handler>` -> `mi-stmlib`'s
  `src/midi.rs` (`MidiStreamParser`, `MidiEventHandler`) -- this is shared
  library code, not Yarns-specific in the C++ (it lives in `stmlib/`, not
  `yarns/`), so it's ported into `mi-stmlib` rather than this crate: a
  from-scratch MIDI byte-stream decoder (running status, realtime messages
  interleaved with data bytes, SysEx framing).
- `midi_handler.{cc,h}`'s dispatch functions (`MidiHandler::NoteOn`/
  `ControlChange`/`PitchBend`/etc., which forward into `Multi` and
  optionally echo to a MIDI-out buffer) -> `src/midi_dispatch.rs`
  (`MidiDispatch`), implementing `stmlib::MidiEventHandler` on top of
  `Multi` + `&mut dyn MidiOut`. Pairing `MidiStreamParser` with
  `MidiDispatch` gives a host the C++'s full pipeline: raw MIDI bytes in,
  `Multi` calls out.

Fixed-point (STM32F2, Cortex-M3, no FPU), `mi-braids`/`mi-edges`-style
verbatim arithmetic: 32-bit phase accumulators, calibrated DAC code tables,
BLEP-antialiased oscillator. `mi-stmlib` gained `NoteStack<N>`,
`VoiceAllocator<N>` (`stmlib/algorithms/{note_stack,voice_allocator}.h`,
needed by `Part`) and `MidiStreamParser`/`MidiEventHandler`
(`stmlib/midi/midi.h`) -- none previously ported -- alongside the
`RingBuffer`, `fixed::*` and `random::Random` the first increment already
used.

### What's still not ported, from `midi_handler.{cc,h}`

The SysEx-specific pieces (scale/octave tuning dumps, the Yarns-specific
packet protocol for save/restore and calibration) are tied to
`storage_manager`/calibration workflows that are out of scope, so
`MidiDispatch` doesn't override `sysex_start`/`sysex_byte`/`sysex_end`
(defaulted to no-ops by `stmlib::MidiEventHandler`). `MidiHandler`'s raw
input byte queue (`PushByte`/`ProcessInput`, a `RingBuffer<u8, 128>` an ISR
fills and the main loop drains into the parser) is host-side plumbing with
no DSP content -- a host can call `MidiStreamParser::push_byte` directly as
bytes arrive.

### Out of scope entirely

Per the top-level `CLAUDE.md` ("no peripheral drivers, no bootloader, no
`settings`/`ui`"): `settings.{cc,h}` (persisted flash state -- also the
source of the `AudioMode`/trigger-shape *display* numbering, see the
Deviations note below), `ui.{cc,h}`, `storage_manager.{cc,h}`,
`layout_configurator.{cc,h}` (the front-panel MIDI-channel-learn feature),
`drivers/`, the bootloader, and `hardware_design/`. See `part.rs`/
`multi.rs`'s module doc comments for the exact methods/fields this cost on
`Part`/`Multi` (`Set`/`Get`'s byte-addressed settings API,
`Serialize`/`Deserialize`, `GetLedsBrightness`, `HandleRemoteControlCC`,
`paques()`) and what replaced them.

### Verification

No C bit-compare harness exists yet (would need `voice_compare.cc`/
`part_compare.cc`/`multi_compare.cc` mirroring a fixed MIDI script against
`Voice::Refresh`/`Part`/`Multi`, dumped and diffed with `tools/wav_diff.py`,
the same approach as `mi-braids`/`mi-edges` -- left as future work).
`tests/smoke.rs` instead:
- sweeps `Voice` across every audio mode, every trigger shape, portamento,
  vibrato, pitch bend, aftertouch and note on/off over 3000 blocks;
- sweeps `JustIntonationProcessor` across a 2000-note sequence, asserting
  the tuned pitch never drifts outside the tuner's own +/- 1 quartertone
  search range;
- sweeps `Multi` across every `Layout`, every `VoiceAllocationMode`, several
  arpeggiator directions, note on/off/CC/pitch-bend/aftertouch, and
  `clock`/`refresh`/`render_audio`/`get_cv_gate`/`get_audio_source` over
  6000 blocks, then confirms a plain note-on in Mono layout gates a voice;
- plays the built-in demo song through past its own length (so it wraps at
  least once) and confirms it produces non-silent audio;
- drives `Multi` through `MidiStreamParser` + `MidiDispatch` with a raw
  byte stream exercising running status, a realtime clock byte interleaved
  mid-message, and note-on-with-zero-velocity-as-note-off, confirming the
  note on/off bytes actually gate a voice and the interleaved clock byte
  reaches `MidiOut`;
- feeds a batch of malformed/edge-case bytes (stray data bytes with no
  status, an unmatched SysEx terminator, truncated multi-byte messages,
  every realtime byte back to back) through the same pipeline, asserting
  only that nothing panics.

All these assert no panics; most also assert the specific behavioural
properties described above.

### A real bug this port's fidelity check caught (from the first increment)

`Oscillator`'s `AudioMode` byte and `Voice`'s `TriggerShape` byte look like
they should both be direct instances of the enums `voice.h` declares
(`enum AudioMode { AUDIO_MODE_OFF, _SAW, _SQUARE, _TRIANGLE, _SINE }` and
`enum TriggerShape { ..6 values.. }`), but only one of them actually is.
`Voice::set_trigger_shape`/`trigger_dac_code` in `voice.cc` use
`TriggerShape`'s declared values directly (`TRIGGER_SHAPE_SQUARE == 0`,
etc.) -- reproduced verbatim in `voice::trigger_shape`. `Oscillator::Render`'s
dispatch (`switch ((mode & 0x0f) - 1)`) does **not** match `AudioMode`'s
declared values at all: `AUDIO_MODE_SINE == 4` would hit the *triangle*
branch (`case 3: RenderSquare(..., true)`, the leaky-integrated one), not
sine. `Voice::set_audio_mode`'s own parameter is already typed `uint8_t`,
not `AudioMode`, in the C++ -- the tell that a translation table in
`settings.cc` (out of scope here) maps the enum's UI-facing ordering to the
different byte `Render` actually switches on. `oscillator::audio_mode`'s
consts are numbered to match `Render`'s real dispatch (`SAW=1,
SQUARE_NARROW=2, SQUARE=3, TRIANGLE=4, SINE=5`), not the C enum's declared
values -- reproducing the latter as if they were dispatch codes would have
been a working-looking but silently-wrong API for any host calling
`oscillator.render(audio_mode::SINE, ...)`.

### Other deviations from the C++ (beyond "modernise structure only")

From the first increment (`voice.rs`):
- **`Voice::note_on`'s portamento-rate calculation keeps the C's 32-bit
  unsigned wraparound.** `1536 * (base_increment >> 11) / delta` is 32-bit
  `uint32_t` arithmetic in the C (both operands promote to `unsigned int`),
  including whatever happens if the multiply overflows -- ported as
  `1536u32.wrapping_mul(base_increment >> 11) / delta` rather than widening
  to `u64` first (which would silently avoid the overflow and produce a
  different, "more correct" but unfaithful result for large `base_increment`).
- **`Voice::dac_code_from_16_bit_value`'s `scale` is computed via `i32`
  subtraction then reinterpreted as `u32`**, matching the C's
  `calibrated_dac_code_[3] - calibrated_dac_code_[8]` (both `uint16_t`
  operands promote to `int` for the subtraction, and the possibly-negative
  `int` result is then assigned to a `uint32_t` -- a 2's-complement
  reinterpretation). A 16-bit `wrapping_sub` between the two `u16`s
  zero-extended to `u32` would give a different value whenever
  `calibrated_dac_code_[8] > calibrated_dac_code_[3]`.
- **`Voice::Refresh`'s "did this phase accumulator wrap?" checks** (both
  `portamento_phase`/`portamento_phase_increment` and
  `trigger_phase`/`trigger_phase_increment`) use the classic unsigned
  overflow idiom, `phase < increment` after a wrapping add, exactly as the
  C does -- *not* a separate `overflowing_add` flag ANDed with that same
  check (the two are equivalent, so ANDing them together makes the
  "wrapped" branch unreachable; caught by hand while re-reading the code
  before running it, since the mistake doesn't produce a compiler warning).

From this increment (`part.rs`/`multi.rs`):
- **`Part` doesn't hold `Voice*` pointers into `Multi`'s voice array** (a
  classic C aliasing pattern with no safe Rust equivalent). Instead it
  stores which indices of a *caller-supplied* voice array are its own
  (`voice_indices`, set by `Part::allocate_voices`), and every method that
  touches a voice takes `voices: &mut [Voice]` (the whole array `Multi`
  owns) as an explicit parameter.
- **`just_intonation_processor` and `Multi::settings_.custom_pitch_table`**
  (a single tuner object and a 12-entry array `Multi` owns and every `Part`
  shares a raw pointer to, in the C++) are bundled into a `TuningContext`
  built once per call and passed to whichever `Part`/`Multi` method needs
  it -- the same shared-state-as-parameter pattern `mi-marbles` uses for its
  `RandomStream`.
- **`midi_handler`'s `OnClock`/`OnStart`/`OnStop`/`OnInternalNoteOn`/
  `OnInternalNoteOff` echoes go through a `&mut dyn MidiOut` parameter**
  (`midi_out.rs`) instead of a global `MidiHandler`, threaded down from
  whichever `Part`/`Multi` method triggers them. A host that doesn't care
  about generated-event MIDI thru/polychaining can pass `NullMidiOut`.
- **The byte-addressed `Set(address, value)`/`Get(address)` settings API**
  (and the `PartSetting`/`MultiSetting`/`PART_MIDI_LAST` enums computing
  offsets via `sizeof(...)`) has no sound Rust equivalent without `unsafe`
  pointer-cast reinterpretation of a `#[repr(C)]` struct as a byte array,
  and its only callers in the C++ are the out-of-scope `settings.cc`/
  `ui.cc` menu system. Replaced with whole-struct setters
  (`Part::set_midi_settings`/`set_voicing_settings`/`set_sequencer_settings`,
  `Multi::set_layout`/`set_tempo`/`set_swing`,
  `Multi::set_part_midi_settings`/`set_part_voicing_settings`) that
  reproduce the specific side-effecting field transitions `Set` handled
  (shutting all notes off on a MIDI-filter change, `touch_voice_allocation`
  vs `touch_voices` on a voicing change depending on whether
  `allocation_mode` itself changed, recomputing the derived `arp_direction`
  sign on an arp-direction change, `change_layout` on a layout change) at
  whole-struct-replace granularity instead of per-byte.
- **`Part::control_change`'s hold-pedal-release branch doesn't call
  `release_latched_notes` itself.** The C++'s `ReleaseLatchedNotes` there
  needs a `TuningContext`/`MidiOut` that `ControlChange`'s own signature
  doesn't carry (releasing a latched note recurses into `NoteOff`, which
  can retune and forward polychained notes). Split into
  `control_change` (matching the C's signature/return value exactly) plus
  `release_latched_notes_on_hold_release`, which the host calls right after
  `control_change(..., 0x40, value)` with `value < 64`.
- **`Multi::change_layout`/`update_layout` don't call the C++'s full
  `Part::Reset()`/`Multi::Stop()` before switching**, since those need a
  `MidiOut` this internal helper doesn't have; they silence voices directly
  (`Voice::note_off`) instead, matching the *audible* effect. A host driving
  the full `Reset()`/`Stop()` echo behavior around a layout change should
  call those itself first.
- **`Multi::Clock()`/`clock() const` and `Multi::Reset()`/`reset() const`**
  are distinguished only by case in the C++, which Rust identifiers can't
  do. Kept `clock`/`reset` for the two boolean CV-gate getters (matching
  the C's own naming, since those are what `get_cv_gate` and a host mirror
  most directly) and renamed the two mutating actions to `clock` (kept --
  it's the far more frequently called of the two clashing pairs) and
  `reset_all` (renamed from `reset`).

From this increment (`stmlib::midi`/`midi_dispatch.rs`):
- **`MidiStreamParser<Handler>`'s C++ template calls *static* methods on
  `Handler`** (compile-time "static polymorphism" -- exactly one `Handler`
  type per parser instantiation, so no vtable needed). Ported as a trait,
  `MidiEventHandler`, with every method defaulted to a no-op
  (`check_channel` to `true`, matching the only C++ handler in this
  workspace, `yarns::MidiHandler::CheckChannel`), and `MidiStreamParser`
  itself holds no handler -- `push_byte` takes `&mut impl MidiEventHandler`
  explicitly, the same "pass the shared/callback state as a parameter"
  pattern used throughout this crate and `mi-marbles`.
- **`MidiOut`'s methods are now all defaulted to no-ops** (previously,
  before this increment, `internal_note_on`/`internal_note_off`/
  `clock_tick`/`start`/`stop` were required). `MidiDispatch` needs several
  more MidiOut methods than `Part`/`Multi` alone did (aftertouch/CC/
  pitch-bend/program-change echo, plus `raw_byte_thru` for
  `MidiHandler::RawByte`'s MIDI-thru forwarding when `direct_thru()` is
  true) -- defaulting the whole trait keeps a host that only cares about a
  few event kinds (say, just note on/off for polychaining) from having to
  stub out methods it doesn't use.
- **`MidiHandler::NoteOn`/`NoteOff` and `Part`'s internal note generation
  send the identical wire bytes** (`0x90|channel, note, velocity` /
  `0x80|channel, note, 0`) for two different reasons (selective thru of a
  directly-played note vs. an arpeggiator/sequencer/polychained note) --
  kept as separate `MidiOut` methods (`note_on`/`note_off` vs.
  `internal_note_on`/`internal_note_off`) since a host may want to tell the
  two apart (e.g. to avoid double-echoing a note it just received
  verbatim), even though their default no-op bodies mean a host that
  doesn't care can implement just one pair, or neither.

## Source inventory (DSP + UI, drivers/bootloader/resources excluded)

| file | lines | ported? |
|------|-------|---------|
| `internal_clock.h` | 94 | yes -- `src/internal_clock.rs` |
| `just_intonation_processor.cc` | 76 | yes -- `src/just_intonation_processor.rs` |
| `just_intonation_processor.h` | 118 | yes |
| `layout_configurator.cc` | 153 | no (out of scope, UI) |
| `layout_configurator.h` | 107 | no |
| `midi_handler.cc` | 271 | partial -- dispatch logic in `src/midi_dispatch.rs`; SysEx handling out of scope (see above) |
| `midi_handler.h` | 291 | partial |
| `stmlib/midi/midi.h` | 222 | yes -- `mi-stmlib`'s `src/midi.rs` |
| `multi.cc` | 864 | yes -- `src/multi.rs` (minus the out-of-scope pieces noted above) |
| `multi.h` | 474 | yes |
| `part.cc` | 758 | yes -- `src/part.rs` (minus the out-of-scope pieces noted above) |
| `part.h` | 421 | yes |
| `settings.cc` | 1032 | no (out of scope) |
| `settings.h` | 194 | no |
| `storage_manager.cc` | 92 | no (out of scope) |
| `storage_manager.h` | 72 | no |
| `ui.cc` | 743 | no (out of scope) |
| `ui.h` | 254 | no |
| `voice.cc` | 411 | yes -- `src/voice.rs` + `src/oscillator.rs` |
| `voice.h` | 283 | yes |
| `yarns.cc` | 196 | no (app-level `main()`/ISR wiring; out of scope) |
| `song/song.h` | 2336 | yes -- `src/song.rs` (data only; see `Multi::clock_song` for the logic) |
| `drivers/*` | ~1122 | no (out of scope) |

## Resources

`yarns/resources.cc` (1666 lines of generated lookup tables) transpiled with
`tools/transpile_resources.py` into `src/resources.rs`. This module's
`lut_euclidean` table needed one fix to the transpiler itself: several of
its `uint32_t` entries are written with an explicit `U` suffix in the C
(`2913840557U`, etc.) to fit values above `INT32_MAX`, and
`to_rust_literal`'s suffix-stripping (`token.rstrip("Lu")`) didn't strip an
uppercase `U`. No other already-ported crate's resources happened to need
that suffix, so this had never been hit before; fixed to
`token.rstrip("LlUu")` and verified byte-identical regeneration for every
other crate's `resources.rs` that doesn't need hand-editing after
generation (`braids`, `clouds`, `rings`, `marbles`, `tides`, `tides2`;
`plaits`/`elements`/`edges` all have a documented hand-added
constant/preprocessing step on top of the generator's output, unrelated to
this fix). `song/song.h` (see above) was transpiled with a one-off script
rather than the general tool, since it's a bare comma-separated byte list
with no C declaration wrapper around it -- verified byte-for-byte identical
to the source by parsing both independently and comparing token lists.

## Also relevant in `mi-stmlib`

- `Oscillator`'s audio buffer needs `stmlib::RingBuffer<T, const N: usize>`
  (added for `mi-marbles`).
- `Part`'s note allocation needs two `stmlib/algorithms/*` classes not
  previously ported by any crate: `NoteStack<N>` (`note_stack.rs` --
  `pressed_keys`/`generated_notes`/`mono_allocator`; the C template's
  `capacity + 1`-sized pool array becomes `NoteStack<capacity + 1>`,
  matching `PatternPredictor`'s existing "pre-add the padding" trick since
  Rust const generics can't express that arithmetic on stable) and
  `VoiceAllocator<N>` (`voice_allocator.rs` -- `poly_allocator`, no padding
  trick needed, its arrays are exactly `N`-sized). Both have unit tests
  covering the tricky bits (`NoteStack`'s LIFO monosynth behaviour and
  saturation eviction; `VoiceAllocator`'s retriggering and voice-stealing).
- `MidiDispatch` needs `stmlib::midi::{MidiStreamParser, MidiEventHandler}`
  (`midi.rs` -- see "Ported" above; not previously ported by any crate).
- `fixed::{interpolate_824_i16, interpolate_824_u16, interpolate_1022}` and
  `random::Random` (added for the first increment) cover everything else.

## Not in scope for the library crate

STM32/AVR peripheral drivers (`drivers/`), the audio bootloader, and the
`hardware_design/` files stay in the C repo -- the Rust crate is a `no_std`
DSP library that a host or an embedded HAL feeds.
