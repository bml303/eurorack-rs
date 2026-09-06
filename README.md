# eurorack-rs

A Rust workspace porting the DSP of [Mutable Instruments' Eurorack
modules](https://github.com/pichenettes/eurorack) to `no_std` library crates.

## Status

| crate            | what it is                                   | state |
|------------------|----------------------------------------------|-------|
| `mi-stmlib`      | shared DSP library (`stmlib`)                | **ported** — primitives Braids/Plaits need, plus generally-useful helpers; tested |
| `mi-braids`      | Braids macro-oscillator (~48 models)         | **ported — reference crate**; 47/48 models verified **bit-identical** to the C firmware DSP |
| `mi-plaits`      | Plaits macro-oscillator (24 models)          | **ported**, floating-point (no bit-exactness contract); 24 models working |
| `mi-clouds`      | Clouds granular texture synthesizer          | **ported**, floating-point; Granular / Stretch / Looping-Delay + all FX; 9/12 (mode×quality) dumps bit-identical to the C, 2 more within 1 LSB. Spectral mode is a documented silent stub |
| `mi-branches` … `mi-yarns` (13 more) | one crate per remaining module | **scaffold** — `Cargo.toml` + `lib.rs` + a per-crate `PORTING.md` source inventory |

`braids` is the worked example every fixed-point module port should follow;
`plaits` is the worked example for a floating-point module (no bit-exactness
contract — see its own `PORTING.md`). See [`PORTING.md`](PORTING.md) for the
method, the fidelity contract, and the verification workflow.

## Layout

```
crates/
  stmlib/            mi-stmlib   — fixed & float DSP, ParameterInterpolator,
                                   CosineOscillator, Random, units, gate flags
  braids/            mi-braids   — analog_oscillator, digital_oscillator,
                                   macro_oscillator, quantizer, svf, excitation,
                                   envelope, resources (transpiled tables)
  plaits/            mi-plaits   — voice (24 engine slots), oscillator, noise,
                                   fx, physical_modelling, chords, drums,
                                   envelope, resources (transpiled tables)
  clouds/            mi-clouds   — granular_processor, granular/wsola/looping
                                   players, grain, window, correlator, fx
                                   (diffuser, reverb, pitch_shifter), audio_buffer,
                                   mu_law, resources (transpiled tables)
  <module>/          mi-<module> — scaffold + PORTING.md
tools/
  transpile_resources.py   C `resources.cc` -> Rust `static` arrays
  braids_compare.cc        reference renderer (links the C firmware DSP)
  clouds_compare.cc        same, for clouds
  wav_diff.py              diff two trees of raw-PCM / WAV dumps
```

## Build & test

```
cargo build --workspace
cargo test  --workspace          # Braids + Clouds equivalence goldens, Plaits/Clouds smoke tests
cargo clippy --workspace

# render 5 s of one Braids model to a WAV
cargo run --release --example render_wav -p mi-braids -- saw_square
# 8 s of one Clouds mode (granular | stretch | looping | spectral)
cargo run --release --example clouds_wav -p mi-clouds -- granular
```

## Verifying `braids` against the C

Requires the C repo next door with submodules checked out
(`git -C ../eurorack submodule update --init`):

```
# 1. reference renders from the actual firmware DSP
cd ../eurorack
g++ -O2 -DTEST -I. -o /tmp/braids_compare \
    ../eurorack-rs/tools/braids_compare.cc \
    braids/analog_oscillator.cc braids/digital_oscillator.cc \
    braids/macro_oscillator.cc braids/resources.cc stmlib/utils/random.cc
mkdir -p /tmp/c_pcm && /tmp/braids_compare /tmp/c_pcm

# 2. matching Rust renders
cd ../eurorack-rs
cargo run --release --example compare -p mi-braids -- /tmp/rust_pcm

# 3. diff
python3 tools/wav_diff.py /tmp/c_pcm /tmp/rust_pcm
```

47 of 48 macro shapes are byte-identical. The one exception (`WaveLine` at
maximum timbre) is a spot where the C itself does a layout-dependent
out-of-bounds table read — see [`PORTING.md`](PORTING.md).

## Verifying `clouds` against the C

```
cd ../eurorack
# NOTE: apply the one-line Stretch fix first — see crates/clouds/PORTING.md
#   clouds/dsp/window.h, Window::Start(), add `done_ = false;` after `phase_ = 0;`
g++ -O2 -DTEST -I. -Istmlib -o /tmp/clouds_compare \
    ../eurorack-rs/tools/clouds_compare.cc \
    clouds/dsp/granular_processor.cc clouds/dsp/correlator.cc clouds/dsp/mu_law.cc \
    clouds/dsp/pvoc/phase_vocoder.cc clouds/dsp/pvoc/stft.cc \
    clouds/dsp/pvoc/frame_transformation.cc clouds/resources.cc \
    stmlib/utils/random.cc stmlib/dsp/units.cc stmlib/dsp/atan.cc
mkdir -p /tmp/c_pcm && /tmp/clouds_compare /tmp/c_pcm

cd ../eurorack-rs
cargo run --release --example clouds_compare -p mi-clouds -- /tmp/rust_pcm
python3 tools/wav_diff.py /tmp/c_pcm /tmp/rust_pcm
```

9 of the 12 (mode × quality) dumps are byte-identical; 2 more differ by 1 LSB on
≤ 2 of 96000 samples, and the last (mono Stretch) diverges into a different but
valid WSOLA splice near the end of the run — see
[`crates/clouds/PORTING.md`](crates/clouds/PORTING.md).

## License

Mirrors the upstream split: AVR-derived code GPL-3.0-or-later, STM32-derived code
MIT. `Cargo.toml` declares `MIT OR GPL-3.0-or-later`.
