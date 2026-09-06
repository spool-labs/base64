#!/usr/bin/env python3
"""Read a `quick` run and print one line per row, for comparing variants."""

import re
import sys

UNITS = {"GiB": 1.0, "MiB": 1 / 1024, "KiB": 1 / 1024 / 1024, "TiB": 1024.0}


def main():
    rows, current = {}, None
    for line in open(sys.argv[1]):
        header = re.match(r"^quick/([\w\-]+)/(\d+)", line)
        if header:
            current = (header.group(1), int(header.group(2)))
        rate = re.search(r"thrpt:\s+\[[\d.]+ \S+ ([\d.]+) (\w+)/s", line)
        if rate and current:
            rows[current] = float(rate.group(1)) * UNITS[rate.group(2)]
            current = None
    label = sys.argv[2] if len(sys.argv) > 2 else "run"
    sizes = sorted({size for _, size in rows})
    print(f"\n{label}")
    for direction in ("encode", "decode"):
        mine = [rows.get((direction, s), 0.0) for s in sizes]
        simd = [rows.get((f"{direction}-simd", s), 0.0) for s in sizes]
        turbo = [rows.get((f"{direction}-turbo", s), 0.0) for s in sizes]
        print(f"  {direction:>7}: " + "  ".join(
            f"{s}={m:.2f}({m / max(a, b, 1e-9):.2f}x)" for s, m, a, b in zip(sizes, mine, simd, turbo)))


if __name__ == "__main__":
    main()
