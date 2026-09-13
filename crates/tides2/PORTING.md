# Porting Tides2

**Tidal modulator (2018)** | MCU family: `stm32f3` (Cortex-M4F, hardware FPU)

## Status: ported

`RampGenerator`, `RampShaper`/`RampWaveshaper`, `RampExtractor` and the
top-level `PolySlopeGenerator` (`tides2/{ramp_generator,ramp_shaper,
poly_slope_generator}.h`, `tides2/ramp/ramp_extractor.{h,cc}`) are ported.

Unlike `mi-tides` (2014, fixed-point Cortex-M3), Tides2's DSP is plain
`float` -- this port follows the `mi-plaits` fidelity contract: ordinary
idiomatic Rust, no bit-exactness *claim* against the C (see the workspace
`PORTING.md`). In practice, though, `cargo test -p mi-tides2 --test
equivalence` checks a 24-way sweep over {ramp mode x output mode x range}
(internal ramp source, `tides2/test/tides_test.cc`'s `TestPolySlopeGenerator`
pattern) against CRC-32s of the C DSP's output, and **on this toolchain it
comes out bit-identical** -- both sides are plain IEEE 754 `f32` ops (`+ - *
/`, comparisons) with no fast-math and no denormals produced in this sweep. A
future toolchain/target/optimisation level producing slightly different
rounding wouldn't be a regression in this port; don't chase a hypothetical
future failure here by weakening the DSP to match a specific compiler's
rounding.

`cargo test -p mi-tides2 --test smoke` additionally exercises the
external-ramp-source path (not covered by the golden sweep) and
`RampExtractor` end-to-end, as crash/plausibility checks (not numeric
correctness), in the spirit of `mi-plaits`'s `tests/smoke.rs`.

Regenerate the reference and re-check by hand:

```
g++ -O2 -DTEST -I. -Istmlib -o /tmp/t2c ../eurorack-rs/tools/tides2_compare.cc \
    tides2/poly_slope_generator.cc tides2/resources.cc \
    tides2/ramp/ramp_extractor.cc   # from the eurorack C repo root
/tmp/t2c /tmp/c
cargo run --release --example compare -p mi-tides2 -- /tmp/rust
python3 ../eurorack-rs/tools/f32_diff.py /tmp/c /tmp/rust   # numeric-tolerance diff
```

### Not in scope for the library crate

Matching the `mi-braids`/`mi-tides` precedent: `cv_reader*.{h,cc}` (ADC
calibration), `factory_test.{h,cc}`, `settings.{h,cc}` (flash-persisted user
settings), `ui.{h,cc}` and `tides.cc` (hardware wiring/main loop), and the
peripheral `drivers/`.

### Structural changes from the C

* `RampGenerator::Step<RampMode, OutputMode, Range, bool use_ramp>` and
  `PolySlopeGenerator::RenderInternal<RampMode, OutputMode, Range>` are C++
  templates instantiated into function-pointer tables (`INSTANTIATE`/
  `INSTANTIATE_RAM` macros) purely so the firmware can place the hot
  (AR/LOOPING) variants `IN_RAM`; the template bodies already branch on their
  "compile-time" parameters with plain `if`/`==` (not `if constexpr`), so
  they'd work identically as ordinary runtime parameters. This port drops the
  table and the RAM-placement split, taking `RampMode`/`OutputMode`/`Range` as
  regular enum arguments and matching the `mi-braids` precedent of a `match`
  replacing a `RenderFn fn_table_[]`.
* `use_ramp` (always 1:1 with "is the `ramp` pointer non-null" at every C call
  site) becomes `ramp: Option<f32>` / `Option<&[f32]>` instead of a separate
  bool.
* `RampExtractor::ProcessInternal<bool smooth_audio_rate_tracking>` keeps its
  const-generic split (`process_internal::<SMOOTH_AUDIO_RATE_TRACKING>`)
  since the two bodies are genuinely disjoint algorithms sharing only the
  history-buffer bookkeeping, not a case of the C over-templating something
  that was already a runtime branch.
* `RampWaveshaper::Shape`'s `shape` parameter is documented as a `[1025]`-x-row
  followed by a `[1025]`-y-row (i.e. two adjacent `LUT_WAVETABLE` rows) rather
  than left as an untyped `const int16_t*` -- the C computed the y-row offset
  (`+ 1025`) inline at every read.

### A pre-existing latent bug, noted but not "fixed"

`RampExtractor::ProcessInternal<true>` (the `smooth_audio_rate_tracking` path)
divides by the just-recorded pulse's `total_duration` (`expected_phase = 2 *
block_size / period * f_ratio`) whenever `reset_counter_` reaches zero on a
rising edge. On a **freshly-`Init`ed extractor whose very first-ever gate
sample is already a rising edge**, that duration is the `Reset()`-seeded `0`,
so `period == 0`: division by zero yields `+inf`, and the subsequent `while
(expected_phase >= 1.0f) expected_phase -= 1.0f;` (a plain reproduction here)
never terminates. This is a property of the algorithm itself, present
identically in the C -- not something the port introduces or could
meaningfully "fix" while staying a faithful translation. `tests/smoke.rs`
documents it at the point it steers its clock-generator fixture clear of
triggering it.
