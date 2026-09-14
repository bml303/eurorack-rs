# Stages -- port status

**Segment generator (envelopes, LFOs, sequencers, S&H, portamento, delay,
audio oscillator)**  |  MCU family: `stm32f3`

## Status: PORTED (floating-point, follows the shipping firmware)

Stages is a 6-channel module built around a single, very flexible engine,
[`SegmentGenerator`], that a channel's configuration turns into an envelope,
a free-running/tap-tempo/PLL-synced LFO, an audio-rate oscillator, a step
sequencer, a sample & hold, a portamento slide, or a clocked delay. This is
the 16th and last crate ported in this workspace.

### Modules

| Rust file | C source | notes |
|-----------|----------|-------|
| `src/resources.rs` | `resources.{cc,h}` | transpiled directly (3 tables: env frequency, portamento coefficient, sine) |
| `src/delay_line_16_bits.rs` | `delay_line_16_bits.h` | `int16_t`-quantized delay line with linear interpolation, used by the `Delay` function |
| `src/variable_shape_oscillator.rs` | `variable_shape_oscillator.h` | `SegmentGenerator`'s own small triangle/saw/square BLEP oscillator -- "taken from Plaits and simplified" per the C's own comment, so deliberately *not* the same class as `mi-plaits::VariableShapeOscillator` |
| `src/oscillator.rs` | `oscillator.h` | a second, general BLEP oscillator with 9 shapes and audio-rate FM -- not used by `SegmentGenerator` at all (see "Deviations" below) |
| `src/segment_generator.rs` | `segment_generator.{h,cc}` | the engine: `SegmentGenerator`, `segment::{Type, Configuration, Parameters}` |

Out of scope, as with every other crate here -- peripheral drivers
(`drivers/`), the audio bootloader, and additionally for this module:

* **`settings.{h,cc}`** -- flash-backed calibration/state storage
  (`stmlib::ChunkStorage`), plus `ChannelCalibrationData::dac_code` (one
  trivial function, but it exists only to serve the out-of-scope flash
  layer).
* **`cv_reader.{h,cc}`** -- wraps the `CvAdc`/`PotsAdc` peripheral drivers
  and a `Settings*` to read pots/sliders/CV; no DSP of its own.
* **`chain_state.{h,cc}`** -- the inter-module serial-link protocol: neighbor
  discovery, cross-module parameter binding, physical switch handling, all
  built on `SerialLink` (a UART driver) and `Settings`. This is where the
  "chain up to 6 Stages together" feature lives, but it's firmware
  orchestration wired through hardware, not DSP.
* **`ui.{h,cc}`**, **`factory_test.{h,cc}`**, **`stages.cc`** (`main`) --
  UI state machine, hardware self-test mode, and the app entry point.
* **`io_buffer.h`** -- the ISR double-buffering scheme; a host just calls
  `SegmentGenerator::process` with whatever block size it likes (see the
  `MAX_BLOCK_SIZE` note below for the one real constraint).
* **`test/fixtures.h`**, **`test/stages_test.cc`** -- a WAV-dumping test
  harness with no assertions; `tests/smoke.rs` reimplements its scenarios as
  real checks instead (see "Verification").

