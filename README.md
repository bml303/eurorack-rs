# eurorack-rs

A Rust workspace porting the DSP of [Mutable Instruments' Eurorack
modules](https://github.com/pichenettes/eurorack) to `no_std` library crates.

## Status

| crate            | what it is                                   | state |
|------------------|----------------------------------------------|-------|
| `mi-stmlib`      | shared DSP library (`stmlib`)                | **ported** — primitives Braids/Plaits need, plus generally-useful helpers; tested |
| `mi-braids`      | Braids macro-oscillator (~48 models)         | **ported — reference crate**; 47/48 models verified **bit-identical** to the C firmware DSP |
| `mi-plaits`      | Plaits macro-oscillator (24 models)          | **ported**, floating-point (no bit-exactness contract); 24 models working |
| `mi-clouds`      | Clouds granular texture synthesizer          | **ported**, floating-point; all 4 playback modes (Granular / Stretch / Looping-Delay / Spectral) + all FX; 13/16 (mode×quality) dumps bit-identical to the C, 2 more within 1 LSB |
| `mi-elements`    | Elements modal / physical-modelling voice    | **ported**, floating-point (no bit-exactness contract); MODAL / STRING / STRINGS + the "Ominous" easter-egg voice + diffuser & reverb; smoke-tested |
| `mi-edges`       | Edges quad chiptune oscillator (AVR)         | **ported**, fixed-point; the sampled `DigitalOscillator` (6 shapes) + `TimerOscillator` (square-wave timer maths + a software square renderer); smoke-tested (no C harness — firmware is AVR-asm-only) |
| `mi-rings`       | Rings modal / sympathetic-string resonator   | **ported**, floating-point (no bit-exactness contract); all 6 resonator models + `Strummer` + the "Disastrous Peace" string-synth easter egg (formant / chorus / ensemble / reverb); smoke-tested |
| `mi-tides`       | Tides tidal modulator (2014, fixed-point)    | **ported**; `Generator` verified **bit-identical** to the C firmware DSP over an 18-way {range x mode x sync} sweep |
| `mi-tides2`      | Tides2 tidal modulator (2018, floating-point)| **ported**; `PolySlopeGenerator`/`RampGenerator`/`RampExtractor` — a 24-way sweep comes out bit-identical to the C on this toolchain (no bit-exactness *contract*, see its `PORTING.md`) |
| `mi-marbles`     | Marbles random sampler / CV generator        | **ported**, floating-point (no bit-exactness contract); the T-section (complementary/independent Bernoulli, three-states, drums, markov, clusters/divider gate generators) + the X/Y section (4-channel quantized/unquantized random voltage generation); smoke-tested (no C harness — the C test drives real hardware capture) |
| `mi-yarns`       | Yarns MIDI interface                         | **ported**, fixed-point; `Voice`/`Oscillator` (portamento, vibrato/PLL-synced LFO, 6 trigger shapes, a 5-waveform BLEP oscillator) + `JustIntonationProcessor` + `InternalClock` + `Part` (9 voice-allocation modes, arpeggiator, 64-step sequencer) + `Multi` (11 layouts, shared clock, CV/gate/audio-source derivation, the built-in demo song) + `MidiDispatch`/`mi-stmlib`'s `MidiStreamParser` (raw MIDI byte stream — running status, realtime interleaving, SysEx framing — straight into `Multi`); SysEx calibration/storage protocol and settings/UI/storage out of scope (see its `PORTING.md`); smoke-tested |
| `mi-grids`       | Grids topographic drum sequencer (AVR)       | **ported**, fixed-point; `PatternGenerator` (25-map drum-density lookup + Euclidean-rhythm generator, both output modes) + `Clock` (tempo/swing phase accumulator); `LoadSettings`/`SaveSettings` (EEPROM) and the app-level main loop out of scope (see its `PORTING.md`); smoke-tested (no C harness) |
| `mi-warps`       | Warps meta-modulator (cross-mod, vocoder)    | **ported**, floating-point (no bit-exactness contract); `Modulator` — 6 cross-modulation algorithms at x6 oversampling, a 20-band `Vocoder`/`FilterBank`, the frequency-shifter easter egg; smoke-tested (no C harness — the C test needs an external WAV file not in the repo) |
| `mi-branches`    | Branches dual Bernoulli gate (AVR)           | **ported**, fixed-point; `Channel`/`Branches` — rising-edge probabilistic gate/toggle decision + the free-running 32-bit LFSR; switch-UI/EEPROM persistence out of scope (see its `PORTING.md`); smoke-tested (no C harness) |
| `mi-streams`     | Streams dual dynamics gate (VCA/VCF)         | **ported**, fixed-point; `Processor` dispatches 6 algorithms (`Envelope`, `Vactrol`, `Follower`, `Compressor`, `FilterController`, `LorenzGenerator`), several with 64-bit intermediate arithmetic matched verbatim; smoke-tested (no C harness) |
| `mi-frames`, `mi-peaks`, `mi-stages` (3 more) | one crate per remaining module | **scaffold** — `Cargo.toml` + `lib.rs` + a per-crate `PORTING.md` source inventory |
| `mi-frames`      | Frames keyframer / mixer                     | **ported**, fixed-point; `Keyframer` (up to 64 keyframes, 6 easing curves, linear/exponential VCA response) + `PolyLfo` (4-channel wavetable LFO easter egg); found & fixed 3 latent C++ out-of-bounds reads (see its `PORTING.md`); smoke-tested (no C harness) |
| `mi-peaks`, `mi-stages`, `mi-streams` (3 more) | one crate per remaining module | **scaffold** — `Cargo.toml` + `lib.rs` + a per-crate `PORTING.md` source inventory |

`braids` is the worked example every fixed-point module port should follow;
`plaits` is the worked example for a floating-point module (no bit-exactness
contract). See [`PORTING.md`](PORTING.md) for the
method, the fidelity contract, and the verification workflow.

## Layout

```
crates/
  stmlib/            mi-stmlib   — fixed & float DSP, ParameterInterpolator,
                                   CosineOscillator, Random, units, gate flags,
                                   MidiStreamParser, NoteStack, VoiceAllocator
  braids/            mi-braids   — analog_oscillator, digital_oscillator,
                                   macro_oscillator, quantizer, svf, excitation,
                                   envelope, resources (transpiled tables)
  plaits/            mi-plaits   — voice (24 engine slots), oscillator, noise,
                                   fx, physical_modelling, chords, drums,
                                   envelope, resources (transpiled tables)
  clouds/            mi-clouds   — granular_processor, granular/wsola/looping
                                   players, pvoc (stft, frame_transformation,
                                   phase_vocoder), grain, window, correlator, fx
                                   (diffuser, reverb, pitch_shifter), audio_buffer,
                                   mu_law, resources (transpiled tables)
  tides/             mi-tides    — generator (fixed-point), resources
  tides2/            mi-tides2   — ramp_generator, ramp_shaper, ramp_extractor,
                                   poly_slope_generator (floating-point), resources
  marbles/           mi-marbles  — ramp (ramp_divider, ramp_extractor, ramp_generator,
                                   slave_ramp), random (t_generator, x_y_generator,
                                   quantizer, discrete_distribution_quantizer,
                                   distributions, output_channel, random_sequence,
                                   random_stream), resources
  yarns/             mi-yarns    — voice, oscillator, just_intonation_processor,
                                   internal_clock, part (note allocation, arpeggiator,
                                   sequencer), multi (MIDI routing, layouts, clock,
                                   demo song), song, midi_dispatch, midi_out, resources
  grids/             mi-grids    — pattern_generator (drum-map + Euclidean sequencer),
                                   clock (tempo/swing), random (AVR LFSR), resources
  warps/             mi-warps    — modulator (Modulator, 6 xmod algorithms), oscillator,
                                   quadrature_oscillator, quadrature_transform,
                                   sample_rate_converter, filter_bank, vocoder, limiter,
                                   parameters, resources
  branches/          mi-branches — Channel/Branches (gate decision), rng (AVR LFSR),
                                   resources
  streams/           mi-streams  — processor (Processor, 6 algorithms), envelope, vactrol,
                                   follower, compressor, lorenz_generator, filter_controller,
                                   svf, audio_cv_meter, meta_parameters, consts, resources
  frames/            mi-frames   — keyframer (Keyframer), poly_lfo (PolyLfo), resources
  <module>/          mi-<module> — scaffold + PORTING.md
tools/
  transpile_resources.py   C `resources.cc` -> Rust `static` arrays
  braids_compare.cc        reference renderer (links the C firmware DSP)
  clouds_compare.cc        same, for clouds
  tides_compare.cc         reference renderer for mi-tides (fixed-point)
  tides2_compare.cc        reference renderer for mi-tides2 (floating-point)
  wav_diff.py              diff two trees of raw-PCM / WAV dumps (bit-exact)
  f32_diff.py              diff two trees of raw-float32 dumps (numeric tolerance)
```

## Build & test

```
cargo build --workspace
cargo test  --workspace          # Braids/Tides/Tides2 equivalence goldens, Plaits/Clouds/... smoke tests
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

13 of the 16 (mode × quality) dumps are byte-identical — every Granular and
Spectral render; 2 more differ by 1 LSB on ≤ 2 of 96000 samples, and the last
(mono Stretch) diverges into a different but valid WSOLA splice near the end of
the run — see [`crates/clouds/PORTING.md`](crates/clouds/PORTING.md).

## Verifying `tides` / `tides2` against the C

```
cd ../eurorack
g++ -O2 -DTEST -I. -Istmlib -o /tmp/tides_compare \
    ../eurorack-rs/tools/tides_compare.cc \
    tides/generator.cc tides/resources.cc
mkdir -p /tmp/c_pcm && /tmp/tides_compare /tmp/c_pcm
cd ../eurorack-rs
cargo run --release --example compare -p mi-tides -- /tmp/rust_pcm
python3 tools/wav_diff.py /tmp/c_pcm /tmp/rust_pcm     # bit-exact

cd ../eurorack
g++ -O2 -DTEST -I. -Istmlib -o /tmp/tides2_compare \
    ../eurorack-rs/tools/tides2_compare.cc \
    tides2/poly_slope_generator.cc tides2/resources.cc tides2/ramp/ramp_extractor.cc
mkdir -p /tmp/c_f32 && /tmp/tides2_compare /tmp/c_f32
cd ../eurorack-rs
cargo run --release --example compare -p mi-tides2 -- /tmp/rust_f32
python3 tools/f32_diff.py /tmp/c_f32 /tmp/rust_f32     # numeric tolerance
```

`mi-tides`'s 18-way sweep is bit-identical (a hard contract, like Braids).
`mi-tides2`'s 24-way sweep also comes out bit-identical on this toolchain, but
that isn't a contract for a floating-point port — see
[`crates/tides2/PORTING.md`](crates/tides2/PORTING.md).

## License

Mirrors the upstream split: AVR-derived code GPL-3.0-or-later, STM32-derived code
MIT. `Cargo.toml` declares `MIT OR GPL-3.0-or-later`.
