# Porting Warps

**Meta-modulator (cross-modulation, vocoder)**  |  MCU family: `stm32f3`  |  ~5589 lines of hand-written C (excl. resources & drivers)

## Method

Follow the `braids` crate as the worked example:

1. `python tools/transpile_resources.py ../eurorack/warps/resources.cc \
       ../eurorack/warps/resources.h crates/warps/src/resources.rs`
2. Port `mi-stmlib` primitives this module needs (check its `#include`s) if not
   already present.
3. Translate the DSP files below. Preserve fixed-point arithmetic verbatim
   (use `wrapping_*`); modernise *structure* only -- modules, methods, enums,
   `match` instead of function-pointer tables.
4. Add `examples/render_wav.rs` mirroring `warps/test/*_test.cc` and diff the
   output WAV against the C test with `tools/wav_diff.py`.

## Source inventory (DSP + UI, drivers/bootloader/resources excluded)

| file | lines |
|------|-------|
| `cv_scaler.cc` | 178 |
| `cv_scaler.h` | 193 |
| `meter.h` | 84 |
| `settings.cc` | 69 |
| `settings.h` | 91 |
| `ui.cc` | 381 |
| `ui.h` | 116 |
| `warps.cc` | 119 |
| `dsp/filter_bank.cc` | 157 |
| `dsp/filter_bank.h` | 116 |
| `dsp/limiter.h` | 70 |
| `dsp/modulator.cc` | 448 |
| `dsp/modulator.h` | 238 |
| `dsp/oscillator.cc` | 217 |
| `dsp/oscillator.h` | 106 |
| `dsp/parameters.h` | 86 |
| `dsp/quadrature_oscillator.h` | 116 |
| `dsp/quadrature_transform.h` | 119 |
| `dsp/sample_rate_conversion_filters.h` | 136 |
| `dsp/sample_rate_converter.h` | 286 |
| `dsp/vocoder.cc` | 130 |
| `dsp/vocoder.h` | 139 |
| `test/warps_test.cc` | 381 |
| `drivers/adc.cc` | 113 |
| `drivers/adc.h` | 70 |
| `drivers/codec.cc` | 562 |
| `drivers/codec.h` | 102 |
| `drivers/debug_pin.h` | 76 |
| `drivers/debug_port.cc` | 64 |
| `drivers/debug_port.h` | 68 |
| `drivers/leds.cc` | 117 |
| `drivers/leds.h` | 72 |
| `drivers/normalization_probe.h` | 77 |
| `drivers/switches.cc` | 59 |
| `drivers/switches.h` | 76 |
| `drivers/system.cc` | 43 |
| `drivers/system.h` | 52 |
| `drivers/version.h` | 62 |

## Resources

`warps/resources.cc` (3475 lines of generated lookup tables) -> transpile with `tools/transpile_resources.py` into `src/resources.rs`, exactly as done for `braids`.

## Not in scope for the library crate

STM32/AVR peripheral drivers (`drivers/`), the audio bootloader, and the
`hardware_design/` files stay in the C repo -- the Rust crate is a `no_std`
DSP library that a host or an embedded HAL feeds.
