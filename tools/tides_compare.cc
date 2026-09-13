// Reference renderer: dumps `<out_dir>/<NN>.pcm` (raw little-endian int16
// triples: bipolar, unipolar-bit-pattern, flags) for every combination of
// {range x mode x sync}, driven by a fixed, deterministic control/parameter
// sweep. The Rust `--compare` example produces the same files from the same
// sweep; `tools/wav_diff.py` diffs the two trees.
//
// `Generator`'s block-processing methods (`ProcessAudioRate`,
// `ProcessControlRate`, `ProcessFilterWavefolder`) are private; the public API
// is a hardware-latency-hiding double-buffer (`Process(control)` /
// `Process()`). To drive the DSP directly, with no warm-up block of zeros and
// no extra block of ring-buffer latency, this harness pokes the (otherwise
// private) block buffers and counters directly via `#define private public`
// -- valid for a single-TU reference tool like this one, since access
// specifiers don't affect layout.
//
// Build (from the eurorack C repo root, submodules checked out):
//   g++ -O2 -DTEST -I. -Istmlib -o /tmp/tides_compare \
//       ../eurorack-rs/tools/tides_compare.cc \
//       tides/generator.cc tides/resources.cc

#define private public
#include "tides/generator.h"
#undef private

#include <cstdint>
#include <cstdio>
#include <cstring>

using namespace tides;

static const int kBlock = 16;
static const int kBlocks = 3000; // 48 000 samples per combination

// Deterministic control-byte sequence, shared (by construction, not by code)
// with the Rust harness: a gate LFO, a clock LFO (only meaningful when sync is
// under test) and an occasional freeze window.
static uint8_t ControlAt(uint32_t n) {
  const uint32_t period = 512;
  const uint32_t clock_period = 37;
  const uint32_t freeze_period = 733;
  const uint32_t freeze_len = 13;

  bool gate = (n % period) < (period / 4);
  bool gate_prev = n == 0 ? false : ((n - 1) % period) < (period / 4);
  bool clock = (n % clock_period) < (clock_period / 2);
  bool clock_prev = n == 0 ? false : ((n - 1) % clock_period) < (clock_period / 2);
  bool freeze = (n % freeze_period) < freeze_len;

  uint8_t control = 0;
  if (freeze) control |= CONTROL_FREEZE;
  if (gate) control |= CONTROL_GATE;
  if (gate && !gate_prev) control |= CONTROL_GATE_RISING;
  if (!gate && gate_prev) control |= CONTROL_GATE_FALLING;
  if (clock) control |= CONTROL_CLOCK;
  if (clock && !clock_prev) control |= CONTROL_CLOCK_RISING;
  return control;
}

int main(int argc, char** argv) {
  const char* out_dir = argc > 1 ? argv[1] : ".";

  const GeneratorRange ranges[] = {
      GENERATOR_RANGE_HIGH, GENERATOR_RANGE_MEDIUM, GENERATOR_RANGE_LOW};
  const GeneratorMode modes[] = {
      GENERATOR_MODE_AD, GENERATOR_MODE_LOOPING, GENERATOR_MODE_AR};
  const bool syncs[] = {false, true};

  int combo = 0;
  for (int ri = 0; ri < 3; ++ri) {
    for (int mi = 0; mi < 3; ++mi) {
      for (int si = 0; si < 2; ++si) {
        Generator gen;
        memset(&gen, 0, sizeof(gen));
        gen.Init();
        gen.set_range(ranges[ri]);
        gen.set_mode(modes[mi]);
        gen.set_sync(syncs[si]);

        char path[512];
        snprintf(path, sizeof(path), "%s/%02d.pcm", out_dir, combo);
        FILE* fp = fopen(path, "wb");

        uint32_t n = 0;
        for (int b = 0; b < kBlocks; ++b) {
          int16_t shape = static_cast<int16_t>(((b * 163) & 0x7fff) - 16384);
          int16_t slope = static_cast<int16_t>(
              static_cast<int32_t>((b * 617) & 0xffff) - 32768);
          int16_t smoothness = static_cast<int16_t>(
              static_cast<int32_t>((b * 941) & 0xffff) - 32768);
          int16_t pitch = static_cast<int16_t>(
              (24 << 7) + static_cast<int16_t>((b * 37) % (72 * 128)));

          gen.set_shape(shape);
          gen.set_slope(slope);
          gen.set_smoothness(smoothness);
          gen.set_pitch(pitch);

          uint8_t control[kBlock];
          for (int j = 0; j < kBlock; ++j) {
            control[j] = ControlAt(n + j);
          }

          memcpy(gen.input_samples_[0], control, kBlock);
          gen.render_block_ = 0;
          gen.playback_block_ = 1;
          gen.Process();

          for (int j = 0; j < kBlock; ++j) {
            const GeneratorSample& s = gen.output_samples_[0][j];
            int16_t out3[3];
            out3[0] = s.bipolar;
            out3[1] = static_cast<int16_t>(s.unipolar);
            out3[2] = static_cast<int16_t>(s.flags);
            fwrite(out3, sizeof(int16_t), 3, fp);
          }
          n += kBlock;
        }
        fclose(fp);
        ++combo;
      }
    }
  }
  return 0;
}
