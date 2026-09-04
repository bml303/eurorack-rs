# Porting Plaits

**Macro oscillator, 24 synthesis models**  |  MCU family: `stm32f3` (hardware FPU)

## Status: ported, 22/24 models real

Unlike `braids` (fixed-point, Cortex-M3, bit-exact port required), Plaits runs
entirely in `f32` on a hardware FPU, so there is no fixed-point arithmetic to
preserve verbatim -- the port follows ordinary idiomatic-Rust judgment
throughout (methods instead of free functions, runtime parameters instead of
C++ template parameters, `Option`/slices instead of nullable pointers). See
`crates/plaits/src/lib.rs` and `crates/plaits/src/voice.rs` for the top-level
docs on that and on the two deviations below.

All shared DSP infrastructure is ported: oscillators (`src/oscillator/`, 12
files), noise (`src/noise/`), FX (`src/fx/`, including a hand-built
`fx_engine.rs`/`Tap`/`FxContext` replacing the C++ template-metaprogrammed
`FxEngine<size,format>::Reserve<N,...>` memory layout), physical modelling
(`src/physical_modelling/`), the chord bank (`src/chords/`), the analog/
synthetic drum voices (`src/drums/`), and the envelope/downsampler helpers.
`stmlib` gained `filter` (`OnePole`/`Svf`/`NaiveSvf`/`DcBlocker`), `delay_line`,
`hysteresis_quantizer`, `limiter`, `polyblep`, `rsqrt`, and a redesigned
`ParameterInterpolator` (see below) to support it.

22 of the 24 `plaits/dsp/engine*` models are ported (`src/engines/`):
virtual analog, waveshaping, FM, grain, additive, wavetable, chord, swarm,
noise, particle, string, modal, bass drum, snare drum, hi-hat, virtual-analog
VCF, phase distortion, wave terrain, string machine, and chiptune (with its
own `arpeggiator.rs`). `SixOpEngine` (6-op DX7-style FM -- `plaits/dsp/fm/*`,
~2000 lines) and `SpeechEngine` (LPC/SAM speech synthesis -- `plaits/dsp/
speech/*`, ~3000 lines) are stubs that satisfy the `Engine` trait and render
silence; they are by far the largest and most self-contained subsystems left
out of this port. `src/voice.rs` wires all 24 slots (22 real + 2 stubs) into
one `Voice::render()`, matching `plaits/dsp/voice.cc`'s engine registration,
trigger/LPG/internal-envelope logic, and final limiter/low-pass-gate stage.

## A necessary design fix along the way: `ParameterInterpolator`

The C's `ParameterInterpolator` is RAII: its destructor writes the ramped
value back through a `float*` it was constructed with. `stmlib::
ParameterInterpolator` (originally written for `braids`, which doesn't use
it) was redesigned to hold `&'a mut f32` and implement `Drop` to do that
write-back automatically -- the natural Rust translation of a C++ destructor
side effect, and the one used throughout this crate. `Downsampler` follows
the same pattern.

## Verification

`tests/smoke.rs` renders every one of the 24 engine slots across a parameter
sweep (harmonics/timbre/morph, note, periodic triggers) and asserts only that
`Voice::render` never panics -- there is no bit-exact or numerical-tolerance
golden test here (unlike `braids`'s CRC32 equivalence test), since floating-
point non-determinism across compilers/optimization levels makes a tight
tolerance-based comparison to the C reference a separate, larger undertaking
than this port itself.

## Deviations from the C (see also `src/voice.rs`'s module doc)

* `Engine::load_user_data` always receives `None` -- nothing in this
  workspace wires flash storage into `Voice`, so the C's `UserData::
  ptr(engine_index)` / `fm_patches_table[]` fallback lookups never fire. This
  only matters for the two stub engines and the wavetable/wave-terrain
  engines' optional user tables.
* `SixOpEngine` is registered 3 times in the C (engine slots 2-4) against a
  single shared instance, differentiated only by which FM patch bank
  `LoadUserData` gave it; since it's a silent stub here, the three slots are
  behaviorally identical.
* `Voice`'s 1ms trigger-delay line uses `stmlib::DelayLine` (const-generic,
  owns its buffer) rather than the C's non-owning `plaits::DelayLine` variant
  that takes an external buffer pointer in `Init()` -- functionally
  equivalent, just a different (Rust-idiomatic) ownership model for the same
  fixed-size ring buffer.
