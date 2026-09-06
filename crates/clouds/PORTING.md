# Porting Clouds

**Granular texture synthesizer**  |  MCU: `stm32f4` (Cortex-M4F, hardware FPU)

## Status: fully ported + verified

Clouds runs almost entirely in floating point, so -- like `plaits`, unlike
`braids` -- there is **no fixed-point bit-exactness contract**. The port is
idiomatic Rust; the integer-exact pieces (phase accumulators, the sign-bit
correlator, mu-law companding, the `ShyFft` and phase words) are translated
verbatim.

| area | state |
|------|-------|
| `resources` (13 LUTs + 3 pointer tables) | transpiled |
| `AudioBuffer` (16-bit + 8-bit mu-law), `mu_law` | ported |
| `Grain`, `Window`, `Correlator`, `SampleRateConverter` | ported |
| `PlaybackMode::Granular` (`granular_sample_player`) | ported |
| `PlaybackMode::Stretch` (`wsola_sample_player`) | ported |
| `PlaybackMode::LoopingDelay` (`looping_sample_player`) | ported |
| `PlaybackMode::Spectral` (`pvoc/{stft,frame_transformation,phase_vocoder}`) | ported |
| `stmlib::fft::ShyFft` (`RotationPhasor`), `stmlib::atan` | ported into `mi-stmlib` |
| `fx`: `FxEngine`, `Diffuser`, `Reverb`, `PitchShifter` | ported |
| `GranularProcessor` (feedback, SRC, post chain, dry/wet) | ported |
| firmware (`cv_scaler`, `ui`, `settings`, persistence, drivers) | out of scope |

### Verification

`tools/clouds_compare.cc` links the C DSP; `examples/clouds_compare.rs` runs
the same deterministic sweep in Rust; `tools/wav_diff.py` diffs the two. Of
the 16 (mode x quality) dumps:

* **13 are bit-identical** to the C firmware DSP -- every Granular and every
  Spectral render, plus stereo Stretch and LoopingDelay q2/q3.
* `LoopingDelay` q0/q1 differ by **<= 1 LSB on <= 2 of 96000 samples**
  (`semitones_to_ratio` / `tan` last-bit rounding, then the pitch-shifter).
* `Stretch` q1 (mono) tracks bit-identically for ~1450 of 1500 blocks, then a
  1-ULP difference flips a correlator `xcorr > best_score` comparison and the
  WSOLA splice points diverge -- audible from that point, but "a different
  valid grain", not wrong. The stereo run stays bit-identical throughout.

`tests/equivalence.rs` folds a hash of the 16 Rust dumps into a golden so CI
catches accidental DSP changes without the C toolchain. `mi-stmlib` has a
`ShyFft` round-trip test.

### Deviations from the C

* **`Window::Start` restores `done_ = false`.** Upstream Clouds had this
  assignment *twice* in `clouds/dsp/window.h`; two near-simultaneous "remove
  duplicate assignment" commits (`fbb53ba`, `0e3756f`) merged in `d1d8839`
  (March 2023) removed **both** copies. Without it a freshly `Start`ed window
  is permanently `done()` and Stretch mode is silent in any host build
  (`clouds_test.cc` only exercises Granular / LoopingDelay). The port
  reinstates the assignment; `clouds_compare.cc` needs the same one-line fix
  to produce a non-silent Stretch reference:

  ```
  # in ../eurorack/clouds/dsp/window.h, Window::Start(), after `phase_ = 0;`
  +    done_ = false;
  ```

* **The `Correlator` and `PitchShifter` get separate buffers.** The firmware
  aliases them into one region of the `BufferAllocator` workspace (they run in
  mutually exclusive modes). The port gives each its own owned buffer -- a RAM
  optimisation with no audible effect.

* **The phase-vocoder buffers are owned, not carved from a `void*` slab.**
  `PhaseVocoder::init` reproduces the firmware `BufferAllocator` texture-count
  arithmetic (7 textures mono, 3 stereo) from the fixed slab sizes, then
  allocates each STFT / `FrameTransformation` buffer directly. The FFT scratch
  is a `Vec<f32>` pair; the C's `float* <-> uint32_t*` phase-word pun becomes
  `f32::{from,to}_bits` on those slots, which is bit-identical.

* **Out-of-bounds table / tail reads are clamped.** `AudioBuffer::read_*`
  (`integral` outside `[0, 2*size)`), `dsp::interpolate` (`table[N+1]` at
  `index == 1.0` for the `N+1`-entry LUTs), `dsp::interpolate_raw` (warp
  indices past the spectrum), the `AudioBuffer` cross-fade tail (`tail_[256]`),
  and the STFT circular index (spill past `buffer_size` for non-32 blocks) are
  all one-past-the-end reads in the C that land on adjacent memory; each is
  multiplied by a zero coefficient (or never happens for 32-sample blocks), so
  clamping / wrapping is identical.

## Rebuilding resources

```
python3 tools/transpile_resources.py \
    ../eurorack/clouds/resources.cc ../eurorack/clouds/resources.h \
    crates/clouds/src/resources.rs
```

`lut_ulaw` lives in `clouds/dsp/mu_law.cc` (not `resources.cc`); it is
hand-copied into `src/mu_law.rs`. `atan_lut` is hand-copied into
`crates/stmlib/src/atan.rs`.
