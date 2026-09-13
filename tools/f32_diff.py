#!/usr/bin/env python3
"""Diff two trees of raw little-endian float32 dumps with a numeric tolerance
(for the floating-point ports, e.g. mi-tides2, which don't claim bit-exactness
against the C -- see wav_diff.py for the fixed-point/bit-exact comparison).

Usage:
    f32_diff.py <c_dir> <rust_dir> [tolerance]
"""
import array
import os
import sys


def load_f32(path):
    a = array.array("f")
    with open(path, "rb") as f:
        a.frombytes(f.read())
    return a


def compare(name, a, b, tol):
    n = min(len(a), len(b))
    max_d = 0.0
    worst = -1
    for i in range(n):
        d = abs(a[i] - b[i])
        if d > max_d:
            max_d = d
            worst = i
    ok = max_d <= tol and len(a) == len(b)
    status = "OK" if ok else "MISMATCH"
    print(
        f"  {name}: {status}  max |delta|={max_d:.3g} (tol {tol:.3g}) at index {worst}, "
        f"len {len(a)} vs {len(b)}"
    )
    return ok


def main():
    a, b = sys.argv[1], sys.argv[2]
    tol = float(sys.argv[3]) if len(sys.argv) > 3 else 1e-4
    names = sorted(f for f in os.listdir(a) if f.endswith(".f32"))
    ok = True
    for name in names:
        pa, pb = os.path.join(a, name), os.path.join(b, name)
        if not os.path.exists(pb):
            print(f"  {name}: missing in {b}")
            ok = False
            continue
        ok &= compare(name, load_f32(pa), load_f32(pb), tol)
    print("ALL OK" if ok else "DIFFERENCES FOUND")
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
