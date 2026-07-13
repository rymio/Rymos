#!/usr/bin/env python3
"""Build the tiny read-only RYMOS boot filesystem archive."""

from __future__ import annotations

import argparse
from pathlib import Path

MAGIC = b"RYFS1\0\0\0"
ENTRY_SIZE = 42
NAME_MAX = 32


def le16(value: int) -> bytes:
    return value.to_bytes(2, "little")


def le32(value: int) -> bytes:
    return value.to_bytes(4, "little")


def build(source: Path, output: Path) -> None:
    files = [path for path in sorted(source.rglob("*")) if path.is_file()]
    header_size = len(MAGIC) + 2 + 2 + len(files) * ENTRY_SIZE
    offset = header_size
    entries = bytearray()
    payload = bytearray()

    for path in files:
        name = path.relative_to(source).as_posix().encode("ascii")
        data = path.read_bytes()
        if len(name) > NAME_MAX:
            raise ValueError(f"bootfs name too long: {name!r}")

        entries.extend(bytes([1, len(name)]))
        entries.extend(le32(offset))
        entries.extend(le32(len(data)))
        entries.extend(name.ljust(NAME_MAX, b"\0"))
        payload.extend(data)
        offset += len(data)

    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_bytes(MAGIC + le16(len(files)) + le16(ENTRY_SIZE) + entries + payload)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    build(args.source, args.output)


if __name__ == "__main__":
    main()
