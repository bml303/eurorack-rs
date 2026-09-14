# Streams -- port status

**Dual dynamics gate (VCA/VCF + envelope follower)**  |  MCU family: `stm32f105`

## Status: PORTED (fixed-point, follows the shipping firmware)

Streams is a 32-bit fixed-point STM32F105 module built around one shared
shape: 6 independent "dynamics processor" algorithms, each turning an
`(audio, excite)` sample pair into a `(gain, frequency)` pair for the
analog VCA/VCF that follows in hardware. Several algorithms
(`Vactrol`/`Follower`/`Compressor`) do real intermediate arithmetic in
64-bit, matching the C++'s explicit `int64_t` casts verbatim -- including
the points where narrowing a 64-bit accumulator back into a 32-bit state
variable each call is load-bearing, not just a formality (see below).

### Modules

| Rust file | C source | notes |
|-----------|----------|-------|
| `src/resources.rs` | `resources.{cc,h}` | transpiled directly, no preprocessing needed |
| `src/consts.rs` | `gain.h` | DAC-code gain reference constants |
| `src/meta_parameters.rs` | `meta_parameters.h` | `compute_amount_offset`, `compute_attack_decay` |
| `src/svf.rs` | `svf.{h,cc}` | the analysis state-variable filter used by `Follower` |
| `src/audio_cv_meter.rs` | `audio_cv_meter.h` | standalone audio/CV discriminator + peak meter (see below) |
| `src/filter_controller.rs` | `filter_controller.h` | the "plain filter" processor function |
| `src/envelope.rs` | `envelope.{h,cc}` | AD/AR/ADSAR/ADAR envelope generator |
| `src/follower.rs` | `follower.{h,cc}` | 3-band envelope follower + spectral-centroid frequency output |
| `src/vactrol.rs` | `vactrol.{h,cc}` | photocell/vactrol VCA+VCF model, plus a "plucked" Schmitt-triggered mode |
| `src/compressor.rs` | `compressor.{h,cc}` | feed-forward RMS compressor, fixed-point `log2`/`exp2`, soft knee |
| `src/lorenz_generator.rs` | `lorenz_generator.{h,cc}` | Lorenz-attractor chaotic generator |
| `src/processor.rs` | `processor.{h,cc}` | `Processor`: dispatches to one of the 6 algorithms above |

Out of scope: `streams.cc` (the app-level ADC/DAC/UI main loop),
`cv_scaler.{h,cc}` (front-panel CV scaling), `ui.{h,cc}` (the UI state
machine), and the STM32 peripheral drivers.

### `AudioCvMeter`: not on `Processor`'s signal path, ported anyway

`audio_cv_meter.h`'s `AudioCvMeter` (zero-crossing-rate audio/CV
discrimination + a peak follower) is only ever used by `ui.cc` (per-channel,
for its own display), which is out of scope -- but the class itself has no
hardware dependency, so it's ported as a standalone utility, the same call
already made for e.g. `mi-warps`. `Compressor::log2`/`exp2` are similar:
`log2` is used by `Compress`; its natural counterpart `exp2` is declared in
the C++ but never actually called anywhere (dead code upstream too) --
kept and made `pub` here since it's a genuinely useful, self-contained
counterpart to have.

### Deviations from the C++

* `Processor` dispatches its 6 algorithms via `match` on a
  [`processor::ProcessorFunction`] enum instead of the C's
  `ProcessorCallbacks` function-pointer table (`DECLARE_PROCESSOR`/
  `REGISTER_PROCESSOR` macros) -- every sub-processor is a plain field, and
  `Processor::process`/`configure` just call the right one directly.
* Every `configure(alternate, parameters, globals)` takes `globals` as
  `Option<&[i32; 4]>` in place of the C's nullable `int32_t* globals`
  (`Processor::Configure` passes either `globals_` or `NULL` depending on
  `linked_`) -- the `None`/`Some` cases match the C's null-check branches
  1:1 in every algorithm.
* `Envelope::set_ad`/`set_ar`/`set_adsar`/`set_adar` only ever configure up
  to 4 segments in the shipped firmware, but the class's public API (`set_
  num_segments`/`set_time`/`set_level`/`set_sustain_point`) technically
  allows up to `kMaxNumSegments == 8` -- `Process` reads `level[segment +
  1]` with `segment` ranging up to `num_segments` itself, so a caller
  configuring more than 7 segments through the raw setters would read past
  `level`'s 8-slot capacity in the C++ too (not something this port
  introduces or fixes; the constraint is inherent to the original array
  sizing and is only ever exercised within it by the code that actually
  ships).
* `Compressor::compress`'s soft-knee lookup (`lut_soft_knee[attenuation >>
  8]`/`[... + 1]`) clamps its index defensively -- unlike the 3
  `mi-frames` findings, this isn't proven reachable with the parameter
  ranges `configure`'s public API can produce (working out the exact bound
  through the compressor's `ratio_ = knee_gain / (threshold_ >> 8)` branch
  would take real effort), so it's flagged here as defensive rather than a
  confirmed upstream bug.

### A vestigial dead branch, preserved on purpose

`Envelope::Process`'s local `release` is declared `false`, and the only
statement that could set it -- `gate_ = false; release = false;`, on the
gate's falling edge -- sets it to `false` too. `release` is therefore
provably always `false` at the `else if (release && sustain_point_)` check
a few lines later, making that whole branch (a sustain-to-release
transition, entered from a "held" state back toward the release segment)
dead code in the shipped firmware -- Streams' envelope is AD/AR only in
practice, with `sustain_point_` support left over from a shared lineage
with Peaks' multistage envelope. Kept exactly as the C++ has it (the
`release` variable and the dead branch both present) rather than simplified
away, since it costs nothing and removes any risk of this analysis being
wrong.

### Verification

No C bit-compare harness for this module. `tests/smoke.rs`: `Processor`
across a 20,000-step sweep of every `ProcessorFunction`/`alternate`/
`linked`/parameter/global combination; `Envelope` triggering and decaying
on a gate; `Vactrol` in both plucked and continuous modes; `Follower` on a
sine sweep; `Compressor` showing real gain reduction under a loud sustained
signal; `FilterController` tracking `excite`; `LorenzGenerator` on both
channel indices; `log2`/monotonicity and `exp2` surviving its input range;
`Svf` staying within `i16` range; and `AudioCvMeter` correctly
discriminating a slow CV-like signal from a 440 Hz audio-rate one.

## Not in scope for the library crate

STM32/AVR peripheral drivers, the audio bootloader, and the
`hardware_design/` files stay in the C repo -- the Rust crate is a `no_std`
DSP library that a host or an embedded HAL feeds.
