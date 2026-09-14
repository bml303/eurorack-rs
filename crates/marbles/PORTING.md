# Porting Marbles

**Random sampler / CV generator**  |  MCU family: `stm32f3`  |  ~8716 lines of hand-written C (excl. resources & drivers)

## Status: ported

`random/{t_generator,x_y_generator}.rs` (the two top-level engines) plus their
`ramp/*` and `random/*` support are ported and wired up in `src/lib.rs`.
Floating-point, no bit-exactness contract (see "Verification" below).

### Scope

Only the generator engine itself is ported -- `TGenerator`/`XYGenerator` and
what they depend on (`ramp/*`, `random/*`). Per the top-level `CLAUDE.md`
("no peripheral drivers, no bootloader, no `settings`/`ui`"), the following
`marbles/*` files are **not** ported:

- `settings.{cc,h}`, `ui.{cc,h}` -- persisted flash state and the front-panel
  menu system. The `State` struct they hold is not a DSP type -- `marbles.cc`
  reads it every block just to build the `GroupSettings`/`TGenerator` setter
  calls this crate's `process()` already takes directly, the same way
  `mi-plaits`' engines take an already-built `EngineParameters` rather than
  raw ADC codes.
- `cv_reader.{cc,h}`, `cv_reader_channel.h`, `io_buffer.h` -- ADC scaling and
  buffering; a host is expected to supply already-scaled floats.
- `clock_self_patching_detector.h`, `note_filter.h`, `scale_recorder.h` --
  small, self-contained, non-hardware-specific helpers that `marbles.cc` sits
  between the ADC and the generators. Left as future work: none of the
  generator code depends on them, so they can be added later as a small,
  separate increment without touching what's here.
- `drivers/`, the bootloader, `hardware_design/` -- as with every other crate
  in this workspace.

### Verification

`marbles/test/marbles_test.cc` (1134 lines) drives real hardware for audio
capture, not a golden-output comparison, so there is no C bit-compare harness
here (as with `mi-plaits`/`mi-clouds`/`mi-elements`/`mi-rings` before it: this
module is float, not fixed-point). `tests/smoke.rs` instead sweeps every
`TGeneratorModel` x `TGeneratorRange`, both internal and external clock, every
`XYGenerator` `ClockSource` x `ControlMode`, and register mode on/off, over
4000 blocks, asserting the process calls never panic, `TGenerator`'s internal
ramps stay finite, and `XYGenerator`'s final output stays finite and in a
sane voltage range with non-trivial RMS energy (i.e. not silence).

### Deviations from the C++ (beyond the "modernise structure only" rule)

- **Shared PRNG passed explicitly, not stored as a pointer.** The C++'s
  `RandomStream`/`RandomSequence` store a `RandomGenerator*`/`RandomStream*`
  field so several instances can share one generator. In practice there is
  exactly one of each, and `RandomGenerator::Mix()` (the only thing that
  would let a `RandomStream` feed back into a *different* fallback generator)
  is a no-op in the shipped firmware. So `RandomStream` embeds its
  `RandomGenerator` by value (`random_stream.rs`), and `RandomSequence`
  (`random_sequence.rs`), `TGenerator::process`/`init`, `XYGenerator::process`/
  `init`, and `OutputChannel::process` all take `&mut RandomStream` as an
  explicit parameter instead of holding a pointer -- this avoids `no_std`
  interior-mutability/shared-ownership plumbing (`Rc<RefCell<_>>` needs
  `alloc`; raw pointers need `unsafe`) for a distinction that was already
  moot upstream.
