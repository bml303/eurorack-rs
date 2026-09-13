# Porting Tides

**Tidal modulator (2014)** | MCU family: `stm32f1`

## Status: ported and bit-verified

`Generator` (`tides/generator.{h,cc}`) is ported in full and matches the C
firmware bit-for-bit: `cargo test -p mi-tides --test equivalence` replays an
18-way sweep over {range x mode x sync} (144 000 output bytes each) and checks
each one's CRC-32 against a value generated from the C DSP.

Regenerate the reference and re-check by hand:

```
g++ -O2 -DTEST -I. -Istmlib -o /tmp/tc ../eurorack-rs/tools/tides_compare.cc \
    tides/generator.cc tides/resources.cc   # from the eurorack C repo root
/tmp/tc /tmp/c
cargo run --release --example compare -p mi-tides -- /tmp/rust
python3 ../eurorack-rs/tools/wav_diff.py /tmp/c /tmp/rust
```

### Not ported (dead code in the shipped firmware)

`Generator::Process()` dispatches to `ProcessWavetable` only when the
`WAVETABLE_HACK` macro is defined; `// #define WAVETABLE_HACK` is commented
out in `generator.h`, so that path -- and the `mode_`-as-wavetable-bank-index
reuse, and the `x_`/`y_`/`z_`/`buffer_`/`prescaler_` fields it alone touches --
never runs in the real firmware and isn't ported here.

### Not in scope for the library crate

Matching the `mi-braids` precedent: `cv_scaler.{h,cc}` (ADC calibration),
`plotter.{h,cc}` and `easter_egg/` (the OLED display "easter egg"),
`ui.{h,cc}` and `tides.cc` (hardware wiring/main loop), and the peripheral
`drivers/`.

### Structural changes from the C

* The hardware interrupt double-buffer (`Process(uint8_t)` single-sample ring
  buffer + `Process()` block drain, `writable_block()`) is a latency-hiding
  wrapper around the block renderer; this port exposes that renderer directly
  as `Generator::render(control, out)` (`control.len() == out.len()`, any
  length -- the firmware always calls it with 16).

### Fidelity gotchas hit during porting

* Several expressions mix a `uint32_t` operand with an `int32_t` one (e.g.
  `discontinuity * (phase_increment >> 18)`, `(phase >> 16) * slope_up`). C's
  usual arithmetic conversions make the *product* -- and therefore the shift to
  its right -- unsigned, i.e. a **logical** right shift, even though one
  operand and the destination variable are signed. Naively casting to `i32`
  before shifting (an *arithmetic*, sign-extending shift) silently diverges
  whenever the product's top bit is set. Fixed by doing the multiply/shift in
  `u32` and only casting the final bits to `i32` when storing back into a
  signed variable. This was the one bug the equivalence harness caught -- it
  only shows up well into a render (once the operands happen to overflow into
  the top bit), which is exactly the kind of thing spot-checking by eye misses.
* `ComputeFrequencyRatio`'s `pitch -= (36 << 7)` truncates through an
  `int16_t` *before* the following `pitch * 12 / (48 << 7)` promotes back to
  `int` for the multiply -- unlike the pure add/sub chains elsewhere in
  `Generator`, truncating late instead of early here isn't equivalent, because
  a division sits in between. Matched explicitly (`i16` subtract, then
  `as i32` for the multiply/divide, then `as i16` back).
* `lut_cutoff[(frequency >> 7) + 1]` is a one-past-the-end read in the C
  whenever `smoothness >= 0` (`frequency` saturates at exactly `65536`,
  `frequency >> 7 == 512 == LUT_CUTOFF.len() - 1`). Harmless in the C -- the
  fractional part multiplying that value is always `0` at the saturation point
  -- but Rust panics on it; clamped to the last valid index instead (see
  `Generator::process_filter_wavefolder`), matching the `mi-braids` `WaveLine`
  precedent for a latent OOB-read in the reference firmware.
