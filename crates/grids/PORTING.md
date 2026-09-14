# Grids -- port status

**Topographic drum sequencer**  |  MCU family: `avr`

## Status: PORTED (fixed-point, follows the shipping firmware)

Grids is an 8-bit AVR module, so the arithmetic is reproduced verbatim like
`mi-braids`/`mi-edges`: `u8`/`u16` wrap, the exact `U8Mix`/`U8U8MulShift8`
multiply-mix helpers, the exact 16-bit Galois LFSR recurrence, and the 25
hand-recorded drum density maps transpiled unchanged from `PROGMEM`.

### Modules

| Rust file | C source | notes |
|-----------|----------|-------|
| `src/resources.rs` | `resources.{cc,h}` | transpiled after a `sed` preprocessing pass (see below) |
| `src/random.rs` | `avrlib/random.h` | the 16-bit Galois LFSR (`avrlib::Random`), kept crate-local |
| `src/clock.rs` | `clock.{h,cc}` | the global tempo/swing phase accumulator |
| `src/pattern_generator.rs` | `pattern_generator.{h,cc}` | the sequencer proper: drum-map lookup + Euclidean generator |

Out of scope: `grids.cc` (the app-level ADC/DAC/UI/shift-register main
loop), `hardware_config.h`'s AVR GPIO/SPI/serial typedefs (only its LED bit
constants are kept, as `pattern_generator::led_bits`), and
`PatternGenerator::LoadSettings`/`SaveSettings` (raw AVR EEPROM I/O) -- like
every other crate's persistence layer, dropped entirely; `init()` just
resets sequencer state, and a host supplies `Options`/`PatternGeneratorSettings`
however it obtains them (its own storage, `Options::unpack` on a stored byte,
or setting fields directly).

### Resources: the AVR `PROGMEM` preprocessing step

`tools/transpile_resources.py` doesn't understand the AVR `prog_uint8_t` /
`prog_uint16_t` / `prog_uint32_t` / `prog_char` / `PROGMEM` macros grids'
`resources.cc`/`.h` use (it produced "0 data arrays, 0 pointer tables"
against the raw source). Same fix as `mi-edges`: preprocess a copy with

```
sed -e 's/prog_uint8_t/uint8_t/g' -e 's/prog_uint16_t/uint16_t/g' \
    -e 's/prog_uint32_t/uint32_t/g' -e 's/prog_char/char/g' -e 's/PROGMEM//g'
```

before transpiling. Result: 27 data arrays + 2 pointer tables (one skipped,
`lookup_table_table`, a mixed/unknown-element-type table that's irrelevant
here). Notably `LUT_RES_EUCLIDEAN: [u32; 1024]`, `LUT_RES_TEMPO_PHASE_INCREMENT:
[u32; 512]`, and `NODE_0..NODE_24: [u8; 96]` (the 25 drum density maps),
plus an auto-generated `NODE_TABLE: [&[u8]; 25]` (unused -- `pattern_generator.rs`
hand-writes the non-sequential `drum_map[5][5]` arrangement directly, see below).

### Deviations from the C++

* Every member of `PatternGenerator`/`Clock`/`Options`/etc. is `static` in
  the C++ (one hardwired global instance, the usual AVR-firmware idiom).
  Ported as a normal instance -- `PatternGenerator::new()`/`Clock::new()` +
  ordinary `&mut self` methods -- matching every other crate in this
  workspace.
* `Options` (`PatternGeneratorOptions` in the Rust) is a C `union` of
  `DrumsSettings` and `[u8; kNumParts]` (euclidean lengths), aliased in
  memory to save the few bytes that matters on an AVR but not here. Ported
  as a plain struct with both fields present (`drums: DrumsSettings`,
  `euclidean_length: [u8; NUM_PARTS]`) rather than an actual union, avoiding
  `unsafe`/C-union aliasing rules for a memory saving that doesn't matter on
  a host. (Note: the C++'s outer `PatternGeneratorSettings::options_` field
  and the separate bit-packed `Options` "clock resolution/output mode/swing/
  etc." struct share the same name `Options` in the C++ source across two
  different types -- disambiguated here as `PatternGeneratorOptions` (the
  union-turned-struct) vs. `Options` (the packed byte, `pack()`/`unpack()`).)
* `Clock::Wrap`/`past_falling_edge` reinterpret `phase_` (a `uint32_t`) as
  an `avrlib::LongWord` union to read/write just its top byte (`bytes[3]`,
  the most significant byte on AVR's little-endian layout). Ported as plain
  shifts/masks on the `u32` directly (`(phase >> 24) as u8`, `phase &=
  0x7fff_ffff`, `phase &= 0x00ff_ffff`) -- the identical operation, no union
  or `unsafe` needed.
* `avrlib/op.h`'s `U8Mix`/`U8U8MulShift8` are AVR inline assembly with a
  portable C fallback; this port uses the portable formula directly
  (`u8_mix`/`u8_u8_mul_shift8` in `pattern_generator.rs`), same approach as
  `mi-edges`' portable-path decision.
* `avrlib/random.h`'s LFSR is kept crate-local (`src/random.rs`) rather than
  folded into `mi-stmlib`, following the precedent that `mi-edges` already
  inlines the identical recurrence directly at its one call site rather than
  factoring it out -- a second AVR crate needing the same 6-line recurrence
  doesn't yet justify a shared module.

### Verification

No C bit-compare harness (no host-buildable C test for this module).
`tests/smoke.rs`:
* `PatternGenerator` across both output modes, every clock resolution,
  swing on/off, gate mode on/off, drum and Euclidean settings, and a
  20,000-tick sweep -- asserts no panic, `led_pattern()` stays within the
  documented `BD|SD|HH` bits, `step()` stays within `STEPS_PER_PATTERN`, and
  the sequencer actually produces a varying, non-silent rhythm (not stuck at
  one state, not permanently silent).
* `Options::pack`/`unpack` round-trips through all 256 byte values.
* `Clock` across every entry of the 512-BPM tempo table and all 3
  resolutions, exercising `tick`/`wrap`/`raising_edge`/`past_falling_edge`;
  plus a sanity check that 24 PPQN cycles faster than 8 PPQN at the same BPM.

### A real bug this port's fidelity check caught

None found in this module -- unlike Plaits' speech engine or Marbles'
`LagProcessor`/`Quantizer`, grids' fixed-point index arithmetic (`drum_map`,
`LUT_RES_EUCLIDEAN`) stays within its tables for every reachable input, so
no latent out-of-bounds read was exposed by Rust's bounds checking.

## Not in scope for the library crate

STM32/AVR peripheral drivers, the audio bootloader, and the
`hardware_design/` files stay in the C repo -- the Rust crate is a `no_std`
DSP library that a host or an embedded HAL feeds.
