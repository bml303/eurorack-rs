# Porting Rings

**Modal / sympathetic-string resonator**  |  MCU family: `stm32f3`  |  ~8042 lines of hand-written C (excl. resources & drivers)

## Method

Follow the `braids` crate as the worked example:

1. `python tools/transpile_resources.py ../eurorack/rings/resources.cc \
       ../eurorack/rings/resources.h crates/rings/src/resources.rs`
2. Port `mi-stmlib` primitives this module needs (check its `#include`s) if not
   already present.
3. Translate the DSP files below. Preserve fixed-point arithmetic verbatim
   (use `wrapping_*`); modernise *structure* only -- modules, methods, enums,
   `match` instead of function-pointer tables.
4. Add `examples/render_wav.rs` mirroring `rings/test/*_test.cc` and diff the
   output WAV against the C test with `tools/wav_diff.py`.

## Source inventory (DSP + UI, drivers/bootloader/resources excluded)

| file | lines |
|------|-------|
| `cv_scaler.cc` | 231 |
| `cv_scaler.h` | 225 |
| `meter.h` | 84 |
| `rings.cc` | 167 |
| `settings.cc` | 65 |
| `settings.h` | 99 |
| `ui.cc` | 454 |
| `ui.h` | 124 |
| `dsp/dsp.h` | 45 |
| `dsp/fm_voice.cc` | 154 |
| `dsp/fm_voice.h` | 126 |
| `dsp/follower.h` | 112 |
| `dsp/limiter.h` | 81 |
| `dsp/note_filter.h` | 121 |
| `dsp/onset_detector.h` | 228 |
| `dsp/part.cc` | 578 |
| `dsp/part.h` | 192 |
| `dsp/patch.h` | 43 |
| `dsp/performance_state.h` | 50 |
| `dsp/plucker.h` | 93 |
| `dsp/resonator.cc` | 122 |
| `dsp/resonator.h` | 99 |
| `dsp/string.cc` | 218 |
| `dsp/string.h` | 169 |
| `dsp/string_synth_envelope.h` | 144 |
| `dsp/string_synth_oscillator.h` | 183 |
| `dsp/string_synth_part.cc` | 442 |
| `dsp/string_synth_part.h` | 141 |
| `dsp/string_synth_voice.h` | 75 |
| `dsp/strummer.h` | 103 |
| `dsp/fx/chorus.h` | 119 |
| `dsp/fx/ensemble.h` | 134 |
| `dsp/fx/fx_engine.h` | 301 |
| `dsp/fx/reverb.h` | 184 |
| `test/rings_test.cc` | 572 |
| `drivers/adc.cc` | 190 |
| `drivers/adc.h` | 94 |
| `drivers/codec.cc` | 562 |
| `drivers/codec.h` | 102 |
| `drivers/debug_pin.h` | 76 |
| `drivers/debug_port.cc` | 64 |
| `drivers/debug_port.h` | 68 |
| `drivers/leds.cc` | 55 |
| `drivers/leds.h` | 63 |
| `drivers/normalization_probe.h` | 77 |
| `drivers/switches.cc` | 59 |
| `drivers/switches.h` | 73 |
| `drivers/system.cc` | 45 |
| `drivers/system.h` | 50 |
| `drivers/trigger_input.cc` | 55 |
| `drivers/trigger_input.h` | 67 |
| `drivers/version.h` | 64 |

## Resources

`rings/resources.cc` (1577 lines of generated lookup tables) -> transpile with `tools/transpile_resources.py` into `src/resources.rs`, exactly as done for `braids`.

## Not in scope for the library crate

STM32/AVR peripheral drivers (`drivers/`), the audio bootloader, and the
`hardware_design/` files stay in the C repo -- the Rust crate is a `no_std`
DSP library that a host or an embedded HAL feeds.
