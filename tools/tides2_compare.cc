// Reference renderer for the (floating-point, not bit-exact) Tides2 port --
// dumps `<out_dir>/<NN>.f32` (raw little-endian float32, `num_channels + 1`
// per frame: gate-high flag then the 4 output channels) for every
// {ramp_mode x output_mode x range} combination, internal ramp source only
// (mirrors `tides2/test/tides_test.cc`'s `TestPolySlopeGenerator`, minus the
// WAV writing and the external-ramp-source half). The Rust `--compare`
// example produces the same files; `tools/f32_diff.py` compares them with a
// numeric tolerance (this port doesn't claim bit-exactness for a
// floating-point module -- see `mi-plaits` and `crates/tides2/PORTING.md`).
//
// Build (from the eurorack C repo root, submodules checked out):
//   g++ -O2 -DTEST -I. -Istmlib -o /tmp/tides2_compare \
//       ../eurorack-rs/tools/tides2_compare.cc \
//       tides2/poly_slope_generator.cc tides2/resources.cc \
//       tides2/ramp/ramp_extractor.cc

#include "tides2/poly_slope_generator.h"
#include "tides2/ramp/ramp_extractor.h"
#include "tides2/test/fixtures.h"

#include <cstdint>
#include <cstdio>
#include <vector>

using namespace tides;
using namespace stmlib;

static const size_t kBlock = 6;
static const float kSampleRate = 48000.0f;
static const int kSeconds = 4;

int main(int argc, char** argv) {
  const char* out_dir = argc > 1 ? argv[1] : ".";

  int combo = 0;
  for (int ramp_mode = 0; ramp_mode < RAMP_MODE_LAST; ++ramp_mode) {
    for (int output_mode = 0; output_mode < OUTPUT_MODE_LAST; ++output_mode) {
      for (int range = 0; range < RANGE_LAST; ++range) {
        PulseGenerator pulses;
        if (ramp_mode != RAMP_MODE_LOOPING) {
          pulses.CreateTestPattern();
        } else {
          pulses.AddPulses(kSampleRate, 100, 10);
        }

        PolySlopeGenerator poly_slope;
        poly_slope.Init();

        char path[512];
        snprintf(path, sizeof(path), "%s/%02d.f32", out_dir, combo);
        FILE* fp = fopen(path, "wb");

        for (size_t i = 0; i < kSampleRate * kSeconds; i += kBlock) {
          GateFlags gate_flags[kBlock];
          pulses.Render(gate_flags, kBlock);

          PolySlopeGenerator::OutputSample out[kBlock];
          const float f0 = (ramp_mode == RAMP_MODE_LOOPING ? 0.5f * 220.0f : 4.0f) / kSampleRate;

          poly_slope.Render(
              RampMode(ramp_mode),
              OutputMode(output_mode),
              ramp_mode == RAMP_MODE_LOOPING ? RANGE_AUDIO : Range(range),
              f0,
              0.3f,   // pw
              0.6f,   // shape
              0.4f,   // smoothness
              0.25f,  // shift
              gate_flags,
              NULL,
              out,
              kBlock);

          for (size_t j = 0; j < kBlock; ++j) {
            float frame[5];
            frame[0] = (gate_flags[j] & GATE_FLAG_HIGH) ? 1.0f : 0.0f;
            frame[1] = out[j].channel[0];
            frame[2] = out[j].channel[1];
            frame[3] = out[j].channel[2];
            frame[4] = out[j].channel[3];
            fwrite(frame, sizeof(float), 5, fp);
          }
        }
        fclose(fp);
        ++combo;
      }
    }
  }
  return 0;
}
