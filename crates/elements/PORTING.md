# Porting Elements

**Modal / physical-modelling synthesizer**  |  MCU family: `stm32f4`  |  ~7043 lines of hand-written C (excl. resources & drivers)

## Method

Follow the `braids` crate as the worked example:

1. `python tools/transpile_resources.py ../eurorack/elements/resources.cc \
       ../eurorack/elements/resources.h crates/elements/src/resources.rs`
2. Port `mi-stmlib` primitives this module needs (check its `#include`s) if not
   already present.
3. Translate the DSP files below. Preserve fixed-point arithmetic verbatim
   (use `wrapping_*`); modernise *structure* only -- modules, methods, enums,
   `match` instead of function-pointer tables.
4. Add `examples/render_wav.rs` mirroring `elements/test/*_test.cc` and diff the
   output WAV against the C test with `tools/wav_diff.py`.

## Source inventory (DSP + UI, drivers/bootloader/resources excluded)

| file | lines |
|------|-------|
| `cv_scaler.cc` | 220 |
| `cv_scaler.h` | 181 |
| `elements.cc` | 147 |
| `meter.h` | 84 |
| `ui.cc` | 260 |
| `ui.h` | 100 |
| `dsp/dsp.h` | 43 |
| `dsp/exciter.cc` | 289 |
| `dsp/exciter.h` | 133 |
| `dsp/multistage_envelope.cc` | 47 |
| `dsp/multistage_envelope.h` | 359 |
| `dsp/ominous_voice.cc` | 321 |
| `dsp/ominous_voice.h` | 264 |
| `dsp/part.cc` | 253 |
| `dsp/part.h` | 118 |
| `dsp/patch.h` | 60 |
| `dsp/resonator.cc` | 188 |
| `dsp/resonator.h` | 134 |
| `dsp/string.cc` | 219 |
| `dsp/string.h` | 170 |
| `dsp/tube.cc` | 89 |
| `dsp/tube.h` | 69 |
| `dsp/voice.cc` | 270 |
| `dsp/voice.h` | 131 |
| `dsp/fx/diffuser.h` | 83 |
| `dsp/fx/fx_engine.h` | 297 |
| `dsp/fx/reverb.h` | 181 |
| `test/elements_test.cc` | 501 |
| `drivers/codec.cc` | 483 |
| `drivers/codec.h` | 191 |
| `drivers/cv_adc.cc` | 127 |
| `drivers/cv_adc.h` | 76 |
| `drivers/debug_pin.h` | 76 |
| `drivers/debug_port.cc` | 64 |
| `drivers/debug_port.h` | 68 |
| `drivers/gate_input.cc` | 55 |
| `drivers/gate_input.h` | 50 |
| `drivers/leds.cc` | 103 |
| `drivers/leds.h` | 69 |
| `drivers/pots_adc.cc` | 155 |
| `drivers/pots_adc.h` | 94 |
| `drivers/switch.cc` | 56 |
| `drivers/switch.h` | 70 |
| `drivers/system.cc` | 45 |
| `drivers/system.h` | 50 |

## Resources

`elements/resources.cc` (44621 lines of generated lookup tables) -> transpile with `tools/transpile_resources.py` into `src/resources.rs`, exactly as done for `braids`.

## Not in scope for the library crate

STM32/AVR peripheral drivers (`drivers/`), the audio bootloader, and the
`hardware_design/` files stay in the C repo -- the Rust crate is a `no_std`
DSP library that a host or an embedded HAL feeds.
