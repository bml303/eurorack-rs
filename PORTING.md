# Porting guide

This workspace turns the Mutable Instruments firmware DSP into `no_std` Rust
libraries. `mi-braids` is fully ported and verified; it is the template for
fixed-point modules (most of them — the MI Cortex-M3 modules). `mi-plaits`
is the template for a floating-point module (hardware-FPU Cortex-M4F/F3):
its fidelity contract is different (see "`mi-plaits` status" below) — no
bit-exact arithmetic to preserve, so the port is ordinary idiomatic Rust
throughout.

## Scope of each crate

A module crate is a **DSP library**, not firmware. It contains the sound engine
(`*_oscillator`, resonators, filters, envelopes, the model router) and its
generated lookup tables. It does **not** contain:

* STM32 / AVR peripheral drivers (`drivers/`) — the audio DAC, ADC, GPIO, timers.
* the audio bootloader, `settings.cc` flash persistence, or `ui.cc`.
* the `hardware_design/` EAGLE files.

A caller (a host test harness, or an embedded binary wiring up `embedded-hal`)
feeds the library blocks of samples and control values.

## Fidelity contract

The MI modules are fixed-point (Braids runs on an FPU-less Cortex-M3). The ports
keep that arithmetic **verbatim** — same shift amounts, same truncation, same
2's-complement wrap-around — so a given control sequence produces bit-identical
samples to the firmware. Concretely:

* Integer overflow that the C relies on is written with `wrapping_add` /
  `wrapping_mul` / `wrapping_sub`. The workspace also sets
  `overflow-checks = false` so a stray non-`wrapping` op wraps rather than
  panics, but new code should still be explicit.
* `int16_t x = <expr that overflows i16>` becomes `... as i16 as i32` — the C
  truncates to 16 bits *before* the next step, and several models depend on it
  (`RenderDigitalFilter`, `RenderWaveParaphonic`, the macro detune tables).
* `Interpolate824` / `Mix` / `Crossfade` and the pitch tables live in
  `mi-stmlib` / `braids::dsp`; they reproduce the C overloads exactly, including
  the unsigned-vs-signed distinction (`Mix(uint16_t,…)` overflows in `int` — the
  low 16 bits that survive are what matters).
* The C `BEGIN/INTERPOLATE/END_INTERPOLATE_*` macros become small ramp structs
  (`ParamRamp`, `PhaseIncrementRamp`).

What *is* modernised — structure only:

* `match` on an enum instead of a `RenderFn fn_table_[]`.
* Enums (`MacroOscillatorShape`, `DigitalModel`, `AnalogOscillatorShape`) instead
  of bare `int`. Note the Braids quirk: the digital `fn_table_` is ordered to
  match the *tail of `MacroOscillatorShape`*, not the stale `DigitalOscillatorShape`
  enum in the C header — `DigitalModel` uses the authoritative order.
* `union DigitalOscillatorState` → a flat struct, zeroed on every shape change
  (the models are mutually exclusive, so the union's aliasing was only a RAM
  optimisation).
* `Option<&mut [u8]>` instead of a nullable `sync_out` pointer.

### Undefined behaviour in the C

A few paths are UB in the C and produce compiler-/layout-dependent results:

* **shift by ≥ 32** in the pitch-table helpers for a note far outside the audio
  range combined with an extreme timbre. `braids::dsp::c_shr_u32` matches the
  x86/g++ reference (mask the count to 5 bits); the samples are garbage under any
  reading.
* **`lut_flute_body_filter[pitch >> 7]`** in `RenderFluted` for notes above ~127
  (no clamp, unlike `RenderBlown`) and **`wave_line[(scan >> 10) + 1]`** in
  `RenderWaveLine` at maximum timbre — both read one entry past a table. This
  port clamps to the last valid entry. Effect: `WaveLine` deviates from the g++
  reference by up to ~250 LSB for a single render block at max timbre; every
  other shape is bit-identical for notes ≤ 120.

## Steps to port a module

1. **Resources.** `python tools/transpile_resources.py
   ../eurorack/<m>/resources.cc ../eurorack/<m>/resources.h
   crates/<m>/src/resources.rs`. Small model-local tables (chord tables, phoneme
   data, wavetable definitions) are hand-copied into the relevant module file.
2. **stmlib primitives.** Check the module's `#include`s; port any missing
   `mi-stmlib` piece (delay lines, SVF, `ParameterInterpolator`, resamplers…).
3. **DSP.** Translate the engine files, following the fidelity contract. Keep C
   struct/field names close enough to diff against upstream.
4. **API.** Mirror the top-level class (`Voice`, `Part`, `Modulator`, …) as the
   crate's public type.
5. **Verify.** Write a `braids_compare.cc`-style reference renderer that links the
   C DSP, a matching `examples/compare.rs`, and diff with `tools/wav_diff.py`.
   Fold a checksum of the result into a `tests/equivalence.rs` golden so CI keeps
   it honest without the C toolchain.

## `mi-braids` status

Ported and, except where noted above, **bit-verified** against
`braids/{analog,digital,macro}_oscillator.cc` @ `08460a6`:

* `analog_oscillator` — 9 BLEP waveforms
* `digital_oscillator` — all 35 models (triple ring-mod, saw swarm, comb, toy,
  4× digital filter, VOSIM, vowel, vowel-FOF, harmonics, 3× FM, plucked, bowed,
  blown, fluted, struck bell/drum, kick, cymbal, snare, 4× wavetable, 5× noise,
  digital modulation, "?")
* `macro_oscillator` — full model router
* `quantizer` + 50 scales, `svf`, `excitation`, `envelope`,
  `signature_waveshaper`, `vco_jitter_source`

Verification: `cargo test -p mi-braids --test equivalence` (47/48 shapes;
`WaveLine` excluded, see above).

## `mi-plaits` status

Ported (24 engine models working) — floating point, so no bit-exactness contract applies.