- **`RandomSequence`'s three raw pointers into its own `loop_`/`history_`
  arrays are plain indices** (`redo_read_idx: usize`,
  `redo_write_idx`/`redo_write_history_idx: Option<usize>`). This also
  simplifies `clone_from_sequence` (the C++'s `Clone`): a pointer copied
  verbatim from a *different* instance needs an offset translation in C++
  (`&loop_[source.redo_read_ptr_ - &source.loop_[0]]`); an index is already
  position-independent, so it's just a field copy.
- **`Ramps` takes `&mut Ramps<'_>` in both generators**, not `Ramps`/
  `const Ramps&` by value. The C++ passes it as `const Ramps&` in
  `XYGenerator::Process`, but that's shallow const -- the struct's `float*`
  fields are still writable through it (and `XYGenerator::Process` does write
  through `ramps.external`/`ramps.slave[0]`), a distinction that only exists
  because C++ pointers-in-a-const-struct aren't const at the pointee level.
  Rust has no such loophole, and `Ramps<'a>`'s `&'a mut [f32]` fields aren't
  `Copy`, so passing `Ramps` by value would move it out of the caller (unusable
  for the *second* generator's call). `&mut Ramps<'_>` matches the C's actual
  read/write behaviour and lets both `TGenerator::process` and
  `XYGenerator::process` share the same four buffers sequentially, same as
  `marbles.cc`'s `Process()` does.
- **The C++'s "borrow a specific `Ramps` field, then separately mutate a
  different field" pattern in `XYGenerator::process`** (select
  `ramps.master`/`ramps.slave[0]`/`ramps.slave[1]` as the divider's input
  while writing `ramps.external` as its output) is written with the field
  match arms inlined at each call site rather than through a small selector
  function taking `&Ramps` -- a function boundary would make the borrow
  checker see "borrows all of `*ramps`" instead of "borrows one field",
  which conflicts with the sibling mutable borrow of `ramps.external` even
  though the two are always disjoint fields at runtime.
- **Two latent out-of-bounds reads in the C++, both hit by this port's own
  smoke test, both clamped in-bounds here** (the C reads adjacent static
  data in these cases -- harmless in practice there, but Rust panics on an
  out-of-bounds slice index):
  - `LagProcessor::process`'s `Interpolate(lut_raised_cosine, phase, 256.0f)`
    reads one past the 257-entry table when `phase` is exactly `1.0` -- which
    a `SlaveRamp` in Bernoulli mode can legitimately produce (it clamps its
    output phase to `1.0` while holding a gate). Fixed the same way
    `mi-rings`/`mi-elements` fix the identical `Interpolate` pattern: clamp
    the `+1` read to the table's last index (`lag_processor.rs`; the same
    clamped helper is duplicated defensively in `distributions.rs`, though no
    caller there can actually reach the edge).
  - `Quantizer::Process` reads `voltage_[l.first]` where `l.first` can still
    be its `Init`-time sentinel `0xff` if no scale degree meets a given
    threshold level's weight cutoff -- reachable with a low-weight scale,
    including this crate's own default `Scale` (a single degree of weight
    `0`). Clamped to `self.num_degrees - 1` in `quantizer.rs`.
- **`DiscreteDistribution<size>`'s const generic is `size + 2`, already
  applied** (`DiscreteDistribution<18>` for the sole instantiation,
  `kMaxDegrees == 16`), matching `stmlib::PatternPredictor`'s existing trick
  for the same "the array is `N` plus a fixed pad, and Rust const generics
  can't express that arithmetic on stable" situation.
- **`ParameterInterpolator` isn't used in `OutputChannel::process`** even
  though the C++ uses `stmlib::ParameterInterpolator` there for the `steps`
  ramp: `ParameterInterpolator` borrows `&mut self.previous_steps` for its
  whole scope (it writes back on `Drop`), which would coexist with several
  other `&mut self` calls (`self.quantize(...)`, `self.generate_new_voltage(...)`)
  inside the same loop -- not something the borrow checker allows for a
  field already mutably borrowed by a live guard object. The ramp is inlined
  by hand instead (a plain `f32` step accumulator, written back to
  `self.previous_steps` after the loop), producing the identical sequence of
  values.

## Source inventory (DSP + UI, drivers/bootloader/resources excluded)

