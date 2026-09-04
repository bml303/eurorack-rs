# Porting Peaks

**Dual function generator (envelopes, LFOs, drums)**  |  MCU family: `stm32f2`  |  ~5210 lines of hand-written C (excl. resources & drivers)

## Method

Follow the `braids` crate as the worked example:

1. `python tools/transpile_resources.py ../eurorack/peaks/resources.cc \
       ../eurorack/peaks/resources.h crates/peaks/src/resources.rs`
2. Port `mi-stmlib` primitives this module needs (check its `#include`s) if not
   already present.
3. Translate the DSP files below. Preserve fixed-point arithmetic verbatim
   (use `wrapping_*`); modernise *structure* only -- modules, methods, enums,
   `match` instead of function-pointer tables.
4. Add `examples/render_wav.rs` mirroring `peaks/test/*_test.cc` and diff the
   output WAV against the C test with `tools/wav_diff.py`.

## Source inventory (DSP + UI, drivers/bootloader/resources excluded)

| file | lines |
|------|-------|
| `calibration_data.cc` | 50 |
| `calibration_data.h` | 69 |
| `gate_processor.h` | 69 |
| `io_buffer.h` | 101 |
| `peaks.cc` | 158 |
| `processors.cc` | 87 |
| `processors.h` | 176 |
| `ui.cc` | 440 |
| `ui.h` | 156 |
| `drums/bass_drum.cc` | 92 |
| `drums/bass_drum.h` | 101 |
| `drums/excitation.h` | 88 |
| `drums/fm_drum.cc` | 196 |
| `drums/fm_drum.h` | 115 |
| `drums/high_hat.cc` | 103 |
| `drums/high_hat.h` | 62 |
| `drums/snare_drum.cc` | 105 |
| `drums/snare_drum.h` | 112 |
| `drums/svf.h` | 117 |
| `test/peaks_test.cc` | 115 |
| `modulations/bouncing_ball.h` | 126 |
| `modulations/lfo.cc` | 212 |
| `modulations/lfo.h` | 154 |
| `modulations/mini_sequencer.h` | 111 |
| `modulations/multistage_envelope.cc` | 89 |
| `modulations/multistage_envelope.h` | 319 |
| `drivers/adc.cc` | 85 |
| `drivers/adc.h` | 55 |
| `drivers/dac.cc` | 71 |
| `drivers/dac.h` | 94 |
| `drivers/debug_pin.h` | 65 |
| `drivers/gate_input.cc` | 46 |
| `drivers/gate_input.h` | 73 |
| `drivers/leds.cc` | 90 |
| `drivers/leds.h` | 72 |
| `drivers/switches.cc` | 68 |
| `drivers/switches.h` | 80 |
| `drivers/system.cc` | 83 |
| `drivers/system.h` | 50 |
| `pulse_processor/pulse_randomizer.cc` | 115 |
| `pulse_processor/pulse_randomizer.h` | 101 |
| `pulse_processor/pulse_shaper.cc` | 134 |
| `pulse_processor/pulse_shaper.h` | 106 |
| `number_station/number_station.cc` | 181 |
| `number_station/number_station.h` | 118 |

## Resources

`peaks/resources.cc` (11090 lines of generated lookup tables) -> transpile with `tools/transpile_resources.py` into `src/resources.rs`, exactly as done for `braids`.

## Not in scope for the library crate

STM32/AVR peripheral drivers (`drivers/`), the audio bootloader, and the
`hardware_design/` files stay in the C repo -- the Rust crate is a `no_std`
DSP library that a host or an embedded HAL feeds.
