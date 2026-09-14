# Frames -- port status

**Keyframer / mixer**  |  MCU family: `stm32f1`

## Status: PORTED (fixed-point, follows the shipping firmware)

Frames is a 32-bit fixed-point STM32F1 module, ported `mi-braids`-style:
verbatim `wrapping_*` int arithmetic wherever the C relies on 32-bit
overflow or narrowing-conversion truncation.

### Modules

| Rust file | C source | notes |
|-----------|----------|-------|
| `src/resources.rs` | `resources.{cc,h}` | transpiled directly, no preprocessing needed |
| `src/keyframer.rs` | `keyframer.{h,cc}` | `Keyframer`: sorted keyframe list, `Easing`, VCA-response `ConvertToDacCode` |
| `src/poly_lfo.rs` | `poly_lfo.{h,cc}` | `PolyLfo`: 4-channel wavetable LFO easter egg |

Out of scope: `frames.cc` (the app-level ADC/DAC/UI main loop), `ui.{h,cc}`
(the UI state machine), the STM32 peripheral drivers, and flash persistence
(`Keyframer::Save`'s `stmlib::Storage` write -- `set_extra_settings`/
`calibrate` update the in-memory fields without it, matching every other
crate's dropped settings layer).

`mi-stmlib::fixed::crossfade_u8`/`interpolate_824_u8` (already ported for
other crates) cover everything `poly_lfo.cc` needs from `stmlib/utils/
dsp.h` -- no new shared primitives were required for this port.

### Deviations from the C++

* `Keyframer`/`PolyLfo` are ordinary instances, not `static` globals,
  matching every other crate in this workspace.
* `Keyframer::find_keyframe` (the C's `std::lower_bound` over the keyframe
  array) is `[Keyframe]::partition_point` -- exactly the same binary search,
  expressed with a `bool`-returning predicate instead of a comparator
  struct.

### Three real out-of-bounds reads this port's fidelity check caught

All three share a shape: `find_keyframe`'s `std::lower_bound` legitimately
returns `num_keyframes()` (one past the last keyframe) whenever the queried
timestamp is past every existing keyframe, and three call sites then read
`keyframes_[num_keyframes_]` unconditionally -- fine on real hardware
(reads whatever static memory follows the array) but exactly a 64-element
array's one-past-the-end index once the keyframe list is completely full
(`kMaxNumKeyframe == 64`), which Rust's bounds checking turns into a panic:

1. **`Keyframer::evaluate`**, the `nearest_keyframe_` computation
   (`keyframes_[position].timestamp`, executed unconditionally after the
   `if (position == 0 || position == num_keyframes_)` branch that handles
   the *values*/*color* correctly already). Fixed by only doing the
   "is there a closer keyframe after this one" comparison when `position <
   num_keyframes()`; when it isn't, there's no keyframe after the last one,
   so `nearest_keyframe` is just `position` -- the comparison the C would
   have made against garbage memory isn't needed at all.
2. **`Keyframer::remove_keyframe`**'s `keyframes_[splice_point].timestamp !=
   timestamp` check. `find_keyframe` landing at `num_keyframes()` already
   means "no keyframe has this timestamp" by construction, so this port
   returns `false` before the read instead of after it.
3. **`Keyframer::easing`**'s lookup-table branch (`EASING_CURVE_IN_QUARTIC`
   and above): `scale` (the interpolation position, `0..=0x10000` in 16.16
   fixed point) can legitimately be **exactly** `0x10000` -- reached every
   time `evaluate` is called with a timestamp landing precisely on an
   interior keyframe's own timestamp, an entirely ordinary occurrence, not
   an edge case a caller has to contrive. `scale >> 6` is then `1024`, the
   table's true last valid index (length 1025) -- reading it for `scale_a`
   is correct and required, not something to clamp away. It's `scale_a`'s
   *neighbour*, `table[scale >> 6 + 1]` (`= table[1025]`), that's out of
   bounds; the C reads that byte too, but the fractional blend weight
   applied to it is provably zero at this exact boundary (`(scale << 10) &
   0xffff == 0` when `scale == 0x10000`), so its value never affects the
   result. This port clamps only that second index (reusing the last valid
   entry, which is multiplied by zero anyway) and leaves the first
   unclamped -- an earlier draft of this fix clamped both indices together
   and silently read the *wrong* entry for `scale_a`, caught by a smoke
   test asserting the eased value at a keyframe boundary lands close to
   that keyframe's own value.

### Verification

No C bit-compare harness for this module. `tests/smoke.rs`: a full
64-keyframe list evaluated past the last keyframe (exercises finding #1
above) and swept across the whole tail region; evaluating exactly at every
interior keyframe's own timestamp across every lookup-table easing curve
(exercises finding #3, asserting the result lands within a few DAC counts
of that keyframe's value); removing past the end of a full list
(exercises finding #2); a 5000-step add/remove/evaluate/find-nearest/
sample-animation sweep across every easing curve and response; the empty-
keyframer immediate-value fallback; `convert_to_dac_code`'s range; and a
long `PolyLfo` sweep across every shape/spread/coupling combination
checked for life (output actually varies) as well as survival.

## Not in scope for the library crate

STM32/AVR peripheral drivers, the audio bootloader, and the
`hardware_design/` files stay in the C repo -- the Rust crate is a `no_std`
DSP library that a host or an embedded HAL feeds.
