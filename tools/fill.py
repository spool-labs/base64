#!/usr/bin/env python3
"""Put a benchmark run into BENCHMARKS.md, where the marker is.

    tools/fill.py run.txt                          # under <!-- RESULTS -->
    tools/fill.py --marker "X86 RESULTS" a.txt b.txt
    tools/fill.py --marker "HUGE RESULTS" --tool huge a.txt b.txt

More than one run is read as a median, cell by cell, and any note already there is kept.
"""

import pathlib
import re
import subprocess
import sys


def main():
    args = sys.argv[1:]
    marker, tool, note = "RESULTS", "report", None
    while args and args[0].startswith("--"):
        flag = args.pop(0)
        if flag == "--marker":
            marker = args.pop(0)
        elif flag == "--tool":
            tool = args.pop(0)
        elif flag == "--note":
            note = args.pop(0)
        else:
            sys.exit(f"unknown flag {flag}")
    if not args:
        sys.exit(__doc__)

    tables = subprocess.run(
        [sys.executable, f"tools/{tool}.py", *args],
        capture_output=True,
        text=True,
        check=True,
    ).stdout.strip()

    tag = f"<!-- {marker} -->"
    path = pathlib.Path("BENCHMARKS.md")
    text = path.read_text()
    if tag not in text:
        sys.exit(f"no {tag} in BENCHMARKS.md")
    head, _, rest = text.partition(tag)
    # Everything from the marker to the next heading is the old table
    tail = rest[rest.index("\n## ") :] if "\n## " in rest else ""
    if note is None:
        kept = re.match(r"\s*((?:(?!###)[^\n]+\n?)+)", rest)
        note = kept.group(1).strip() if kept else ""
    middle = f"{note}\n\n" if note else ""
    path.write_text(f"{head}{tag}\n\n{middle}{tables}\n{tail}")
    print(f"filled {marker} with {len(tables.splitlines())} lines")


if __name__ == "__main__":
    main()
