# Peaks -- port status

**Dual function generator (envelopes, LFOs, drums)**  |  MCU family: `stm32f2`

## Status: PORTED (fixed-point, follows the shipping firmware)

Peaks is a 32-bit fixed-point STM32F2 module built around one shared
shape: 12 independent "processor functions" (envelope, LFO/tap-LFO, 4 drum
voices, 2 pulse processors, bouncing ball, mini sequencer, number station),
each turning a per-sample [`GateFlags`] stream into an `i16` output stream.
This is the largest port in this workspace so far -- 20 source files behind
one dispatcher, several sharing state (`Lfo` doubles as both `Lfo` and
`TapLfo`) or a common building block (`Svf`, `Excitation`).

### Modules

| Rust file | C source | notes |
|-----------|----------|-------|
| `src/resources.rs` | `resources.{cc,h}` | transpiled directly, no preprocessing needed |
| `src/gate_processor.rs` | `gate_processor.h` | `ControlMode`; `GateFlags`/`extract_gate_flags` re-exported from `mi-stmlib` |
| `src/calibration_data.rs` | `calibration_data.{h,cc}` | signed sample -> DAC code, with a per-channel offset |
| `src/drums/svf.rs` | `drums/svf.h` | the drum engines' SVF, with an added "punch" parameter vs. `mi-streams`' `Svf` |
| `src/drums/excitation.rs` | `drums/excitation.h` | triggerable delayed exponential-decay impulse |
| `src/drums/bass_drum.rs` | `drums/bass_drum.{h,cc}` | 808-style bass drum |
| `src/drums/snare_drum.rs` | `drums/snare_drum.{h,cc}` | 808-style snare drum |
| `src/drums/high_hat.rs` | `drums/high_hat.{h,cc}` | 808-style hi-hat (no configurable parameters) |
| `src/drums/fm_drum.rs` | `drums/fm_drum.{h,cc}` | sine-FM drum (BD/SD-style, Anushri-inspired) |
| `src/modulations/multistage_envelope.rs` | `modulations/multistage_envelope.{h,cc}` | AD/AR/ADSR/ADSAR/ADAR envelope, optional looping |
| `src/modulations/lfo.rs` | `modulations/lfo.{h,cc}` | 5-shape LFO, tap-tempo sync via `stmlib::PatternPredictor` |
| `src/modulations/bouncing_ball.rs` | `modulations/bouncing_ball.h` | bouncing-ball physical simulation |
| `src/modulations/mini_sequencer.rs` | `modulations/mini_sequencer.h` | 2/4-step CV sequencer |
| `src/pulse_processor/pulse_shaper.rs` | `pulse_processor/pulse_shaper.{h,cc}` | trigger-to-gate with pre-delay/duration/repetitions |
| `src/pulse_processor/pulse_randomizer.rs` | `pulse_processor/pulse_randomizer.{h,cc}` | randomly accepts/repeats triggers |
| `src/number_station.rs` | `number_station/number_station.{h,cc}` | shortwave "number station" generator |
| `src/processors.rs` | `processors.{h,cc}` | `Processors`: dispatches to one of the 12 functions above |

Out of scope: `peaks.cc` (the app-level ADC/DAC/UI main loop), `ui.{h,cc}`
(the UI state machine), `io_buffer.h` (the hardware ISR double-buffering
scheme -- a host just calls `Processors::process` with whatever block size
it likes), the STM32 peripheral drivers, and flash persistence
(`CalibrationData::Save`).

