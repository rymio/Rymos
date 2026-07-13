#!/usr/bin/env python3
"""Create or grow the persistent RYMOS data disk."""

from __future__ import annotations

import argparse
from pathlib import Path


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    parser.add_argument("--size-mib", type=int, default=4096)
    args = parser.parse_args()

    args.output.parent.mkdir(parents=True, exist_ok=True)
    size = args.size_mib * 1024 * 1024
    if args.output.exists():
        current = args.output.stat().st_size
        if current >= size:
            return
        with args.output.open("ab") as handle:
            handle.truncate(size)
        return

    with args.output.open("wb") as handle:
        handle.truncate(size)


if __name__ == "__main__":
    main()
