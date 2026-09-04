# Porting Streams

**Dual dynamics gate (VCA/VCF + envelope follower)**  |  MCU family: `stm32f105`  |  ~4038 lines of hand-written C (excl. resources & drivers)

## Method

Follow the `braids` crate as the worked example:

1. `python tools/transpile_resources.py ../eurorack/streams/resources.cc \
       ../eurorack/streams/resources.h crates/streams/src/resources.rs`
2. Port `mi-stmlib` primitives this module needs (check its `#include`s) if not
   already present.
3. Translate the DSP files below. Preserve fixed-point arithmetic verbatim
   (use `wrapping_*`); modernise *structure* only -- modules, methods, enums,
   `match` instead of function-pointer tables.
4. Add `examples/render_wav.rs` mirroring `streams/test/*_test.cc` and diff the
   output WAV against the C test with `tools/wav_diff.py`.

## Source inventory (DSP + UI, drivers/bootloader/resources excluded)

| file | lines |
|------|-------|
| `audio_cv_meter.h` | 95 |
| `compressor.cc` | 158 |
| `compressor.h` | 133 |
| `cv_scaler.cc` | 77 |
| `cv_scaler.h` | 108 |
| `envelope.cc` | 128 |
| `envelope.h` | 246 |
| `filter_controller.h` | 84 |
| `follower.cc` | 137 |
| `follower.h` | 131 |
| `gain.h` | 56 |
| `lorenz_generator.cc` | 82 |
| `lorenz_generator.h` | 80 |
| `meta_parameters.h` | 66 |
| `processor.cc` | 70 |
| `processor.h` | 174 |
| `streams.cc` | 106 |
| `svf.cc` | 65 |
| `svf.h` | 76 |
| `ui.cc` | 495 |
| `ui.h` | 133 |
| `vactrol.cc` | 186 |
| `vactrol.h` | 139 |
| `drivers/adc.cc` | 179 |
| `drivers/adc.h` | 78 |
| `drivers/dac.cc` | 66 |
| `drivers/dac.h` | 67 |
| `drivers/leds.cc` | 105 |
| `drivers/leds.h` | 124 |
| `drivers/pwm.cc` | 74 |
| `drivers/pwm.h` | 64 |
| `drivers/switches.cc` | 58 |
| `drivers/switches.h` | 73 |
| `drivers/system.cc` | 75 |
| `drivers/system.h` | 50 |

## Resources

`streams/resources.cc` (1435 lines of generated lookup tables) -> transpile with `tools/transpile_resources.py` into `src/resources.rs`, exactly as done for `braids`.

## Not in scope for the library crate

STM32/AVR peripheral drivers (`drivers/`), the audio bootloader, and the
`hardware_design/` files stay in the C repo -- the Rust crate is a `no_std`
DSP library that a host or an embedded HAL feeds.
