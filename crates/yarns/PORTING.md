# Porting Yarns

**Monophonic / polyphonic MIDI interface**  |  MCU family: `stm32f2`  |  ~10417 lines of hand-written C (excl. resources & drivers)

## Method

Follow the `braids` crate as the worked example:

1. `python tools/transpile_resources.py ../eurorack/yarns/resources.cc \
       ../eurorack/yarns/resources.h crates/yarns/src/resources.rs`
2. Port `mi-stmlib` primitives this module needs (check its `#include`s) if not
   already present.
3. Translate the DSP files below. Preserve fixed-point arithmetic verbatim
   (use `wrapping_*`); modernise *structure* only -- modules, methods, enums,
   `match` instead of function-pointer tables.
4. Add `examples/render_wav.rs` mirroring `yarns/test/*_test.cc` and diff the
   output WAV against the C test with `tools/wav_diff.py`.

## Source inventory (DSP + UI, drivers/bootloader/resources excluded)

| file | lines |
|------|-------|
| `internal_clock.h` | 94 |
| `just_intonation_processor.cc` | 76 |
| `just_intonation_processor.h` | 118 |
| `layout_configurator.cc` | 153 |
| `layout_configurator.h` | 107 |
| `midi_handler.cc` | 271 |
| `midi_handler.h` | 291 |
| `multi.cc` | 864 |
| `multi.h` | 474 |
| `part.cc` | 758 |
| `part.h` | 421 |
| `settings.cc` | 1032 |
| `settings.h` | 194 |
| `storage_manager.cc` | 92 |
| `storage_manager.h` | 72 |
| `ui.cc` | 743 |
| `ui.h` | 254 |
| `voice.cc` | 411 |
| `voice.h` | 283 |
| `yarns.cc` | 196 |
| `song/song.h` | 2336 |
| `drivers/channel_leds.cc` | 64 |
| `drivers/channel_leds.h` | 64 |
| `drivers/dac.cc` | 72 |
| `drivers/dac.h` | 94 |
| `drivers/display.cc` | 182 |
| `drivers/display.h` | 90 |
| `drivers/encoder.cc` | 54 |
| `drivers/encoder.h` | 84 |
| `drivers/gate_output.cc` | 50 |
| `drivers/gate_output.h` | 50 |
| `drivers/midi_io.cc` | 62 |
| `drivers/midi_io.h` | 68 |
| `drivers/switches.cc` | 56 |
| `drivers/switches.h` | 66 |
| `drivers/system.cc` | 71 |
| `drivers/system.h` | 50 |

## Resources

`yarns/resources.cc` (1666 lines of generated lookup tables) -> transpile with `tools/transpile_resources.py` into `src/resources.rs`, exactly as done for `braids`.

## Not in scope for the library crate

STM32/AVR peripheral drivers (`drivers/`), the audio bootloader, and the
`hardware_design/` files stay in the C repo -- the Rust crate is a `no_std`
DSP library that a host or an embedded HAL feeds.
