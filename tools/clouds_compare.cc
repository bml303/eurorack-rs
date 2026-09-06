// Reference renderer for the `mi-clouds` port: dumps `<out_dir>/<MM>.pcm`
// (raw little-endian interleaved stereo int16) for every (playback mode,
// quality) pair, driven by a fixed deterministic parameter sweep over an
// arithmetic sawtooth input. The Rust `--example compare` produces the same
// files; `tools/wav_diff.py` reports the maximum per-sample delta.
//
// Clouds is a floating-point port with no bit-exactness contract, so a small
// non-zero delta is expected; it should stay within a few LSB.
//
// Build (from the eurorack C repo root, submodules checked out):
//   g++ -O2 -DTEST -I. -Istmlib -o /tmp/clouds_compare \
//       ../eurorack-rs/tools/clouds_compare.cc \
//       clouds/dsp/granular_processor.cc clouds/dsp/correlator.cc \
//       clouds/dsp/mu_law.cc clouds/dsp/pvoc/phase_vocoder.cc \
//       clouds/dsp/pvoc/stft.cc clouds/dsp/pvoc/frame_transformation.cc \
//       clouds/resources.cc stmlib/utils/random.cc stmlib/dsp/units.cc \
//       stmlib/dsp/atan.cc
//   mkdir -p /tmp/c_pcm && /tmp/clouds_compare /tmp/c_pcm

#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <new>

#include "clouds/dsp/granular_processor.h"
#include "stmlib/utils/random.h"

using namespace clouds;
using stmlib::Random;

static const int kBlock = 32;
static const int kBlocks = 1500;
// The firmware runs Prepare() in a tight while(1) loop between audio blocks;
// the WSOLA correlator search is spread across those calls.
static const int kPrepareIters = 32;

static const PlaybackMode kModes[3] = {
    PLAYBACK_MODE_GRANULAR,
    PLAYBACK_MODE_STRETCH,
    PLAYBACK_MODE_LOOPING_DELAY,
};

static float tri(float x) {
  x = fabsf(x - floorf(x));
  return x < 0.5f ? x * 2.0f : 2.0f - x * 2.0f;
}

// Same sizes as clouds.cc (block_mem / block_ccm).
static uint8_t large_buffer[118784];
static uint8_t small_buffer[65536 - 128];

int main(int argc, char** argv) {
  const char* out_dir = argc > 1 ? argv[1] : ".";

  for (int mode_idx = 0; mode_idx < 3; ++mode_idx) {
    for (int quality = 0; quality < 4; ++quality) {
      Random::Seed(0x21);

      void* mem = calloc(1, sizeof(GranularProcessor));
      GranularProcessor& gp = *new (mem) GranularProcessor();
      memset(mem, 0, sizeof(GranularProcessor));
      memset(large_buffer, 0, sizeof(large_buffer));
      memset(small_buffer, 0, sizeof(small_buffer));

      gp.Init(large_buffer, sizeof(large_buffer),
              small_buffer, sizeof(small_buffer));
      gp.set_playback_mode(kModes[mode_idx]);
      gp.set_quality(quality);
      for (int k = 0; k < kPrepareIters; ++k) gp.Prepare();

      char path[512];
      snprintf(path, sizeof(path), "%s/%02d.pcm", out_dir, mode_idx * 4 + quality);
      FILE* fp = fopen(path, "wb");

      float phase = 0.0f;
      for (int i = 0; i < kBlocks; ++i) {
        float t = (float) i / (float) kBlocks;
        Parameters* p = gp.mutable_parameters();
        p->position = tri(t * 2.0f);
        p->size = 0.2f + 0.6f * tri(t * 3.0f + 0.1f);
        p->pitch = -7.0f + 14.0f * tri(t * 1.3f);
        p->density = 0.3f + 0.5f * tri(t * 2.5f + 0.3f);
        p->texture = tri(t * 1.7f + 0.5f);
        p->dry_wet = 1.0f;
        p->stereo_spread = 0.5f;
        p->feedback = 0.0f;
        p->reverb = 0.0f;
        p->freeze = (i / 200) % 3 == 2;
        p->trigger = i % 48 == 0;
        p->gate = false;

        ShortFrame input[kBlock];
        ShortFrame output[kBlock];
        for (int j = 0; j < kBlock; ++j) {
          phase += 220.0f / 32000.0f;
          if (phase >= 1.0f) phase -= 1.0f;
          int16_t s = (int16_t) ((phase - 0.5f) * 24000.0f);
          input[j].l = s;
          input[j].r = s;
        }

        gp.Process(input, output, kBlock);
        for (int k = 0; k < kPrepareIters; ++k) gp.Prepare();
        fwrite(output, sizeof(ShortFrame), kBlock, fp);
      }
      fclose(fp);
      free(mem);
    }
  }
  return 0;
}
