# Porting Clouds

**Granular texture synthesizer**  |  MCU: `stm32f4` (Cortex-M4F, hardware FPU)

## Status: ported (time-domain modes) + verified

Clouds runs almost entirely in floating point, so -- like `plaits`, unlike
`braids` -- there is **no fixed-point bit-exactness contract**. The port is
idiomatic Rust; the few integer-exact pieces (phase accumulators, the sign-bit
correlator, mu-law companding) are translated verbatim.

| area | state |
|------|-------|
| `resources` (13 LUTs + 3 pointer tables) | transpiled |
| `AudioBuffer` (16-bit + 8-bit mu-law), `mu_law` | ported |
| `Grain`, `Window`, `Correlator`, `SampleRateConverter` | ported |
| `PlaybackMode::Granular` (`granular_sample_player`) | ported |
| `PlaybackMode::Stretch` (`wsola_sample_player`) | ported |
| `PlaybackMode::LoopingDelay` (`looping_sample_player`) | ported |
| `fx`: `FxEngine`, `Diffuser`, `Reverb`, `PitchShifter` | ported |
| `GranularProcessor` (feedback, SRC, post chain, dry/wet) | ported |
| `PlaybackMode::Spectral` (`pvoc/` + `stmlib` `ShyFFT`) | **not ported** -- silent stub |
| firmware (`cv_scaler`, `ui`, `settings`, persistence, drivers) | out of scope |

### Verification

`tools/clouds_compare.cc` links the C DSP; `examples/clouds_compare.rs` runs the same
deterministic sweep in Rust; `tools/wav_diff.py` diffs the two. As of the port,
of the 12 (mode x quality) dumps:

* **9 are bit-identical** to the C firmware DSP.
* `LoopingDelay` q0/q1 differ by **<= 1 LSB on <= 2 of 96000 samples**
  (`semitones_to_ratio` / `tan` last-bit rounding, then the pitch-shifter).
* `Stretch` q1 (mono) tracks bit-identically for ~1450 of 1500 blocks, then a
  1-ULP difference flips a correlator `xcorr > best_score` comparison and the
  WSOLA splice points diverge -- audible from that point, but "a different
  valid grain", not wrong. The stereo run stays bit-identical throughout.

`tests/equivalence.rs` folds a hash of the Rust dumps into a golden so CI
catches accidental DSP changes without the C toolchain.

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

* **Out-of-bounds table / tail reads are clamped.** `AudioBuffer::read_*`
  (`integral` outside `[0, 2*size)`), `dsp::interpolate` (`table[N+1]` at
  `index == 1.0` for the `N+1`-entry LUTs), and the `AudioBuffer` cross-fade
  tail (`tail_[256]`) are all one-past-the-end reads in the C that land on
  adjacent statics; every one is multiplied by a zero coefficient there, so
  clamping to the last valid entry is identical.

## Rebuilding resources

```
python3 tools/transpile_resources.py \
    ../eurorack/clouds/resources.cc ../eurorack/clouds/resources.h \
    crates/clouds/src/resources.rs
```

`lut_ulaw` lives in `clouds/dsp/mu_law.cc` (not `resources.cc`); it is
hand-copied into `src/mu_law.rs`.

## Finishing the port: Spectral mode

1. Port `stmlib/fft/shy_fft.h` into `mi-stmlib` (`ShyFFT<f32, 4096>` -- the
   `RotationPhasor` variant; header-only, ~450 lines).
2. Port `clouds/dsp/pvoc/{stft,frame_transformation,phase_vocoder}` +
   `stmlib/dsp/atan.h` (`fast_atan2r` + `atan_lut`).
3. Wire `PhaseVocoder` into `GranularProcessor::{prepare, process_granular}`
   in place of the `PlaybackMode::Spectral` stub, and give it the
   `BufferAllocator`-carved buffers the C `Prepare()` computes.
4. Extend `clouds_compare.rs` / `clouds_compare.cc` with the spectral dumps.
