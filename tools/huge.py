#!/usr/bin/env python3
"""Turn a `huge` run into a table, in GiB/s.

The sizes cross out of L2, out of L3 and into memory, so the column that matters is
the ratio against the rival that switches to non-temporal stores partway up.
"""

import collections
import re
import statistics
import sys

NAMES = ["tape-base64", "base64-turbo", "base64-simd"]
UNITS = {"GiB": 1.0, "MiB": 1 / 1024, "KiB": 1 / 1024 / 1024, "TiB": 1024.0}


def parse(lines):
    results = collections.defaultdict(dict)
    current = None
    for line in lines:
        header = re.match(r"^(encode|decode)-huge/([\w\-]+)/(\d+)", line)
        if header:
            current = (header.group(1), header.group(2), int(header.group(3)))
        rate = re.search(r"thrpt:\s+\[[\d.]+ \S+ ([\d.]+) (\w+)/s", line)
        if rate and current:
            results[(current[0], current[2])][current[1]] = float(rate.group(1)) * UNITS[rate.group(2)]
            current = None
    return results


def median_of(runs):
    merged = collections.defaultdict(dict)
    for key in {key for run in runs for key in run}:
        for name in {name for run in runs for name in run.get(key, {})}:
            seen = [run[key][name] for run in runs if name in run.get(key, {})]
            if seen:
                merged[key][name] = statistics.median(seen)
    return merged


def main():
    if len(sys.argv) > 1:
        results = median_of([parse(open(path)) for path in sys.argv[1:]])
    else:
        results = parse(sys.stdin)
    sizes = sorted({size for _, size in results})
    for direction in ("encode", "decode"):
        rows = [(size, results[(direction, size)]) for size in sizes if results.get((direction, size))]
        if not rows:
            continue
        print(f"\n### {direction} (GiB/s)\n")
        print("| MiB | " + " | ".join(NAMES) + " | vs turbo |")
        print("|---" * (len(NAMES) + 2) + "|")
        for size, row in rows:
            mine = row.get("tape-base64", 0.0)
            turbo = row.get("base64-turbo", 0.0)
            cells = " | ".join(f"{row.get(name, 0.0):.2f}" for name in NAMES)
            ratio = f"**{mine / turbo:.2f}x**" if turbo else "-"
            print(f"| {size >> 20} | {cells} | {ratio} |")
    print()


main()
