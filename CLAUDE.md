# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

A Rust workspace porting the DSP of Mutable Instruments' Eurorack firmware (the C
repo at `../eurorack`) to `no_std` library crates — one crate per module plus
`mi-stmlib` for the shared code. `mi-braids` (fixed-point, bit-verified),
`mi-plaits` and `mi-clouds` (floating-point) are ported; the other 13 module
crates are scaffolds (`Cargo.toml` + `lib.rs` + `PORTING.md` inventory). Read
`PORTING.md` before porting anything — it has the fidelity contract and the
verification workflow.

## Commands

```
cargo build --workspace
cargo test  --workspace                 # braids + clouds equivalence goldens, plaits/clouds smoke
cargo test  -p mi-braids --test equivalence   # the C-parity golden test
cargo clippy --workspace
cargo run --release --example render_wav -p mi-braids -- [shape_slug | --all]
cargo run --release --example compare   -p mi-braids -- <out_dir>   # dumps NN.pcm per shape
cargo run --release --example clouds_wav     -p mi-clouds -- [granular|stretch|looping|spectral]
cargo run --release --example clouds_compare -p mi-clouds -- <out_dir>   # dumps MM.pcm per (mode,quality)
```

Verifying a port against the C requires `../eurorack` with submodules
(`git -C ../eurorack submodule update --init`); build `tools/braids_compare.cc`
against the C DSP, run both `compare` renderers, diff with `tools/wav_diff.py`.
The exact command lines are in `README.md`.

## Architecture / conventions

- **Crate = DSP library, not firmware.** No peripheral drivers, no bootloader, no
  `settings`/`ui`. Each crate is `#![no_std]`; nothing allocates. `mi-stmlib`
  depends on `libm` for the handful of float transcendental calls.
- **Fixed-point is preserved verbatim.** The ports reproduce the C integer
  arithmetic bit-for-bit (shift amounts, truncation, wrap). Overflow the C relies
  on is written `wrapping_*`; the workspace also disables `overflow-checks` in all
  profiles so a missed one wraps instead of panicking. `int16_t x = <overflowing
  expr>` ports as `... as i16 as i32`. Only *structure* is modernised: `match` on
  an enum instead of function-pointer tables, flat struct instead of the
  `union` state, ramp structs instead of the `INTERPOLATE_*` macros.
- **Lookup tables are transpiled, not hand-typed.** `tools/transpile_resources.py`
  turns a `resources.cc`/`.h` pair into `src/resources.rs` (`pub static` arrays +
  `&[&[T]]` pointer tables). Regenerate rather than edit. Small model-local tables
  are hand-copied into the module file.
- **Braids shape-enum quirk:** the C digital `fn_table_` is ordered to match the
  *tail of `MacroOscillatorShape`*, not the `DigitalOscillatorShape` enum in the
  C header (whose names past ~index 14 are stale). `braids::shapes::DigitalModel`
  uses the authoritative `fn_table_` order.
- **Known deviations from the C** live in `PORTING.md` under "Undefined behaviour
  in the C": shift-by-≥32 in the pitch helpers (matched to x86/g++ via
  `braids::dsp::c_shr_u32`) and two out-of-bounds table reads (`RenderFluted`
  above note ~127, `RenderWaveLine` at max timbre) that this port clamps. The
  equivalence test skips `WaveLine` and caps the pitch sweep at note 120 for
  this reason.
- **Clouds Stretch fix:** `mi-clouds` reinstates `done_ = false` in
  `Window::Start` — upstream Clouds deleted it (a botched "remove duplicate
  assignment" merge, March 2023), silently breaking Stretch mode in host
  builds. `tools/clouds_compare.cc` needs the same one-line C fix to verify.
  See `crates/clouds/PORTING.md`. Spectral (phase-vocoder) mode is not ported —
  it is a silent stub.

## Adding real DSP to a scaffold crate

Follow `mi-braids` exactly. The scaffold's `PORTING.md` lists the source files and
line counts. Order: transpile resources → port needed `mi-stmlib` primitives →
translate the engine → mirror the top-level class as the public type → build a
`compare` reference + golden test.
