#!/usr/bin/env python3
"""Diff two trees of raw little-endian int16 PCM dumps (or two WAV files).

Usage:
    wav_diff.py <c_dir> <rust_dir>      # compares NN.pcm in each
    wav_diff.py a.wav b.wav             # compares two WAVs
"""
import array
import os
import sys
import wave


def load_pcm(path):
    a = array.array("h")
    with open(path, "rb") as f:
        a.frombytes(f.read())
    return a


def load_wav(path):
    with wave.open(path) as w:
        a = array.array("h")
        a.frombytes(w.readframes(w.getnframes()))
    return a


def compare(name, a, b):
    n = min(len(a), len(b))
    diffs = [i for i in range(n) if a[i] != b[i]]
    if not diffs and len(a) == len(b):
        print(f"  {name}: OK ({n} samples, bit-identical)")
        return True
    max_d = max((abs(a[i] - b[i]) for i in diffs), default=0)
    print(
        f"  {name}: MISMATCH  {len(diffs)}/{n} samples differ, "
        f"max |delta|={max_d}, len {len(a)} vs {len(b)}, first at {diffs[0] if diffs else '-'}"
    )
    return False


def main():
    a, b = sys.argv[1], sys.argv[2]
    ok = True
    if os.path.isdir(a):
        names = sorted(f for f in os.listdir(a) if f.endswith(".pcm"))
        for name in names:
            pa, pb = os.path.join(a, name), os.path.join(b, name)
            if not os.path.exists(pb):
                print(f"  {name}: missing in {b}")
                ok = False
                continue
            ok &= compare(name, load_pcm(pa), load_pcm(pb))
    else:
        load = load_wav if a.endswith(".wav") else load_pcm
        ok &= compare(os.path.basename(a), load(a), load(b))
    print("ALL OK" if ok else "DIFFERENCES FOUND")
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
