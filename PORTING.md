# Porting guide

This workspace turns the Mutable Instruments firmware DSP into `no_std` Rust
libraries. `mi-braids` is fully ported and verified; it is the template for
fixed-point modules (most of them — the MI Cortex-M3 modules). `mi-plaits`
and `mi-clouds` are the templates for floating-point modules (hardware-FPU
Cortex-M4F/F3): their fidelity contract is different (see the status sections
below) — no bit-exact arithmetic to preserve, so the port is ordinary
idiomatic Rust throughout, though any genuinely integer-exact sub-parts
(phase accumulators, bit-twiddling correlators, companding) are still kept
verbatim.

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

## `mi-clouds` status

Fully ported — floating point (Cortex-M4F), so like `plaits` no bit-exactness
contract applies; the integer-exact pieces (phase accumulators, the sign-bit
`Correlator`, `mu_law`, the `ShyFft` and phase words) are still translated
verbatim.

* `PlaybackMode::Granular`, `::Stretch` (WSOLA), `::LoopingDelay`,
  `::Spectral` (phase vocoder) — all working.
* `fx`: `Diffuser`, `Reverb` (12-bit), `PitchShifter`, plus the shared
  `FxEngine` accumulator machine — working.
* `GranularProcessor` — feedback path, low-fidelity 2x resampling + 8-bit
  mu-law buffer, diffusion / pitch-shift / tone-filter / reverb post chain,
  dry/wet.
* Spectral pulled `stmlib::fft::ShyFft` (the `RotationPhasor` variant) and
  `stmlib::atan` (`fast_atan2r` + `atan_lut`) into `mi-stmlib`.

Verification: `tools/clouds_compare.cc` + `examples/clouds_compare.rs` +
`tools/wav_diff.py`. 13 of 16 (mode × quality) dumps are bit-identical to the
C firmware DSP — every Granular and every Spectral render; `LoopingDelay`
q0/q1 differ by ≤ 1 LSB on ≤ 2 of 96000 samples, and mono `Stretch` diverges
into a different-but-valid WSOLA splice near the end of a 3 s render (1-ULP
flip of a correlator comparison). `tests/equivalence.rs` locks the Rust
output as a CI regression guard; `mi-stmlib` has a `ShyFft` round-trip test.

**Bug in the C reproduced with a fix:** `clouds/dsp/window.h`'s `Window::Start`
originally set `done_ = false` *twice*; two "remove duplicate assignment"
commits (`fbb53ba`, `0e3756f`, merged March 2023 in `d1d8839`) removed both,
so a freshly started window is permanently `done()` and Stretch mode is silent
in any host build (`clouds_test.cc` only tests Granular / LoopingDelay). The
port reinstates the assignment; the C reference needs the same one-line fix.

## `mi-elements` status

Ported — floating point (Cortex-M4F), same contract as `plaits` / `clouds`.
The integer-exact pieces (exciter sample-player + granular phase accumulators,
the Ominous FM operator phase words, `Random` draws) are translated verbatim.

* `Part` — the top-level type (`kNumVoices == 1`): "space"-macro mix, soft
  limiter, level meters, output reverb.
* `Voice` — the MODAL router: 3 exciters (bow / blow / strike) + tube + input
  diffuser feeding either the `Resonator` (64 modal + 8 bowed modes) or a bank
  of 1 / 5 `String`s (STRING / STRINGS), each a Karplus-Strong loop with a FIR
  damping filter and a dispersion all-pass / curved-bridge non-linearity.
* `OminousVoice` — the hidden "Ominous" 2x2-op FM voice, 8x oversampled, IIR +
  101-tap FIR downsampled, band-pass filtered, spatialised.
* `fx` — the `FxEngine` accumulator machine (`elements/dsp/fx/fx_engine.h`,
  a sibling of the clouds one), `Diffuser` (mono, 32-bit) and `Reverb` (16-bit).

Verification: no C bit-compare (float port). `tests/smoke.rs` sweeps every
resonator model + the Ominous voice through extreme parameters as a crash /
NaN / OOB guard and checks each makes sound; `examples/elements_wav.rs` mirrors
`elements_test.cc::TestPart`. Deviations from the C (in-bounds `Interpolate`
clamps where the C reads one past a `size + 1` table, positive `InterpolateWrap`,
`as i64 as u32` for the negative-float phase cast) are listed in
`crates/elements/PORTING.md`.