`mi-stmlib::{hysteresis_quantizer::HysteresisQuantizer2, delay_line::
DelayLine, parameter_interpolator::ParameterInterpolator, fdsp, units,
random, polyblep, gate_flags}` and `mi-tides2::{ramp_extractor::
RampExtractor, ratio::Ratio}` (already ported for `mi-tides2` itself) covered
everything this port needed. `mi-stages` is the first crate in this
workspace to depend on another module crate (`mi-tides2`) rather than only
`mi-stmlib` -- `segment_generator.cc` genuinely `#include`s
`tides2/ramp/ramp_extractor.h` and reuses it unmodified for the LFO/
oscillator functions' PLL/clock-division logic. `mi-stmlib::DelayLine<const
N: usize>` is `f32`-only (see its own doc comment), so the `GateFlags`-typed
gate-delay ring buffer `ProcessSampleAndHold` needs is a small private
`GateDelayLine` in `segment_generator.rs` instead of a shared generic --
genericizing the widely-used shared `DelayLine` just for this one caller
felt riskier than adding ten lines locally.

### `MAX_BLOCK_SIZE`: the one real call-pattern constraint

`ProcessOscillator` (behind the `Lfo`/`TapLfo`/`PLLOscillator`/
`FreeRunningOscillator` functions) uses a C variable-length array,
`float ramp[size]`, as scratch space. This port uses a fixed
`[f32; MAX_BLOCK_SIZE]` stack buffer instead (`MAX_BLOCK_SIZE = 64`,
well above the firmware's own `kBlockSize == 8`) and asserts `size <=
MAX_BLOCK_SIZE`, matching the convention already established by
`mi-braids`/`mi-clouds`/`mi-elements`. Every other function has no such
limit and accepts a block of any size.

### Deviations / design notes

* **`Segment`'s pointer fields become a `Field` enum, not raw pointers.**
  The C's `Segment` struct holds `float*` members that alias one of three
  per-instance constants (`zero_ = 0.0`, `half_ = 0.5`, `one_ = 1.0`, set
  once in `Init` and never mutated again -- confirmed by reading every
  assignment in `segment_generator.cc`) or a field of `parameters_[i]`, so
  that several segments can share backing storage and `Configure` can
  rewire a segment by repointing a pointer instead of copying a value. A
  `Field` enum (`Zero`/`Half`/`One`/`Primary(usize)`/`Secondary(usize)`)
  read through `SegmentGenerator::read` reproduces exactly the same
  aliasing/pointer-*identity* semantics (`segment.end != previous.end`'s
  C pointer comparison becomes an enum equality check, which is true
  exactly when both would have pointed at the same address) without any
  unsafe code or lifetimes. `Option<Field>` (`None` = C `NULL`) covers
  `start` ("begin from the current running value" when null), `time`
  ("infinite duration / hold" when null), and `phase` ("use the real
  accumulating ramp phase" instead of a fixed sample-or-track flag, when
  null).
* **`process_fn_table_`'s 16 function pointers become a `match` on
  `ProcessFn`.** One table slot is a genuine dead-code trap worth calling
  out explicitly: slot 9 (`HOLD` type, `loop = true`, `has_trigger =
  false`) is `ProcessDelay` in the table, with
  `// &SegmentGenerator::ProcessClockedSampleAndHold,` commented out right
  above it -- and nothing else in the codebase ever assigns
  `process_fn_ = &SegmentGenerator::ProcessClockedSampleAndHold` either.
  So `ProcessClockedSampleAndHold` is real, complete C++ that has never
  been reachable in any shipped firmware. Ported faithfully as
  `SegmentGenerator::process_clocked_sample_and_hold` (`#[allow(dead_code)]`,
  documented, real logic) rather than silently dropped, per this
  workspace's standing rule to preserve dead code the C source itself
  still carries.
* **`step_quantizer_` (musical-note quantization for the step sequencer) is
  owned, not borrowed.** In the C this is an externally-supplied
  `HysteresisQuantizer2*`; `stages.cc` hands channel `i` a pointer into one
  shared `note_quantizer[kNumChannels + kMaxNumSegments]` array *at offset
  `i`*, so `step_quantizer_[active_segment_]` for channel `i` actually reads
  `note_quantizer[i + active_segment_]` -- meaning neighbouring channels'
  quantizer windows overlap and can alias each other's hysteresis state.
  That cross-channel sharing is `stages.cc` wiring (out of scope), not a
  documented part of `SegmentGenerator`'s own contract, so this port gives
  each `SegmentGenerator` its own owned `[HysteresisQuantizer2;
  K_MAX_NUM_SEGMENTS]` bank instead of a borrowed external pointer (avoiding
  a lifetime parameter on the whole struct for a feature whose real-world
  aliasing behaviour depends entirely on out-of-scope integration code
  anyway). `Init(step_quantizer)`'s null-vs-non-null distinction becomes
  `init(quantized_step_output: bool)`; when enabled, each embedded
  quantizer is initialized with the exact parameters `stages.cc` uses for
  `note_quantizer` (`Init(13, 0.03, false)`).
