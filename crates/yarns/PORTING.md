# Porting Yarns

**Monophonic / polyphonic MIDI interface**  |  MCU family: `stm32f2`  |  ~10417 lines of hand-written C (excl. resources & drivers)

## Status: partially ported

Yarns is structurally different from every other crate in this workspace:
it's a MIDI sequencer/router, not an audio voice engine, and most of its
source is MIDI parsing, note allocation/arpeggiation, multi-part routing, a
song recorder, persisted settings and the front-panel UI -- not audio-rate
DSP. This increment ports only the self-contained, non-hardware-specific
audio-rate/timing/tuning pieces:

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

Fixed-point (STM32F2, Cortex-M3, no FPU), `mi-braids`/`mi-edges`-style
verbatim arithmetic: 32-bit phase accumulators, calibrated DAC code tables,
BLEP-antialiased oscillator. `mi-stmlib`'s existing `RingBuffer`,
`fixed::{interpolate_824_i16, interpolate_824_u16, interpolate_1022}` and
`random::Random` cover everything this increment needs.

### Deferred to a follow-up

None of what's ported here depends on any of the following, so they can be
added later as a separate increment:

- `part.{cc,h}` (758+421 lines) -- per-part note allocation, the
  arpeggiator, note LFOs.
- `multi.{cc,h}` (864+474 lines) -- multi-part MIDI routing, the top-level
  `Multi` class `yarns.cc`'s main loop drives.
- `midi_handler.{cc,h}` (271+291 lines) -- MIDI byte-stream parsing (running
  status, SysEx, etc.).
- `song/song.h` (2336 lines) -- the song recorder, by far the single
  largest remaining file in the module.
- `layout_configurator.{cc,h}` (153+107 lines) -- front-panel LED layout,
  UI-adjacent.

### Out of scope entirely

Per the top-level `CLAUDE.md` ("no peripheral drivers, no bootloader, no
`settings`/`ui`"): `settings.{cc,h}` (persisted flash state -- also the
source of the `AudioMode`/trigger-shape *display* numbering, see the
Deviations note below), `ui.{cc,h}`, `storage_manager.{cc,h}`, `drivers/`,
the bootloader, and `hardware_design/`.

### Verification

No C bit-compare harness exists yet (would need a `voice_compare.cc`
mirroring `Voice::Refresh`/`Oscillator::Render` against a fixed note/CC
script, dumped to WAV and diffed with `tools/wav_diff.py`, the same
approach as `mi-braids`/`mi-edges` -- left as future work). `tests/smoke.rs`
instead sweeps `Voice` across every audio mode, every trigger shape,
portamento, vibrato, pitch bend, aftertouch and note on/off over 3000
blocks (asserting no panics and a non-silent, varying output), and
`JustIntonationProcessor` across a 2000-note sweep (asserting the tuned
pitch never drifts outside the tuner's own +/- 1 quartertone search range).

### A real bug this port's fidelity check caught

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

## Source inventory (DSP + UI, drivers/bootloader/resources excluded)

| file | lines | ported? |
|------|-------|---------|
| `internal_clock.h` | 94 | yes -- `src/internal_clock.rs` |
| `just_intonation_processor.cc` | 76 | yes -- `src/just_intonation_processor.rs` |
| `just_intonation_processor.h` | 118 | yes |
| `layout_configurator.cc` | 153 | no (future work, UI-adjacent) |
| `layout_configurator.h` | 107 | no |
| `midi_handler.cc` | 271 | no (future work) |
| `midi_handler.h` | 291 | no |
| `multi.cc` | 864 | no (future work) |
| `multi.h` | 474 | no |
| `part.cc` | 758 | no (future work) |
| `part.h` | 421 | no |
| `settings.cc` | 1032 | no (out of scope) |
| `settings.h` | 194 | no |
| `storage_manager.cc` | 92 | no (out of scope) |
| `storage_manager.h` | 72 | no |
| `ui.cc` | 743 | no (out of scope) |
| `ui.h` | 254 | no |
| `voice.cc` | 411 | yes -- `src/voice.rs` + `src/oscillator.rs` |
| `voice.h` | 283 | yes |
| `yarns.cc` | 196 | no (app-level `main()`/ISR wiring; out of scope) |
| `song/song.h` | 2336 | no (future work) |
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
this fix).

## Also relevant in `mi-stmlib`

`Oscillator`'s audio buffer needs `stmlib::RingBuffer<T, const N: usize>`.
It's the same one added for `mi-marbles` (merged into `master` first); this
branch was rebased onto that merge, so there was nothing left to reconcile
by the time it landed. Everything else this increment needs
(`fixed::{interpolate_824_i16, interpolate_824_u16, interpolate_1022}`,
`random::Random`) was already there.

## Not in scope for the library crate

STM32/AVR peripheral drivers (`drivers/`), the audio bootloader, and the
`hardware_design/` files stay in the C repo -- the Rust crate is a `no_std`
DSP library that a host or an embedded HAL feeds.
