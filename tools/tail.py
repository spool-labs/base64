#!/usr/bin/env python3
"""Read a `tail` run: nanoseconds per call, by length."""

import re
import sys

TIMES = {"ps": 0.001, "ns": 1.0, "µs": 1000.0, "us": 1000.0, "ms": 1e6}


def main():
    rows, current = {}, None
    for line in open(sys.argv[1]):
        header = re.match(r"^tail/(\w+)/(\d+)", line)
        if header:
            current = (header.group(1), int(header.group(2)))
        time = re.search(r"time:\s+\[[\d.]+ \S+ ([\d.]+) (\S+)", line)
        if time and current:
            rows[current] = float(time.group(1)) * TIMES[time.group(2)]
            current = None
    print(f"\n{sys.argv[2] if len(sys.argv) > 2 else 'run'} (ns per call)")
    sizes = sorted({size for _, size in rows})
    for direction in ("encode", "decode"):
        cells = " ".join(f"{s}={rows[(direction, s)]:.2f}" for s in sizes if (direction, s) in rows)
        print(f"  {direction:>7}: {cells}")


if __name__ == "__main__":
    main()
