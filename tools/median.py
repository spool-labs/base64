#!/usr/bin/env python3
"""Median of several `quick` runs, so a comparison is not one run's weather.

Each row is throughput and a ratio against the better of base64-simd and base64-turbo
in the same run, since those two move with the machine as much as this crate does.
"""

import re
import statistics
import sys

UNITS = {"GiB": 1.0, "MiB": 1 / 1024, "KiB": 1 / 1024 / 1024, "TiB": 1024.0}


def parse(path):
    rows, current = {}, None
    for line in open(path):
        header = re.match(r"^quick/([\w\-]+)/(\d+)", line)
        if header:
            current = (header.group(1), int(header.group(2)))
        rate = re.search(r"thrpt:\s+\[[\d.]+ \S+ ([\d.]+) (\w+)/s", line)
        if rate and current:
            rows[current] = float(rate.group(1)) * UNITS[rate.group(2)]
            current = None
    return rows


def main():
    label, paths = sys.argv[1], sys.argv[2:]
    runs = [parse(path) for path in paths]
    sizes = sorted({size for run in runs for _, size in run})
    print(f"\n{label}  ({len(runs)} runs, median)")
    for direction in ("encode", "decode"):
        cells = []
        for size in sizes:
            mine = [run[(direction, size)] for run in runs if (direction, size) in run]
            rivals = [
                max(run.get((f"{direction}-simd", size), 0), run.get((f"{direction}-turbo", size), 0))
                for run in runs
                if (direction, size) in run
            ]
            if not mine:
                continue
            ratios = [m / r for m, r in zip(mine, rivals) if r]
            cells.append(
                f"{size}={statistics.median(mine):.2f}"
                f"({statistics.median(ratios):.2f}x)" if ratios else f"{size}={statistics.median(mine):.2f}"
            )
        print(f"  {direction:>7}: " + "  ".join(cells))


if __name__ == "__main__":
    main()