`mi-stmlib`'s `fixed::{interpolate_824_u16, interpolate_88_u16,
interpolate_1022, mix_i16}`, `random::Random`, and `pattern_predictor::
PatternPredictor` (already ported for `mi-tides`/`mi-peaks`'s eventual LFO)
covered everything this port needed; the only shared-type change was adding
`GateFlags::FROM_BUTTON`/`AUXILIARY_{LOW,HIGH,RISING,FALLING}` (Peaks-only
bits `mi-grids`/`mi-tides`/etc. don't use) to `stmlib::gate_flags::GateFlags`.

### A cross-cutting contract: several engines assume `kBlockSize == 4`

The firmware always calls `Process` in blocks of exactly 4 samples
(`peaks::kBlockSize`, from the now-out-of-scope `IOBuffer`). A few engines
bake that block size into their own timing rather than tracking it
independently -- documented on each engine's own module doc comment, and
summarized on the crate root:

* **`FmDrum`** recomputes its FM phase increment every 4th sample, keyed
  off the *remaining* sample count in a `while (size--)`-style loop (`size
  & 3 == 0`), not an independent counter. Ported as `remaining = size - 1 -
  n` inside the loop, exactly reproducing what the C's `size` holds at that
  point.
* **`NumberStation`** downsamples its control-rate processing by exactly 4
  (`kDownsample`) and always emits 4 output samples per tick
  (`size /= kDownsample` in the C -- if the caller's block isn't a multiple
  of 4, the trailing `size % 4` samples are simply never written, in the
  Rust exactly as in the C++).
* **`PulseShaper`**/**`PulseRandomizer`** look for a rising edge anywhere
  in the whole call and fill the *entire* output block with one flat value,
  so their time resolution is exactly the caller's block size.

None of this panics or misbehaves at other block sizes -- it just means
bit-for-bit fidelity to the shipped firmware requires calling `process` in
blocks of 4 (or a multiple of 4), same as the real hardware does.

### Deviations / design notes

* `Processors` dispatches via `match` on [`processors::ProcessorFunction`]
  instead of the C's `ProcessorCallbacks` function-pointer table.
  `TapLfo` isn't a separate engine -- it's the same `Lfo` instance with
  `set_sync(true)`, matching the C's `lfo_.set_sync(function ==
  PROCESSOR_FUNCTION_TAP_LFO)` and its skip of `LfoInit()` on that specific
  transition (switching in and out of tap-sync mode doesn't reset the
  LFO's phase/pattern-predictor state).
* `Processors::default()`'s `parameter` array is `[0; 4]`, not `[32768;
  4]`. The C's `processors[2]` is a zero-initialized static array, and
  `Init()` triggers its first `Configure()` pass (via `set_function
  (PROCESSOR_FUNCTION_ENVELOPE)`) *before* filling `parameter_` with
  32768 -- so that first configuration genuinely runs on zeros, and
  `parameter_` only becomes 32768 afterward with no further reconfigure.
  Defaulting to `[32768; 4]` instead would have made a freshly-constructed
  `Processors` "look right" without ever having actually run `init()`,
  which is the wrong thing to fix quietly -- the whole point of a fixed-
  point port is that this kind of construction-order quirk is exactly what
  it's supposed to preserve.
* `MultistageEnvelope`'s `level`/`shape` arrays only ever get 5 of their 8
  slots configured by any of the shipped presets (`set_adsr` et al., which
  top out at 4 segments). Like `mi-streams::Envelope`, `Process` reads
  `level[segment + 1]` with `segment` ranging up to `num_segments` itself,
  which is the same latent constraint the C++'s 8-slot array capacity
  already implies for any hypothetical `num_segments > 6` configuration --
  not reachable through `Processors`, so not defensively clamped.

### Two real subtleties this port's fidelity check caught

1. **`Excitation::process`'s decay multiply is a 32-bit *unsigned* wrap,
   not a widening one.** `state_ * decay_` is `int32_t * uint32_t` in the
   C++ -- the usual arithmetic conversions make that a plain 32-bit
   unsigned multiply (wrapping at 2^32), *not* promoted to 64-bit the way
   `mi-streams::Vactrol`'s `static_cast<int64_t>(...)`-guarded multiplies
   are. An early draft used `i64` headroom here by pattern-matching
   "coefficient multiply -> probably needs 64-bit like the other drum/
   dynamics engines" without checking for an explicit cast -- fixed to
   `(state as u32).wrapping_mul(decay) >> 12` once cross-referenced against
   the header. For the magnitudes these voices actually reach the two
   versions happen to agree, but only the corrected one is *provably*
   right for any input. Worth remembering as a recurring trap: 64-bit
   arithmetic in this codebase is never implicit, always an explicit
   `int64_t`/`static_cast<int64_t>` in the source -- check for it rather
   than assuming from a coefficient's name.
2. **`FmDrum::compute_phase_increment`'s parameter is `int16_t`, and the
   caller's running sum genuinely narrows into it (with sign
   reinterpretation), not just nominally.** The sum feeding it
   (`frequency_ + fm_envelope*fm_amount_>>16 + aux_envelope*
   aux_envelope_strength_>>15 + previous_sample_>>6`) is `uint32_t`-typed
   throughout its own evaluation, easily exceeding `int16_t`'s range in
   practice -- ported by computing the sum as `u32` and casting once, `as
   i16`, at the call site, so `compute_phase_increment` itself keeps the
   C's exact `i16` signature rather than being widened to `i32` "to avoid
   the truncation" (which would silently change behavior for any sum that
   doesn't already fit `i16`).

### Verification

No C bit-compare harness for this module. `tests/smoke.rs`: `Processors`
across a 20,000-step sweep of every function/control-mode/parameter
combination (called in blocks of 4, matching the firmware); each engine
exercised individually via `stmlib::gate_flags::extract_gate_flags`-derived
gate streams (not hand-built raw bit patterns) -- `MultistageEnvelope`
triggering and releasing through a real `GATE_FLAG_FALLING` edge, `Lfo`
across every shape in both sync/unsynced modes (an unsynced LFO needs a
*sparse* trigger pattern in the test to let it free-run instead of
constantly resetting its phase -- a synced one needs the opposite, a
periodic clock to lock its pattern predictor onto), all 4 drum voices for
audible output, `BouncingBall`/`MiniSequencer`/`PulseShaper`/
`PulseRandomizer`/`NumberStation` for liveness, and `CalibrationData`'s DAC
code conversion.

## Not in scope for the library crate

STM32/AVR peripheral drivers, the audio bootloader, and the
`hardware_design/` files stay in the C repo -- the Rust crate is a `no_std`
DSP library that a host or an embedded HAL feeds.
