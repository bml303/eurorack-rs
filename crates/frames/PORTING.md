# Porting Frames

**Keyframer / mixer**  |  MCU family: `stm32f1`  |  ~2638 lines of hand-written C (excl. resources & drivers)

## Method

Follow the `braids` crate as the worked example:

1. `python tools/transpile_resources.py ../eurorack/frames/resources.cc \
       ../eurorack/frames/resources.h crates/frames/src/resources.rs`
2. Port `mi-stmlib` primitives this module needs (check its `#include`s) if not
   already present.
3. Translate the DSP files below. Preserve fixed-point arithmetic verbatim
   (use `wrapping_*`); modernise *structure* only -- modules, methods, enums,
   `match` instead of function-pointer tables.
4. Add `examples/render_wav.rs` mirroring `frames/test/*_test.cc` and diff the
   output WAV against the C test with `tools/wav_diff.py`.

## Source inventory (DSP + UI, drivers/bootloader/resources excluded)

| file | lines |
|------|-------|
| `frames.cc` | 189 |
| `keyframer.cc` | 260 |
| `keyframer.h` | 176 |
| `poly_lfo.cc` | 124 |
| `poly_lfo.h` | 99 |
| `ui.cc` | 433 |
| `ui.h` | 141 |
| `drivers/adc.cc` | 98 |
| `drivers/adc.h` | 57 |
| `drivers/channel_leds.cc` | 79 |
| `drivers/channel_leds.h` | 58 |
| `drivers/dac.cc` | 64 |
| `drivers/dac.h` | 69 |
| `drivers/factory_testing_switch.h` | 61 |
| `drivers/keyframe_led.cc` | 53 |
| `drivers/keyframe_led.h` | 51 |
| `drivers/rgb_led.cc` | 82 |
| `drivers/rgb_led.h` | 72 |
| `drivers/switches.cc` | 53 |
| `drivers/switches.h` | 71 |
| `drivers/system.cc` | 79 |
| `drivers/system.h` | 50 |
| `drivers/trigger_output.cc` | 55 |
| `drivers/trigger_output.h` | 52 |
| `drivers/uart_logger.cc` | 62 |
| `drivers/uart_logger.h` | 50 |

## Resources

`frames/resources.cc` (2697 lines of generated lookup tables) -> transpile with `tools/transpile_resources.py` into `src/resources.rs`, exactly as done for `braids`.

## Not in scope for the library crate

STM32/AVR peripheral drivers (`drivers/`), the audio bootloader, and the
`hardware_design/` files stay in the C repo -- the Rust crate is a `no_std`
DSP library that a host or an embedded HAL feeds.
