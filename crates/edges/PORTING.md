# Edges -- port status

**Quad chiptune oscillator**  |  MCU family: `avr` (ATxmega, `F_CPU` 32 MHz)

## Status: PORTED (fixed-point, follows the shipping firmware)

Edges is an 8-bit AVR module, so the arithmetic is reproduced verbatim like
`mi-braids`: 24-bit phase accumulators, `u8` / `u16` wrap, the exact
table-lookup interpolation.

### Modules

| Rust file | C source | notes |
|-----------|----------|-------|
| `src/resources.rs` | `resources.{cc,h}` | transpiled (see header for the two mechanical preprocessing steps) |
| `src/digital_oscillator.rs` | `digital_oscillator.{h,cc}` | channel 4: the sampled oscillator -- band-limited triangle, NES triangle, pitched noise, NES noise (long/short), bit-crushed sine |
| `src/timer_oscillator.rs` | `timer_oscillator.{h,cc}` | channels 0..3: the firmware's dual-slope-PWM square-wave timer register computation, plus a software square renderer (`render_square`, **not** in the firmware) |

Out of scope: the MIDI stack (`midi*.h`, `note_stack`, `voice_allocator`),
`settings` (flash), `ui`, `adc_acquisition`, the bootloader, `hardware_design/`.

### The `InterpolateSample` question

`digital_oscillator.cc` defines its own `InterpolateSample` / `InterpolateSample16`
as AVR inline assembly. The pointer arithmetic works out to:

* index `= phase.integral >> 7` (the wavetables are **513** bytes = 512 + guard)
* blend weight `w = (phase.integral & 0x7f) << 1` -- an even value in `0..=254`
* result `table[i] * (255 - w) + table[i+1] * w` (`InterpolateSample` returns the
  high byte, `InterpolateSample16` the full 16 bits)

`avrlibx/utils/op.h` also ships a *portable* `InterpolateSample` that indexes at
`phase >> 8` (256-entry) -- but Edges never calls it (its functions are local
statics, always the asm form), and it disagrees with the 513-byte table layout.
**This port follows the firmware asm.** A host build of the C would take the
portable path and drift, which is why there is no C bit-compare harness.

### Deviations from the C

* `lut_res_oscillator_increments` (97 entries) is indexed without an upper bound
  in `ComputePhaseIncrement`; a valid MIDI note stays inside it, and the port
  clamps the index so an out-of-range pitch reads the last cell instead of past
  the array. Same for a high-note NES triangle indexing `waveform_table`.
* `TimerOscillator::render_square` is new: the firmware drives a hardware PWM
  pin, so the C type only computes the registers. `render_square` synthesises a
  plain 2-level square (`F_CPU / (2 * period * prescaler_divisor)`, duty
  `value / period`) so a host can hear the channel.

### Verification

`tests/smoke.rs`: range / silence / "makes sound" for every shape; measured
pitch tracking (`A4` triangle within 12 Hz of 440, octave ratio within 5 %);
`TimerOscillator` note frequency and 50 % duty; `render_square` frequency and
duty; `sub_follow` ratios; and a locked FNV-1a checksum of the digital
oscillator output per shape as a no-toolchain regression guard.
`examples/edges_wav.rs` renders a short arpeggio from any oscillator:

```
cargo run --release --example edges_wav -p mi-edges -- \
    [triangle|nes_triangle|noise|nes_noise_long|nes_noise_short|sine|square] [out.wav]
```
