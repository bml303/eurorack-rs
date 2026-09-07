# Elements -- port status

**Modal / physical-modelling synthesizer**  |  MCU family: `stm32f4` (Cortex-M4F, hardware FPU)

## Status: PORTED (floating-point, no bit-exactness contract)

Elements runs on an FPU part, so -- like `mi-plaits` and `mi-clouds` -- this is
an idiomatic floating-point port, not a bit-verified fixed-point one. The
integer-exact pieces are still translated verbatim (`wrapping_*`): the exciter
sample-player / granular phase accumulators, the Ominous FM operator phase
words, the `Random` draws.

### Modules

| Rust file | C source | notes |
|-----------|----------|-------|
| `src/resources.rs` | `resources.{cc,h}` | transpiled; `SMP_BOUNDARIES` (`size_t[]`, skipped by the tool) hand-appended |
| `src/dsp.rs` | `dsp.h` | `SAMPLE_RATE`, `MAX_BLOCK_SIZE` |
| `src/multistage_envelope.rs` | `multistage_envelope.{h,cc}` | only the ADSR preset is kept (all the voice ever uses) |
| `src/exciter.rs` | `exciter.{h,cc}` | 7 models; `fn_table_` -> `match` |
| `src/tube.rs` | `tube.{h,cc}` | boxed 2048-tap delay line |
| `src/resonator.rs` | `resonator.{h,cc}` | 64 modal + 8 bowed modes |
| `src/string.rs` | `string.{h,cc}` | `DampingFilter` + dispersion all-pass / curved bridge |
| `src/fx/fx_engine.rs` | `fx/fx_engine.h` | template metaprogramming -> `const` base-offset arrays + a borrowed `Context` (same shape as `mi-clouds`) |
| `src/fx/diffuser.rs` | `fx/diffuser.h` | mono, `FORMAT_32_BIT`, size 1024 |
| `src/fx/reverb.rs` | `fx/reverb.h` | `FORMAT_16_BIT`, size 32768 |
| `src/voice.rs` | `voice.{h,cc}` | the MODAL / STRING / STRINGS router |
| `src/ominous_voice.rs` | `ominous_voice.{h,cc}` | the "Ominous" easter-egg FM voice, 8x oversampled |
| `src/part.rs` | `part.{h,cc}` + `patch.h` | top-level type; `kNumVoices == 1` |

Out of scope (unchanged from the plan): `drivers/`, `cv_scaler`, `ui`, the
bootloader, `hardware_design/`.

### Deviations from the C

* **`stmlib::Interpolate` at full scale.** The MI resource tables are `size + 1`
  entries and several engines index them at the extreme (`geometry == 1.0`,
  `ratio == 1.0`, ...). The C then reads one entry past the array, saved only by
  the trailing `* 0.0` fraction. The local `interpolate` helpers in
  `resonator.rs` / `ominous_voice.rs` clamp the index so the result is identical
  without the out-of-bounds read.
* **`InterpolateWrap` for `x < 0`** (Ominous `Spatializer`). The C relies on
  negative-index UB; the port folds the index into `[0, 1)` instead.
* **`(uint32_t)negative_float`** in `SineFm` -- Rust `as u32` saturates, so the
  port routes through `as i64 as u32` to get the C's modular wrap.
* Allocation: the delay lines and the 64 KB reverb buffer are `Box`ed
  (`extern crate alloc`), as in `mi-plaits` / `mi-clouds`.

### Verification

No C bit-compare harness (float port). `tests/smoke.rs` sweeps every resonator
model + the Ominous voice through extreme parameter ranges as a crash / NaN /
out-of-bounds guard and checks each makes sound; `examples/elements_wav.rs`
mirrors `elements_test.cc::TestPart` for auditioning:

```
cargo run --release --example elements_wav -p mi-elements -- [modal|string|strings|ominous] [out.wav]
```