* **`Oscillator` (`oscillator.h`) is ported but genuinely unused by
  `SegmentGenerator`.** Grepping the whole module, its only instantiation
  is `stages.cc`'s factory-test tone player (`oscillator[kNumChannels]`,
  driven by the out-of-scope hardware self-test mode) -- `segment_generator.
  cc` only ever uses the much smaller `VariableShapeOscillator`. Ported
  anyway since it's small, self-contained DSP with no hardware dependency,
  and it rounds out the crate; `render`'s C template parameters
  `<has_external_fm, through_zero_fm>` collapse to a single
  `external_fm: Option<&[f32]>` argument, since every real call site ties
  the two flags together (`Render(freq, pw, out, size)` passes neither;
  `Render(freq, pw, fm, out, size)` passes both).
* **`ConfigureSlave`/`ProcessSlave` are ported in full** even though
  nothing in this crate's scope calls them -- they're part of
  `SegmentGenerator`'s own public surface (the "channel N tracks a segment
  of channel N-1" feature), just normally driven by `chain_state.cc` (out
  of scope). `process_slave` reads `out[i].segment`/`.phase` as *input*
  (populated by processing the monitored generator into the same buffer
  first) and only overwrites `.value` in place -- documented on the
  method.
* **`previous_delay_sample_`** (declared in the C header) is never read or
  written anywhere in `segment_generator.cc` -- omitted; there's no
  observable behavior to preserve.

### Two real out-of-bounds table reads this port's fidelity check caught

Both are in the small rate/coefficient lookup helpers, and both stem from
`segment_generator.cc` itself (not the LUT generator) -- neither table has
the "one extra guard sample" convention several other modules' LUTs use.

1. **`RateToFrequency`'s bounds check allows a one-past-the-end read.**
   `CONSTRAIN(i, 0, LUT_ENV_FREQUENCY_SIZE)` clamps the index to `[0,
   4096]` *inclusive* against a table that -- verified by counting the
   literals in `resources.cc` -- has exactly 4096 entries (valid indices
   0..4095). Any `rate >= 2.0` (reachable: `parameters_[i].primary`'s
   range is only asserted, not enforced, and that assert is commented out)
   produces `i == 4096`, reading one element past the array. Ported as a
   clamp to `LUT_ENV_FREQUENCY.len() - 1` instead.
2. **`PortamentoRateToLPCoefficient` has *no* bounds check at all.**
   `lut_portamento_coefficient[static_cast<int32_t>(rate * 512.0f)]` with a
   completely unclamped index -- `rate >= 1.0` or `rate < 0.0` (both
   reachable the same way as above) reads out of bounds in either
   direction. Ported as a clamp to `[0, LUT_PORTAMENTO_COEFFICIENT.len() -
   1]`.

### Verification

No C bit-compare harness for this module (the C's own `test/stages_test.cc`
only dumps WAV files, with no reference to diff against).
`tests/smoke.rs` reimplements each of its scenarios as a real assertion
using `stmlib::gate_flags::extract_gate_flags`-derived gate streams:
ADSR multi-segment tracking a real gate, a 2-segment `TYPE_HOLD` sequence
(exercising the general multi-segment path, since `Configure`'s
`sequencer_mode` needs `num_segments >= 3` -- with no `TYPE_STEP` segment
present here, every segment's `if_rising` stays the default 0, so *any*
rising edge restarts the pair at segment 0, matching the C exactly), a
single-segment decay envelope, timed pulse generator, gate generator,
sample & hold, portamento settling near its target, free-running LFO,
tap-tempo LFO locking to a clock, clocked delay, the `Zero` function, an
audio-rate oscillator, `ConfigureSlave`/`ProcessSlave` tracking a monitored
master's segment 0, plus standalone checks for `DelayLine16Bits` (matches
the C test's own read-interpolation assertion) and every `Oscillator`
shape.

## Not in scope for the library crate

STM32 peripheral drivers, the audio bootloader, and the `hardware_design/`
files stay in the C repo -- the Rust crate is a `no_std` DSP library that a
host or an embedded HAL feeds.