## `mi-edges` status

Ported — fixed-point (8-bit AVR / ATxmega), `mi-braids`-style verbatim
arithmetic: 24-bit phase accumulators, `u8`/`u16` wrap, table interpolation.

* `DigitalOscillator` (channel 4, the only PCM output) — band-limited triangle,
  NES triangle, pitched noise, NES noise long/short, bit-crushed sine.
* `TimerOscillator` (channels 0..3) — the firmware's dual-slope-PWM timer
  register maths (`period` / `value` / prescaler with its LFO-mode hysteresis,
  `SubFollow`), plus a **new** `render_square` (software 2-level square from
  those registers — the firmware uses a hardware PWM pin).

Out of scope: the MIDI stack, `note_stack`, `voice_allocator`, `settings`,
`ui`, `adc_acquisition`, bootloader.

**No C bit-compare harness.** `digital_oscillator.cc`'s `InterpolateSample` is
AVR inline assembly that indexes the 513-byte wavetables at `phase >> 7` with an
even 8-bit blend weight; the *portable* `avrlibx` fallback (`phase >> 8`, which a
host build would compile) disagrees and Edges never calls it. The port follows
the firmware asm. `tests/smoke.rs` checks range / silence / audio for every
shape, measured pitch tracking, `TimerOscillator` frequency + duty, and locks an
FNV-1a checksum of the output per shape. `examples/edges_wav.rs` renders an
arpeggio from any oscillator. Deviations (index clamps where the C reads past
`lut_res_oscillator_increments` / `waveform_table`, the new `render_square`) are
in `crates/edges/PORTING.md`.

## `mi-rings` status

Ported — floating point (STM32F373, hardware FPU), same contract as `plaits` /
`clouds` / `elements`. Integer-exact pieces (the FM operator phase words, the
ensemble/chorus LFO table indices) translated verbatim.

* `Part` — the resonator: **all six models** (modal, sympathetic string,
  string, FM voice, quantised sympathetic string, string + reverb), 1-4 voice
  polyphony, internal exciter (`Plucker`), the odd/even (or per-voice) output
  routing, the string+reverb stereo blend.
* `Strummer` + `OnsetDetector` — audio-onset / note-CV / trigger strum
  detection with an inter-onset-interval inhibit.
* `StringSynthPart` — "Disastrous Peace": a polyphonic PolyBLEP
  string-ensemble / organ (`StringSynthOscillator`/`Voice`/`Envelope`) with a
  formant filter, chorus, ensemble and reverb.
* `fx` — the shared `FxEngine` accumulator machine (identical to
  `mi-elements` / `mi-clouds`), `Reverb` (differs from Elements' only in the
  modulated taps), `Chorus`, `Ensemble`.
* `string.cc` is byte-identical to Elements' -- lifted from `mi-elements`.

Out of scope: `cv_scaler`, `ui`, `settings`, the STM32 drivers, bootloader.

Added `NaiveSvf::split` / `split_high_in_place` to `mi-stmlib`. No C
bit-compare harness (float port; `rings_test.cc` needs external audio anyway).
`tests/smoke.rs`: model x polyphony x exciter sweeps, `StringSynthPart` x every
FX, a `Strummer` inhibit check, and an autocorrelation pitch check on STRING.
Deviations (in-bounds `Interpolate` clamps, `as i64 as u32` FM phase cast,
chord-index clamp) are in `crates/rings/PORTING.md`.

## `mi-tides` status

Ported and bit-verified (fixed-point, follows the `mi-braids` template) — see
`crates/tides/PORTING.md`. `cargo test -p mi-tides --test equivalence` checks
an 18-way {range x mode x sync} sweep, bit-identical to the C.

## `mi-tides2` status

Ported (floating point, follows the `mi-plaits` template — no bit-exactness
*contract*) — see `crates/tides2/PORTING.md`. In practice `cargo test -p
mi-tides2 --test equivalence` checks a 24-way {ramp mode x output mode x
range} sweep and it comes out bit-identical to the C on this toolchain.
