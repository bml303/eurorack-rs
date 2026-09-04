# Porting Edges

**Quad chiptune digital oscillator**  |  MCU family: `avr`  |  ~2795 lines of hand-written C (excl. resources & drivers)

## Method

Follow the `braids` crate as the worked example:

1. `python tools/transpile_resources.py ../eurorack/edges/resources.cc \
       ../eurorack/edges/resources.h crates/edges/src/resources.rs`
2. Port `mi-stmlib` primitives this module needs (check its `#include`s) if not
   already present.
3. Translate the DSP files below. Preserve fixed-point arithmetic verbatim
   (use `wrapping_*`); modernise *structure* only -- modules, methods, enums,
   `match` instead of function-pointer tables.
4. Add `examples/render_wav.rs` mirroring `edges/test/*_test.cc` and diff the
   output WAV against the C test with `tools/wav_diff.py`.

## Source inventory (DSP + UI, drivers/bootloader/resources excluded)

| file | lines |
|------|-------|
| `adc_acquisition.cc` | 42 |
| `adc_acquisition.h` | 82 |
| `audio_buffer.cc` | 27 |
| `audio_buffer.h` | 41 |
| `digital_oscillator.cc` | 262 |
| `digital_oscillator.h` | 95 |
| `edges.cc` | 246 |
| `hardware_config.h` | 102 |
| `midi.h` | 291 |
| `midi_handler.cc` | 45 |
| `midi_handler.h` | 248 |
| `note_stack.h` | 178 |
| `settings.cc` | 82 |
| `settings.h` | 262 |
| `storage.h` | 95 |
| `timer_oscillator.cc` | 70 |
| `timer_oscillator.h` | 111 |
| `ui.cc` | 257 |
| `ui.h` | 104 |
| `voice_allocator.cc` | 99 |
| `voice_allocator.h` | 56 |

## Resources

`edges/resources.cc` (1005 lines of generated lookup tables) -> transpile with `tools/transpile_resources.py` into `src/resources.rs`, exactly as done for `braids`.

## Not in scope for the library crate

STM32/AVR peripheral drivers (`drivers/`), the audio bootloader, and the
`hardware_design/` files stay in the C repo -- the Rust crate is a `no_std`
DSP library that a host or an embedded HAL feeds.
