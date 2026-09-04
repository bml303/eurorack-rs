# Porting Plaits

**Macro oscillator (24 synthesis + noise models)**  |  MCU family: `stm32f3`  |  ~21343 lines of hand-written C (excl. resources & drivers)

## Method

Follow the `braids` crate as the worked example:

1. `python tools/transpile_resources.py ../eurorack/plaits/resources.cc \
       ../eurorack/plaits/resources.h crates/plaits/src/resources.rs`
2. Port `mi-stmlib` primitives this module needs (check its `#include`s) if not
   already present.
3. Translate the DSP files below. Preserve fixed-point arithmetic verbatim
   (use `wrapping_*`); modernise *structure* only -- modules, methods, enums,
   `match` instead of function-pointer tables.
4. Add `examples/render_wav.rs` mirroring `plaits/test/*_test.cc` and diff the
   output WAV against the C test with `tools/wav_diff.py`.

## Source inventory (DSP + UI, drivers/bootloader/resources excluded)

| file | lines |
|------|-------|
| `plaits.cc` | 172 |
| `pot_controller.h` | 174 |
| `settings.cc` | 96 |
| `settings.h` | 107 |
| `ui.cc` | 593 |
| `ui.h` | 163 |
| `user_data.h` | 116 |
| `user_data_receiver.cc` | 84 |
| `user_data_receiver.h` | 189 |
| `dsp/dsp.h` | 55 |
| `dsp/envelope.h` | 130 |
| `dsp/voice.cc` | 272 |
| `dsp/voice.h` | 258 |
| `dsp/downsampler/4x_downsampler.h` | 71 |
| `dsp/drums/analog_bass_drum.h` | 195 |
| `dsp/drums/analog_snare_drum.h` | 201 |
| `dsp/drums/hi_hat.h` | 265 |
| `dsp/drums/synthetic_bass_drum.h` | 248 |
| `dsp/drums/synthetic_snare_drum.h` | 198 |
| `dsp/fm/algorithms.cc` | 457 |
| `dsp/fm/algorithms.h` | 214 |
| `dsp/fm/dx_units.cc` | 115 |
| `dsp/fm/dx_units.h` | 206 |
| `dsp/fm/envelope.h` | 258 |
| `dsp/fm/lfo.h` | 192 |
| `dsp/fm/operator.h` | 138 |
| `dsp/fm/patch.h` | 152 |
| `dsp/fm/voice.h` | 288 |
| `dsp/physical_modelling/delay_line.h` | 103 |
| `dsp/physical_modelling/modal_voice.cc` | 100 |
| `dsp/physical_modelling/modal_voice.h` | 68 |
| `dsp/physical_modelling/resonator.cc` | 136 |
| `dsp/physical_modelling/resonator.h` | 134 |
| `dsp/physical_modelling/string.cc` | 190 |
| `dsp/physical_modelling/string.h` | 97 |
| `dsp/physical_modelling/string_voice.cc` | 113 |
| `dsp/physical_modelling/string_voice.h` | 69 |
| `dsp/engine/additive_engine.cc` | 151 |
| `dsp/engine/additive_engine.h` | 73 |
| `dsp/engine/bass_drum_engine.cc` | 96 |
| `dsp/engine/bass_drum_engine.h` | 65 |
| `dsp/engine/chord_engine.cc` | 172 |
| `dsp/engine/chord_engine.h` | 74 |
| `dsp/engine/engine.h` | 133 |
| `dsp/engine/fm_engine.cc` | 123 |
| `dsp/engine/fm_engine.h` | 69 |
| `dsp/engine/grain_engine.cc` | 89 |
| `dsp/engine/grain_engine.h` | 68 |
| `dsp/engine/hi_hat_engine.cc` | 81 |
| `dsp/engine/hi_hat_engine.h` | 63 |
| `dsp/engine/modal_engine.cc` | 73 |
| `dsp/engine/modal_engine.h` | 61 |
| `dsp/engine/noise_engine.cc` | 102 |
| `dsp/engine/noise_engine.h` | 70 |
| `dsp/engine/particle_engine.cc` | 99 |
| `dsp/engine/particle_engine.h` | 64 |
| `dsp/engine/snare_drum_engine.cc` | 78 |
| `dsp/engine/snare_drum_engine.h` | 61 |
| `dsp/engine/speech_engine.cc` | 142 |
| `dsp/engine/speech_engine.h` | 81 |
| `dsp/engine/string_engine.cc` | 91 |
| `dsp/engine/string_engine.h` | 66 |
| `dsp/engine/swarm_engine.cc` | 86 |
| `dsp/engine/swarm_engine.h` | 256 |
| `dsp/engine/virtual_analog_engine.cc` | 245 |
| `dsp/engine/virtual_analog_engine.h` | 72 |
| `dsp/engine/waveshaping_engine.cc` | 137 |
| `dsp/engine/waveshaping_engine.h` | 63 |
| `dsp/engine/wavetable_engine.cc` | 219 |
| `dsp/engine/wavetable_engine.h` | 80 |
| `dsp/engine2/arpeggiator.h` | 133 |
| `dsp/engine2/chiptune_engine.cc` | 128 |
| `dsp/engine2/chiptune_engine.h` | 79 |
| `dsp/engine2/phase_distortion_engine.cc` | 87 |
| `dsp/engine2/phase_distortion_engine.h` | 62 |
| `dsp/engine2/six_op_engine.cc` | 180 |
| `dsp/engine2/six_op_engine.h` | 118 |
| `dsp/engine2/string_machine_engine.cc` | 138 |
| `dsp/engine2/string_machine_engine.h` | 70 |
| `dsp/engine2/virtual_analog_vcf_engine.cc` | 131 |
| `dsp/engine2/virtual_analog_vcf_engine.h` | 70 |
| `dsp/engine2/wave_terrain_engine.cc` | 235 |
| `dsp/engine2/wave_terrain_engine.h` | 75 |
| `dsp/noise/clocked_noise.h` | 104 |
| `dsp/noise/dust.h` | 48 |
| `dsp/noise/fractal_random_generator.h` | 73 |
| `dsp/noise/particle.h` | 93 |
| `dsp/noise/smooth_random_generator.h` | 69 |
| `dsp/oscillator/formant_oscillator.h` | 129 |
| `dsp/oscillator/grainlet_oscillator.h` | 195 |
| `dsp/oscillator/harmonic_oscillator.h` | 120 |
| `dsp/oscillator/nes_triangle_oscillator.h` | 167 |
| `dsp/oscillator/oscillator.h` | 254 |
| `dsp/oscillator/sine_oscillator.h` | 254 |
| `dsp/oscillator/string_synth_oscillator.h` | 179 |
| `dsp/oscillator/super_square_oscillator.h` | 164 |
| `dsp/oscillator/variable_saw_oscillator.h` | 165 |
| `dsp/oscillator/variable_shape_oscillator.h` | 285 |
| `dsp/oscillator/vosim_oscillator.h` | 139 |
| `dsp/oscillator/wavetable_oscillator.h` | 190 |
| `dsp/oscillator/z_oscillator.h` | 205 |
| `dsp/speech/lpc_speech_synth.cc` | 163 |
| `dsp/speech/lpc_speech_synth.h` | 110 |
| `dsp/speech/lpc_speech_synth_controller.cc` | 335 |
| `dsp/speech/lpc_speech_synth_controller.h` | 197 |
| `dsp/speech/lpc_speech_synth_phonemes.cc` | 126 |
| `dsp/speech/lpc_speech_synth_words.cc` | 1573 |
| `dsp/speech/lpc_speech_synth_words.h` | 48 |
| `dsp/speech/naive_speech_synth.cc` | 160 |
| `dsp/speech/naive_speech_synth.h` | 85 |
| `dsp/speech/sam_speech_synth.cc` | 185 |
| `dsp/speech/sam_speech_synth.h` | 90 |
| `dsp/chords/chord_bank.cc` | 154 |
| `dsp/chords/chord_bank.h` | 111 |
| `dsp/fx/diffuser.h` | 108 |
| `dsp/fx/ensemble.h` | 136 |
| `dsp/fx/fx_engine.h` | 300 |
| `dsp/fx/low_pass_gate.h` | 92 |
| `dsp/fx/overdrive.h` | 83 |
| `dsp/fx/sample_rate_reducer.h` | 136 |
| `test/plaits_test.cc` | 1339 |
| `drivers/audio_dac.cc` | 132 |
| `drivers/audio_dac.h` | 73 |
| `drivers/cv_adc.cc` | 193 |
| `drivers/cv_adc.h` | 74 |
| `drivers/debug_pin.h` | 72 |
| `drivers/debug_port.cc` | 62 |
| `drivers/debug_port.h` | 67 |
| `drivers/firmware_update_adc.cc` | 111 |
| `drivers/firmware_update_adc.h` | 60 |
| `drivers/leds.cc` | 114 |
| `drivers/leds.h` | 72 |
| `drivers/normalization_probe.h` | 87 |
| `drivers/pots_adc.cc` | 111 |
| `drivers/pots_adc.h` | 71 |
| `drivers/switches.cc` | 72 |
| `drivers/switches.h` | 82 |

## Resources

`plaits/resources.cc` (10548 lines of generated lookup tables) -> transpile with `tools/transpile_resources.py` into `src/resources.rs`, exactly as done for `braids`.

## Not in scope for the library crate

STM32/AVR peripheral drivers (`drivers/`), the audio bootloader, and the
`hardware_design/` files stay in the C repo -- the Rust crate is a `no_std`
DSP library that a host or an embedded HAL feeds.
