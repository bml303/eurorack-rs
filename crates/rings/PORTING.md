# Rings -- port status

**Modal / sympathetic-string resonator**  |  MCU family: `stm32f373` (hardware FPU)

## Status: PORTED (floating-point, no bit-exactness contract)

Rings has an FPU, so -- like `mi-plaits` / `mi-clouds` / `mi-elements` -- this is
an idiomatic floating-point port. The integer-exact pieces (the FM operator
phase words, the ensemble/chorus LFO table indices) are translated verbatim.

### Modules

| Rust file | C source | notes |
|-----------|----------|-------|
| `src/resources.rs` | `resources.{cc,h}` | transpiled (`lut_sine` 5121, `lut_stiffness` / `lut_4_decades` / `lut_svf_shift` 257, `lut_fm_frequency_quantizer` 129) |
| `src/string.rs` | `string.{h,cc}` | `string.cc` is byte-identical to Elements'; lifted from `mi-elements`, `set_dispersion` made a plain setter |
| `src/resonator.rs` | `resonator.{h,cc}` | 64-mode bank, even modes -> `out`, odd -> `aux` |
| `src/fm_voice.rs` + `src/follower.rs` | `fm_voice.{h,cc}`, `follower.h` | the "bonus" 2-op FM voice + its 3-band envelope/centroid follower |
| `src/plucker.rs` | `plucker.h` | the internal exciter (noise burst + comb + LP) |
| `src/note_filter.rs` | `note_filter.h` | median + adaptive-lag pitch filter |
| `src/limiter.rs` | `limiter.h` | stereo peak limiter |
| `src/onset_detector.rs` + `src/strummer.rs` | `onset_detector.h`, `strummer.h` | audio-onset strum detection |
| `src/fx/{fx_engine,reverb,chorus,ensemble}.rs` | `fx/*.h` | `fx_engine` is the shared Dattorro machine (same as `mi-elements`/`mi-clouds`); `Reverb` differs from Elements' only in the modulated taps |
| `src/part.rs` | `part.{h,cc}` + `patch.h` + `performance_state.h` | the 6-model router, 1-4 voice polyphony, chord tables, string+reverb blend |
| `src/string_synth_{oscillator,envelope,voice,part}.rs` | `string_synth_*.{h,cc}` | "Disastrous Peace": a polyphonic PolyBLEP string-ensemble / organ with formant / chorus / ensemble / reverb |

Out of scope: `cv_scaler`, `ui`, `settings`, the STM32 drivers, the bootloader.

### stmlib additions

`NaiveSvf::split` and `NaiveSvf::split_high_in_place` (the C's `Split(in, low,
high)` and its aliased `Split(in, low, in)` call).

### Deviations from the C

* `stmlib::Interpolate` at full scale: the MI resource tables are `size + 1`
  entries and the engines index them at `1.0` (`structure`, `damping`, `ratio`),
  where the C reads one entry past the array saved only by a zero fraction. The
  local `interpolate` helpers (`resonator.rs`, `fm_voice.rs`) and the chorus'
  `interp_sine` clamp the index so the result is identical without the
  out-of-bounds read.
* `(uint32_t)negative_float` in the FM `SineFm` phase cast -> `as i64 as u32`
  for the C's modular wrap (Rust's `as u32` saturates).
* `performance_state.chord` is clamped to `0..=10` before indexing the chord
  tables (the C does not clamp).

### Verification

No C bit-compare harness (float port; the C `rings_test.cc` needs external audio
files anyway). `tests/smoke.rs`: every model x polyphony + external/internal
exciter through an extreme sweep (finite / bounded / audible), `StringSynthPart`
x every FX, a `Strummer` inhibit-timer check, bypass passthrough, and an
autocorrelation pitch check on the STRING model (A3 within 10 Hz).
`examples/rings_wav.rs` mirrors `rings_test.cc`:

```
cargo run --release --example rings_wav -p mi-rings -- \
    [modal|sympathetic|string|fm|quantized|string_reverb|synth] [out.wav]
```
