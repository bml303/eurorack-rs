# Warps -- port status

**Meta-modulator (cross-modulation, vocoder)**  |  MCU family: `stm32f3`

## Status: PORTED (floating-point, no bit-exactness contract)

Warps is a Cortex-M4F module, so it's ported idiomatically like `mi-rings`/
`mi-clouds`/`mi-elements`: integer-exact bits (the sample-format conversions,
the `Xor` algorithm's `i16` cast) kept verbatim, everything else ordinary
`f32` arithmetic.

### Modules

| Rust file | C source | notes |
|-----------|----------|-------|
| `src/resources.rs` | `resources.{cc,h}` | transpiled directly, no preprocessing needed (unlike the AVR crates) |
| `src/parameters.rs` | `dsp/parameters.h` | `OscillatorShape`, `Parameters` (`ModulationAlgorithm` dropped -- UI-only labels, see below) |
| `src/sample_rate_converter.rs` | `dsp/sample_rate_converter.h` + `dsp/sample_rate_conversion_filters.h` | polyphase FIR up/downsampler, resolved to a runtime loop (see below) |
| `src/oscillator.rs` | `dsp/oscillator.{h,cc}` | sine / polyBLEP triangle-saw-pulse / filtered-ducked-noise carrier oscillator |
| `src/quadrature_oscillator.rs` | `dsp/quadrature_oscillator.h` | wavetable I/Q oscillator (easter egg carrier) |
| `src/quadrature_transform.rs` | `dsp/quadrature_transform.h` | all-pass-filter-cascade Hilbert transform (easter egg modulator input) |
| `src/filter_bank.rs` | `dsp/filter_bank.{h,cc}` | 20-band vocoder analysis/synthesis filter bank |
| `src/limiter.rs` | `dsp/limiter.h` | soft peak limiter (distinct from `mi-stmlib`'s own `Limiter` -- see below) |
| `src/vocoder.rs` | `dsp/vocoder.{h,cc}` | envelope followers + the vocoder's band-gain crossfade |
| `src/modulator.rs` | `dsp/modulator.{h,cc}` | the top-level engine: `Modulator` |

Out of scope: `warps.cc` (the app-level ADC/DAC/UI main loop),
`cv_scaler.{h,cc}` / `meter.h` (front-panel CV scaling and metering),
`settings.{h,cc}` / `ui.{h,cc}` (persisted settings and the UI state
machine), the STM32 peripheral drivers, and `hardware_design/`.

### Deviations from the C++

* **`ModulationAlgorithm` dropped.** It's a UI-only labelling enum (used by
  `settings.cc`'s string tables, out of scope) -- `Modulator` itself only
  ever reads `parameters_.modulation_algorithm` as a plain `f32` in `[0, 8)`.
* **`SampleRateConverter` re-implemented as a runtime loop**, not a
  compile-time-unrolled template. The C++ builds `SampleRateConverterUp`/
  `Down` from a recursive `FilterState<N>` shift-register struct and an
  `Accumulator`/`PolyphaseStage` template metaprogram that resolves the
  polyphase decomposition at compile time -- effectively a compile-time-
  specialised direct-form FIR, unrolled 3x for speed on a Cortex-M. Only 3
  `(ratio, filter_size)` pairs are ever instantiated in the firmware: `(6,
  48)` (`Modulator`'s x6 oversampling), `(4, 48)` and `(3, 36)`
  (`FilterBank`'s low/mid-band resampling). This port works the polyphase
  index arithmetic out once (documented in `sample_rate_converter.rs`'s
  module doc comment) into a plain loop parameterised by a const-generic tap
  count (`SampleRateConverterUp<TAPS>`/`SampleRateConverterDown<FULL>`),
  with `ratio` and the half-length coefficient table (all FIRs are
  symmetric/linear-phase, so only half is stored, matching the C) as
  ordinary fields -- mathematically identical, just not unrolled.
* **Packed/shared buffers given their own storage.** Three places in the
  C++ reuse one physical array for two logically distinct roles purely to
  save RAM on the STM32F3:
  - `Modulator::buffer_[0]` is both `carrier` and, later in the same
    `Process` call, `main_output`.
  - `Modulator::src_buffer_[0]` is both `oversampled_carrier` and, later,
    `oversampled_output`.
  - `FilterBank::samples_`/`delay_buffer_` are shared arenas that every
    `Band` takes a slice of (offsets computed once in `Init`); likewise
    `PooledDelayLine`.

  Verified sample-by-sample that every case is either "fully consumed
  before the reuse" or "read-then-immediately-overwritten at the same
  index" (safe in-place aliasing, not a case where the *aliasing itself*
  changes the result) -- so this port just gives each role its own
  fixed-capacity array/buffer instead of reproducing the pointer reuse.
  Same call already made for `mi-grids`' `Options` union.
* **`mi-warps::limiter::Limiter` is not `mi-stmlib::limiter::Limiter`.**
  They share a name and a `SLOPE`-based peak tracker, but `warps/dsp/
  limiter.h`'s `Process` additionally runs the result through
  `stmlib::SoftLimit` before writing it back, which `stmlib/dsp/limiter.h`'s
  own (simpler) `Limiter` does not -- genuinely two different classes in
  the C++, kept as two separate types here rather than merged.
* **A real ordering subtlety in `ProcessEasterEgg`, replicated on purpose.**
  Three of the C++'s local `ParameterInterpolator`s (`mix`, `feedback_amount`,
  `dry_wet`) are declared in the function's outer scope, so their
  destructor -- which writes the ramped value back into the `float*` it
  borrowed -- runs when the function returns, i.e. *after* the function's
  own `previous_parameters_ = parameters_;` (its last statement) has already
  copied every field including the ones those three interpolators still
  own. The destructors' writes land last, so `previous_parameters_.
  modulation_parameter`/`.channel_drive[0..2]` end up holding the
  *ramped* value, not the exact `parameters_` copy the assignment just put
  there -- while every other field (including `phase_shift`, whose own
  interpolator is scoped to the inner `else` block and so drops, and
  writes, *before* the final assignment) does get the exact copy. Rust's
  borrow checker won't allow a whole-struct assignment while a
  `ParameterInterpolator` still holds a live borrow into one of its fields,
  so this port can't literally reproduce the "assign, then let interpolators
  clobber 3 fields" sequence as one statement -- instead it copies every
  *other* field explicitly and leaves `modulation_parameter`/`channel_drive`
  alone, then lets `mix`/`feedback_amount`/`dry_wet` drop naturally at the
  end of the function, landing in the same final state. See the comments at
  `Modulator::process_easter_egg`'s interpolator declarations. The audible
  effect is utterly negligible (the ramped value differs from the target by
  at most a couple of float ULPs, only visible as the *starting point* of
  the next call's ramp) but it's a genuine, easy-to-miss C++ RAII-ordering
  quirk worth documenting for any future review of this file.

### Verification

No C bit-compare harness (float; the C's own `warps_test.cc` needs an
external `audio_samples/modulation_96k.wav` file not present in the repo for
its two most representative tests, `TestModulator`/`TestEasterEgg`).
`tests/smoke.rs`: `Modulator` across a 2000-block sweep of every
`carrier_shape`/`modulation_algorithm`/`modulation_parameter`/
`channel_drive`/`note` combination (crossing every cross-modulation
algorithm and the vocoder), asserting no panic and audible main+aux output;
the same for the easter egg; a bypass pass-through check; every
`Oscillator` shape for finite, audible output; and a `SampleRateConverterUp`
-> `Down` round-trip amplitude sanity check on a sine sweep.

### A real bug this port's fidelity check caught

None found in this module -- unlike Plaits' speech engine or Marbles'
`LagProcessor`/`Quantizer`, nothing in Warps' index arithmetic reads outside
its tables for any input this port's smoke sweep reaches. The interesting
finding here was the `ProcessEasterEgg` RAII-ordering quirk documented
above -- not a bug exactly (it's what the shipped firmware does), but subtle
enough that a less careful port would likely have "fixed" it by accident.

## Not in scope for the library crate

STM32 peripheral drivers, the audio bootloader, and the `hardware_design/`
files stay in the C repo -- the Rust crate is a `no_std` DSP library that a
host or an embedded HAL feeds.