| file | lines | ported? |
|------|-------|---------|
| `clock_self_patching_detector.h` | 87 | no (future work, see Scope) |
| `cv_reader.cc` | 117 | no (out of scope, see Scope) |
| `cv_reader.h` | 133 | no |
| `cv_reader_channel.h` | 225 | no |
| `io_buffer.h` | 110 | no |
| `marbles.cc` | 469 | no (app-level `Process()`/`Init()` glue; its DSP-relevant logic is inlined into what a host is expected to do around `TGenerator`/`XYGenerator`) |
| `note_filter.h` | 82 | no (future work) |
| `scale_recorder.h` | 149 | no (future work) |
| `settings.cc` | 249 | no (out of scope) |
| `settings.h` | 163 | no |
| `ui.cc` | 590 | no (out of scope) |
| `ui.h` | 148 | no |
| `ramp/ramp.h` | 40 | yes -- `ramp/mod.rs` (`MAX_RAMP_VALUE`) |
| `ramp/ramp_divider.h` | 116 | yes -- `ramp/ramp_divider.rs` (`Ratio`, `RampDivider`) |
| `ramp/ramp_extractor.cc` | 333 | yes -- `ramp/ramp_extractor.rs` |
| `ramp/ramp_extractor.h` | 148 | yes |
| `ramp/ramp_generator.h` | 63 | yes -- `ramp/ramp_generator.rs` |
| `ramp/slave_ramp.h` | 137 | yes -- `ramp/slave_ramp.rs` |
| `test/fixtures.h` | 233 | no (hardware test harness; see `tests/smoke.rs` instead) |
| `test/marbles_test.cc` | 1134 | no |
| `test/ramp_checker.h` | 84 | no |
| `random/discrete_distribution_quantizer.cc` | 115 | yes -- `random/discrete_distribution_quantizer.rs` |
| `random/discrete_distribution_quantizer.h` | 75 | yes |
| `random/distributions.h` | 182 | yes -- `random/distributions.rs` |
| `random/lag_processor.cc` | 88 | yes -- `random/lag_processor.rs` |
| `random/lag_processor.h` | 61 | yes |
| `random/output_channel.cc` | 145 | yes -- `random/output_channel.rs` |
| `random/output_channel.h` | 139 | yes |
| `random/quantizer.cc` | 138 | yes -- `random/quantizer.rs` (`Scale`, `Degree`, `Quantizer`) |
| `random/quantizer.h` | 122 | yes |
| `random/random_generator.h` | 65 | yes -- `random/random_generator.rs` |
| `random/random_sequence.h` | 268 | yes -- `random/random_sequence.rs` |
| `random/random_stream.h` | 82 | yes -- `random/random_stream.rs` (embeds `RandomGenerator`, see Deviations) |
| `random/t_generator.cc` | 429 | yes -- `random/t_generator.rs` |
| `random/t_generator.h` | 217 | yes (also defines `Ramps`, shared with `x_y_generator.rs`) |
| `random/x_y_generator.cc` | 207 | yes -- `random/x_y_generator.rs` |
| `random/x_y_generator.h` | 146 | yes (`GroupSettings`, `VoltageRange`, `ClockSource`, `ControlMode`; `OutputGroup` dropped -- declared but never referenced anywhere in the C++ either) |
| `drivers/*` | ~1122 | no (out of scope) |

## Resources

`marbles/resources.cc` (4767 lines of generated lookup tables) transpiled
with `tools/transpile_resources.py` into `src/resources.rs`, exactly as for
`braids`.

## Also added to `mi-stmlib`

`stmlib::RingBuffer<T, const N: usize>` (`ring_buffer.rs`) -- a minimal port
of `stmlib::RingBuffer` for `RandomStream`'s hardware-RNG buffer, covering
only the non-blocking subset this port needs (`init`, `writable`/`readable`,
`overwrite`, `immediate_read` on single elements; not the spin-waiting
`Write`/`Read` or the bulk-slice variants).

## Not in scope for the library crate

STM32/AVR peripheral drivers (`drivers/`), the audio bootloader, and the
`hardware_design/` files stay in the C repo -- the Rust crate is a `no_std`
DSP library that a host or an embedded HAL feeds.
