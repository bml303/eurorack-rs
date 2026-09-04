// Reference renderer: dumps `<out_dir>/<NN>.pcm` (raw little-endian int16) for
// every MacroOscillatorShape, driven by a fixed, deterministic parameter sweep.
// The Rust `--compare` example produces the same files; `tools/wav_diff.py`
// diffs the two trees.
//
// Build (from the eurorack C repo root, submodules checked out):
//   g++ -O2 -DTEST -I. -Istmlib -o /tmp/braids_compare \
//       ../eurorack-rs/tools/braids_compare.cc \
//       braids/analog_oscillator.cc braids/digital_oscillator.cc \
//       braids/macro_oscillator.cc braids/resources.cc stmlib/utils/random.cc

#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <new>
#include <string>

#include "braids/macro_oscillator.h"
#include "stmlib/utils/random.h"

using namespace braids;
using stmlib::Random;

static const int kBlock = 24;
static const int kBlocks = 4000;  // 96000 samples per shape

int main(int argc, char** argv) {
  const char* out_dir = argc > 1 ? argv[1] : ".";
  for (int shape = 0; shape < MACRO_OSC_SHAPE_LAST; ++shape) {
    Random::Seed(0x21);  // isolate each shape's RNG sequence
    // Zero-initialise like the firmware's global instance (and the Rust
    // `MacroOscillator::new()`), otherwise stack garbage in
    // `previous_phase_increment_` desyncs the first block.
    void* mem = calloc(1, sizeof(MacroOscillator));
    MacroOscillator& osc = *new (mem) MacroOscillator();
    memset(mem, 0, sizeof(MacroOscillator));
    osc.Init();
    osc.set_shape(static_cast<MacroOscillatorShape>(shape));

    char path[512];
    snprintf(path, sizeof(path), "%s/%02d.pcm", out_dir, shape);
    FILE* fp = fopen(path, "wb");

    for (int i = 0; i < kBlocks; ++i) {
      int16_t buffer[kBlock];
      uint8_t sync[kBlock];
      memset(sync, 0, sizeof(sync));
      if ((i % 37) == 0) sync[i % kBlock] = 1;

      int32_t p1 = (i * 163) & 0x7fff;
      int32_t p2 = (i * 617) & 0x7fff;
      osc.set_parameters(p1, p2);
      osc.set_pitch(((24 << 7) + i * 17) > (120 << 7) ? (120 << 7) : ((24 << 7) + i * 17));
      if ((i % 128) == 0) osc.Strike();
      osc.Render(sync, buffer, kBlock);
      fwrite(buffer, sizeof(int16_t), kBlock, fp);
    }
    fclose(fp);
    free(mem);
  }
  return 0;
}
