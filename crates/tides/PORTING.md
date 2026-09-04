# Porting Tides

**Tidal modulator (2014)**  |  MCU family: `stm32f1`  |  ~3532 lines of hand-written C (excl. resources & drivers)

## Method

Follow the `braids` crate as the worked example:

1. `python tools/transpile_resources.py ../eurorack/tides/resources.cc \
       ../eurorack/tides/resources.h crates/tides/src/resources.rs`
2. Port `mi-stmlib` primitives this module needs (check its `#include`s) if not
   already present.
3. Translate the DSP files below. Preserve fixed-point arithmetic verbatim
   (use `wrapping_*`); modernise *structure* only -- modules, methods, enums,
   `match` instead of function-pointer tables.
4. Add `examples/render_wav.rs` mirroring `tides/test/*_test.cc` and diff the
   output WAV against the C test with `tools/wav_diff.py`.

## Source inventory (DSP + UI, drivers/bootloader/resources excluded)

| file | lines |
|------|-------|
| `cv_scaler.cc` | 70 |
| `cv_scaler.h` | 159 |
| `generator.cc` | 778 |
| `generator.h` | 282 |
| `plotter.cc` | 67 |
| `plotter.h` | 84 |
| `tides.cc` | 167 |
| `ui.cc` | 305 |
| `ui.h` | 118 |
| `easter_egg/plotter_program.h` | 205 |
| `test/generator_test.cc` | 145 |
| `drivers/adc.cc` | 105 |
| `drivers/adc.h` | 56 |
| `drivers/dac.cc` | 64 |
| `drivers/dac.h` | 93 |
| `drivers/factory_testing_switch.h` | 61 |
| `drivers/gate_input.cc` | 44 |
| `drivers/gate_input.h` | 81 |
| `drivers/gate_output.cc` | 43 |
| `drivers/gate_output.h` | 65 |
| `drivers/leds.cc` | 86 |
| `drivers/leds.h` | 85 |
| `drivers/switches.cc` | 53 |
| `drivers/switches.h` | 72 |
| `drivers/system.cc` | 82 |
| `drivers/system.h` | 50 |
| `drivers/uart_logger.cc` | 62 |
| `drivers/uart_logger.h` | 50 |

## Resources

`tides/resources.cc` (18771 lines of generated lookup tables) -> transpile with `tools/transpile_resources.py` into `src/resources.rs`, exactly as done for `braids`.

## Not in scope for the library crate

STM32/AVR peripheral drivers (`drivers/`), the audio bootloader, and the
`hardware_design/` files stay in the C repo -- the Rust crate is a `no_std`
DSP library that a host or an embedded HAL feeds.
