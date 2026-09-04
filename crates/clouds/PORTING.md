# Porting Clouds

**Granular texture synthesizer**  |  MCU family: `stm32f4`  |  ~7529 lines of hand-written C (excl. resources & drivers)

## Method

Follow the `braids` crate as the worked example:

1. `python tools/transpile_resources.py ../eurorack/clouds/resources.cc \
       ../eurorack/clouds/resources.h crates/clouds/src/resources.rs`
2. Port `mi-stmlib` primitives this module needs (check its `#include`s) if not
   already present.
3. Translate the DSP files below. Preserve fixed-point arithmetic verbatim
   (use `wrapping_*`); modernise *structure* only -- modules, methods, enums,
   `match` instead of function-pointer tables.
4. Add `examples/render_wav.rs` mirroring `clouds/test/*_test.cc` and diff the
   output WAV against the C test with `tools/wav_diff.py`.

## Source inventory (DSP + UI, drivers/bootloader/resources excluded)

| file | lines |
|------|-------|
| `clouds.cc` | 137 |
| `cv_scaler.cc` | 215 |
| `cv_scaler.h` | 156 |
| `meter.h` | 84 |
| `settings.cc` | 89 |
| `settings.h` | 113 |
| `ui.cc` | 392 |
| `ui.h` | 130 |
| `dsp/audio_buffer.h` | 302 |
| `dsp/correlator.cc` | 88 |
| `dsp/correlator.h` | 88 |
| `dsp/frame.h` | 44 |
| `dsp/grain.h` | 205 |
| `dsp/granular_processor.cc` | 465 |
| `dsp/granular_processor.h` | 215 |
| `dsp/granular_sample_player.h` | 256 |
| `dsp/looping_sample_player.h` | 206 |
| `dsp/mu_law.cc` | 69 |
| `dsp/mu_law.h` | 83 |
| `dsp/parameters.h` | 68 |
| `dsp/sample_rate_converter.h` | 96 |
| `dsp/window.h` | 122 |
| `dsp/wsola_sample_player.h` | 290 |
| `dsp/pvoc/frame_transformation.cc` | 361 |
| `dsp/pvoc/frame_transformation.h` | 100 |
| `dsp/pvoc/phase_vocoder.cc` | 107 |
| `dsp/pvoc/phase_vocoder.h` | 76 |
| `dsp/pvoc/stft.cc` | 210 |
| `dsp/pvoc/stft.h` | 116 |
| `dsp/fx/diffuser.h` | 112 |
| `dsp/fx/fx_engine.h` | 302 |
| `dsp/fx/pitch_shifter.h` | 116 |
| `dsp/fx/reverb.h` | 180 |
| `test/clouds_test.cc` | 230 |
| `drivers/adc.cc` | 116 |
| `drivers/adc.h` | 72 |
| `drivers/codec.cc` | 564 |
| `drivers/codec.h` | 102 |
| `drivers/debug_pin.h` | 76 |
| `drivers/debug_port.cc` | 64 |
| `drivers/debug_port.h` | 68 |
| `drivers/gate_input.cc` | 60 |
| `drivers/gate_input.h` | 72 |
| `drivers/leds.cc` | 100 |
| `drivers/leds.h` | 109 |
| `drivers/switches.cc` | 69 |
| `drivers/switches.h` | 77 |
| `drivers/system.cc` | 43 |
| `drivers/system.h` | 52 |
| `drivers/version.h` | 62 |

## Resources

`clouds/resources.cc` (2983 lines of generated lookup tables) -> transpile with `tools/transpile_resources.py` into `src/resources.rs`, exactly as done for `braids`.

## Not in scope for the library crate

STM32/AVR peripheral drivers (`drivers/`), the audio bootloader, and the
`hardware_design/` files stay in the C repo -- the Rust crate is a `no_std`
DSP library that a host or an embedded HAL feeds.
