# Porting Stages

**Segment generator**  |  MCU family: `stm32f3`  |  ~6423 lines of hand-written C (excl. resources & drivers)

## Method

Follow the `braids` crate as the worked example:

1. `python tools/transpile_resources.py ../eurorack/stages/resources.cc \
       ../eurorack/stages/resources.h crates/stages/src/resources.rs`
2. Port `mi-stmlib` primitives this module needs (check its `#include`s) if not
   already present.
3. Translate the DSP files below. Preserve fixed-point arithmetic verbatim
   (use `wrapping_*`); modernise *structure* only -- modules, methods, enums,
   `match` instead of function-pointer tables.
4. Add `examples/render_wav.rs` mirroring `stages/test/*_test.cc` and diff the
   output WAV against the C test with `tools/wav_diff.py`.

## Source inventory (DSP + UI, drivers/bootloader/resources excluded)

| file | lines |
|------|-------|
| `chain_state.cc` | 603 |
| `chain_state.h` | 270 |
| `cv_reader.cc` | 87 |
| `cv_reader.h` | 81 |
| `delay_line_16_bits.h` | 85 |
| `factory_test.cc` | 205 |
| `factory_test.h` | 105 |
| `io_buffer.h` | 105 |
| `oscillator.h` | 283 |
| `segment_generator.cc` | 901 |
| `segment_generator.h` | 249 |
| `settings.cc` | 93 |
| `settings.h` | 111 |
| `stages.cc` | 270 |
| `ui.cc` | 226 |
| `ui.h` | 96 |
| `variable_shape_oscillator.h` | 160 |
| `test/fixtures.h` | 155 |
| `test/stages_test.cc` | 210 |
| `drivers/cv_adc.cc` | 192 |
| `drivers/cv_adc.h` | 64 |
| `drivers/dac.cc` | 173 |
| `drivers/dac.h` | 79 |
| `drivers/firmware_update_adc.cc` | 122 |
| `drivers/firmware_update_adc.h` | 62 |
| `drivers/firmware_update_dac.cc` | 114 |
| `drivers/firmware_update_dac.h` | 90 |
| `drivers/gate_inputs.cc` | 125 |
| `drivers/gate_inputs.h` | 70 |
| `drivers/leds.cc` | 141 |
| `drivers/leds.h` | 75 |
| `drivers/pots_adc.cc` | 181 |
| `drivers/pots_adc.h` | 85 |
| `drivers/serial_link.cc` | 215 |
| `drivers/serial_link.h` | 89 |
| `drivers/switches.cc` | 77 |
| `drivers/switches.h` | 78 |
| `drivers/system.cc` | 46 |
| `drivers/system.h` | 50 |

## Resources

`stages/resources.cc` (1525 lines of generated lookup tables) -> transpile with `tools/transpile_resources.py` into `src/resources.rs`, exactly as done for `braids`.

## Not in scope for the library crate

STM32/AVR peripheral drivers (`drivers/`), the audio bootloader, and the
`hardware_design/` files stay in the C repo -- the Rust crate is a `no_std`
DSP library that a host or an embedded HAL feeds.
