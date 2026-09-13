#!/usr/bin/env python3
"""Turn criterion's output into the tables in BENCHMARKS.md.

Reads a criterion run from a file or stdin and prints one table per direction, in
GiB/s, with what this crate did against the best of the rest.
"""

import collections
import re
import statistics
import sys

NAMES = ["tape-base64", "base64-simd", "base64-turbo", "base64-simd-engine", "base64", "firedancer"]
UNITS = {"GiB": 1.0, "MiB": 1 / 1024, "KiB": 1 / 1024 / 1024, "TiB": 1024.0}

# criterion picks whatever unit fits, so a run mixes them freely
TIMES = {"ps": 0.001, "ns": 1.0, "\u00b5s": 1000.0, "us": 1000.0, "ms": 1e6, "s": 1e9}


def parse(lines):
    results = collections.defaultdict(dict)
    current = None
    for line in lines:
        header = re.match(r"^(encode|decode)/([\w\-]+)/(\d+)", line)
        if header:
            current = (header.group(1), header.group(2), int(header.group(3)))
        rate = re.search(r"thrpt:\s+\[[\d.]+ \S+ ([\d.]+) (\w+)/s", line)
        if rate and current:
            results[(current[0], current[2])][current[1]] = float(rate.group(1)) * UNITS[rate.group(2)]
            current = None
    return results


def median_of(runs):
    """One table from several runs, each cell the median of what they said."""
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
        print("| bytes | " + " | ".join(NAMES) + " | vs best other |")
        print("|---" * (len(NAMES) + 2) + "|")
        for size, row in rows:
            mine = row.get("tape-base64", 0.0)
            best = max((row.get(name, 0.0) for name in NAMES[1:]), default=0.0)
            cells = " | ".join(f"{row[name]:.2f}" if name in row else "-" for name in NAMES)
            ratio = f"{mine / best:.2f}x" if best else "-"
            print(f"| {size} | {cells} | **{ratio}** |")


if __name__ == "__main__":
    main()
