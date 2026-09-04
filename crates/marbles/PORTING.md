# Porting Marbles

**Random sampler / CV generator**  |  MCU family: `stm32f3`  |  ~8716 lines of hand-written C (excl. resources & drivers)

## Method

Follow the `braids` crate as the worked example:

1. `python tools/transpile_resources.py ../eurorack/marbles/resources.cc \
       ../eurorack/marbles/resources.h crates/marbles/src/resources.rs`
2. Port `mi-stmlib` primitives this module needs (check its `#include`s) if not
   already present.
3. Translate the DSP files below. Preserve fixed-point arithmetic verbatim
   (use `wrapping_*`); modernise *structure* only -- modules, methods, enums,
   `match` instead of function-pointer tables.
4. Add `examples/render_wav.rs` mirroring `marbles/test/*_test.cc` and diff the
   output WAV against the C test with `tools/wav_diff.py`.

## Source inventory (DSP + UI, drivers/bootloader/resources excluded)

| file | lines |
|------|-------|
| `clock_self_patching_detector.h` | 87 |
| `cv_reader.cc` | 117 |
| `cv_reader.h` | 133 |
| `cv_reader_channel.h` | 225 |
| `io_buffer.h` | 110 |
| `marbles.cc` | 469 |
| `note_filter.h` | 82 |
| `scale_recorder.h` | 149 |
| `settings.cc` | 249 |
| `settings.h` | 163 |
| `ui.cc` | 590 |
| `ui.h` | 148 |
| `ramp/ramp.h` | 40 |
| `ramp/ramp_divider.h` | 116 |
| `ramp/ramp_extractor.cc` | 333 |
| `ramp/ramp_extractor.h` | 148 |
| `ramp/ramp_generator.h` | 63 |
| `ramp/slave_ramp.h` | 137 |
| `test/fixtures.h` | 233 |
| `test/marbles_test.cc` | 1134 |
| `test/ramp_checker.h` | 84 |
| `random/discrete_distribution_quantizer.cc` | 115 |
| `random/discrete_distribution_quantizer.h` | 75 |
| `random/distributions.h` | 182 |
| `random/lag_processor.cc` | 88 |
| `random/lag_processor.h` | 61 |
| `random/output_channel.cc` | 145 |
| `random/output_channel.h` | 139 |
| `random/quantizer.cc` | 138 |
| `random/quantizer.h` | 122 |
| `random/random_generator.h` | 65 |
| `random/random_sequence.h` | 268 |
| `random/random_stream.h` | 82 |
| `random/t_generator.cc` | 429 |
| `random/t_generator.h` | 217 |
| `random/x_y_generator.cc` | 207 |
| `random/x_y_generator.h` | 146 |
| `drivers/adc.cc` | 162 |
| `drivers/adc.h` | 81 |
| `drivers/clock_inputs.cc` | 122 |
| `drivers/clock_inputs.h` | 75 |
| `drivers/dac.cc` | 160 |
| `drivers/dac.h` | 82 |
| `drivers/debug_pin.h` | 76 |
| `drivers/debug_port.h` | 94 |
| `drivers/gate_outputs.h` | 86 |
| `drivers/leds.cc` | 86 |
| `drivers/leds.h` | 75 |
| `drivers/rng.h` | 62 |
| `drivers/switches.cc` | 79 |
| `drivers/switches.h` | 89 |
| `drivers/system.cc` | 48 |
| `drivers/system.h` | 50 |

## Resources

`marbles/resources.cc` (4767 lines of generated lookup tables) -> transpile with `tools/transpile_resources.py` into `src/resources.rs`, exactly as done for `braids`.

## Not in scope for the library crate

STM32/AVR peripheral drivers (`drivers/`), the audio bootloader, and the
`hardware_design/` files stay in the C repo -- the Rust crate is a `no_std`
DSP library that a host or an embedded HAL feeds.
