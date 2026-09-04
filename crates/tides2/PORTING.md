# Porting Tides2

**Tidal modulator (2018)**  |  MCU family: `stm32f3`  |  ~5362 lines of hand-written C (excl. resources & drivers)

## Method

Follow the `braids` crate as the worked example:

1. `python tools/transpile_resources.py ../eurorack/tides2/resources.cc \
       ../eurorack/tides2/resources.h crates/tides2/src/resources.rs`
2. Port `mi-stmlib` primitives this module needs (check its `#include`s) if not
   already present.
3. Translate the DSP files below. Preserve fixed-point arithmetic verbatim
   (use `wrapping_*`); modernise *structure* only -- modules, methods, enums,
   `match` instead of function-pointer tables.
4. Add `examples/render_wav.rs` mirroring `tides2/test/*_test.cc` and diff the
   output WAV against the C test with `tools/wav_diff.py`.

## Source inventory (DSP + UI, drivers/bootloader/resources excluded)

| file | lines |
|------|-------|
| `cv_reader.cc` | 135 |
| `cv_reader.h` | 82 |
| `cv_reader_channel.h` | 96 |
| `factory_test.cc` | 201 |
| `factory_test.h` | 106 |
| `io_buffer.h` | 119 |
| `poly_slope_generator.cc` | 92 |
| `poly_slope_generator.h` | 428 |
| `ramp_generator.h` | 239 |
| `ramp_shaper.h` | 264 |
| `settings.cc` | 105 |
| `settings.h` | 121 |
| `tides.cc` | 280 |
| `ui.cc` | 217 |
| `ui.h` | 92 |
| `ramp/ramp_extractor.cc` | 286 |
| `ramp/ramp_extractor.h` | 115 |
| `ramp/ratio.h` | 43 |
| `test/fixtures.h` | 89 |
| `test/tides_test.cc` | 344 |
| `drivers/cv_adc.cc` | 189 |
| `drivers/cv_adc.h` | 72 |
| `drivers/dac.cc` | 141 |
| `drivers/dac.h` | 74 |
| `drivers/debug_pin.h` | 78 |
| `drivers/debug_port.cc` | 62 |
| `drivers/debug_port.h` | 67 |
| `drivers/firmware_update_adc.cc` | 122 |
| `drivers/firmware_update_adc.h` | 62 |
| `drivers/firmware_update_dac.cc` | 107 |
| `drivers/firmware_update_dac.h` | 91 |
| `drivers/gate_inputs.cc` | 130 |
| `drivers/gate_inputs.h` | 71 |
| `drivers/leds.cc` | 88 |
| `drivers/leds.h` | 71 |
| `drivers/pots_adc.cc` | 151 |
| `drivers/pots_adc.h` | 79 |
| `drivers/switches.cc` | 74 |
| `drivers/switches.h` | 83 |
| `drivers/system.cc` | 46 |
| `drivers/system.h` | 50 |

## Resources

`tides2/resources.cc` (3970 lines of generated lookup tables) -> transpile with `tools/transpile_resources.py` into `src/resources.rs`, exactly as done for `braids`.

## Not in scope for the library crate

STM32/AVR peripheral drivers (`drivers/`), the audio bootloader, and the
`hardware_design/` files stay in the C repo -- the Rust crate is a `no_std`
DSP library that a host or an embedded HAL feeds.
