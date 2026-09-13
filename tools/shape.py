#!/usr/bin/env python3
"""Read a `shape` benchmark run and print where the time goes, in nanoseconds.

criterion picks whatever unit fits a row, so every unit it can print is converted.
"""

import re
import sys

TIMES = {"ps": 0.001, "ns": 1.0, "µs": 1000.0, "us": 1000.0, "ms": 1e6, "s": 1e9}
ROWS = ("nothing", "copy", "kernel", "codec")


def parse(lines):
    out, current = {}, None
    for line in lines:
        header = re.match(r"^shape-(encode|decode)/(\w+)/(\d+)", line)
        if header:
            current = (header.group(1), header.group(2), int(header.group(3)))
        time = re.search(r"time:\s+\[[\d.]+ \S+ ([\d.]+) (\S+)", line)
        if time and current:
            out[current] = float(time.group(1)) * TIMES[time.group(2)]
            current = None
    return out


def main():
    out = parse(open(sys.argv[1]))
    for direction in ("encode", "decode"):
        sizes = sorted({size for d, _, size in out if d == direction})
        if not sizes:
            continue
        print(f"\n=== {direction} (ns) ===")
        head = " ".join(f"{name:>9}" for name in ROWS)
        print(f"{'bytes':>6} {head} {'net codec':>10} {'net kernel':>11}")
        for size in sizes:
            cells = []
            for name in ROWS:
                value = out.get((direction, name, size))
                cells.append(f"{value:>9.2f}" if value is not None else "        -")
            floor = out.get((direction, "nothing", size), 0.0)
            codec = out.get((direction, "codec", size))
            kernel = out.get((direction, "kernel", size))
            net_codec = f"{codec - floor:>10.2f}" if codec else "         -"
            net_kernel = f"{kernel - floor:>11.2f}" if kernel else "          -"
            print(f"{size:>6} {' '.join(cells)} {net_codec} {net_kernel}")


if __name__ == "__main__":
    main()
