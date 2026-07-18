#!/usr/bin/env python3
"""Upload a host file into the RYMOS persistent filesystem image."""

from __future__ import annotations

import argparse
from pathlib import Path


SECTOR_SIZE = 512
MAGIC = b"RYMFS5\0\0"
ENTRY_COUNT = 256
ENTRY_BASE = 16
MAX_EXTENTS = 4
EXTENT_ENTRY_SIZE = 8
EXTENT_COUNT_OFFSET = 6
EXTENTS_OFFSET = 7
EXTENTS_BYTES = MAX_EXTENTS * EXTENT_ENTRY_SIZE
CREATED_OFFSET = EXTENTS_OFFSET + EXTENTS_BYTES
MODIFIED_OFFSET = CREATED_OFFSET + 8
MODE_OFFSET = MODIFIED_OFFSET + 8
NAME_OFFSET = MODE_OFFSET + 1
NAME_MAX = 96
ENTRY_SIZE = NAME_OFFSET + NAME_MAX
HEADER_SECTORS = (16 + ENTRY_COUNT * ENTRY_SIZE + SECTOR_SIZE - 1) // SECTOR_SIZE
HEADER_BYTES = HEADER_SECTORS * SECTOR_SIZE
SECTORS_PER_FILE = 524288
FILE_MAX = SECTORS_PER_FILE * SECTOR_SIZE
DATA_START = HEADER_SECTORS
DISK_SECTORS = 8_388_608
KIND_FILE = 1
KIND_DIR = 2
MODE_READ = 0b001
MODE_WRITE = 0b010
MODE_EXEC = 0b100
MODE_DEFAULT_FILE = MODE_READ | MODE_WRITE
MODE_DEFAULT_DIR = MODE_READ | MODE_WRITE | MODE_EXEC


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("image", type=Path)
    parser.add_argument("source", type=Path)
    parser.add_argument("dest", help="RYMFS path, for example uploads/blob.bin")
    parser.add_argument("--format", action="store_true", help="format the RYMFS header first")
    args = parser.parse_args()

    data = args.source.read_bytes()
    if len(data) > FILE_MAX:
        raise SystemExit(f"source is too large: {len(data)} > {FILE_MAX}")
    if not valid_path(args.dest):
        raise SystemExit(f"invalid RYMFS destination: {args.dest}")

    ensure_disk(args.image)
    with args.image.open("r+b") as image:
        if args.format:
            header = format_header()
            image.seek(0)
            image.write(header)
        else:
            header = read_header(image)

        for parent in parent_dirs(args.dest):
            found = find_entry(header, parent)
            if found is None:
                header = create_entry(image, header, parent, b"", KIND_DIR)
            elif entry_kind(header, found) != KIND_DIR:
                raise SystemExit(f"parent is not a directory: {parent}")

        header = create_entry(image, header, args.dest, data, KIND_FILE)
        image.seek(0)
        image.write(header)
    print(f"uploaded {len(data)} bytes -> pfs:{args.dest}")


def ensure_disk(path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    minimum = DISK_SECTORS * SECTOR_SIZE
    if not path.exists():
        with path.open("wb") as handle:
            handle.truncate(minimum)
        return
    current = path.stat().st_size
    if current < minimum:
        with path.open("ab") as handle:
            handle.write(b"\0" * (minimum - current))


def format_header() -> bytearray:
    header = bytearray(HEADER_BYTES)
    header[:8] = MAGIC
    header[8] = 5
    header[9] = ENTRY_COUNT
    return header


def read_header(image) -> bytearray:
    image.seek(0)
    header = bytearray(image.read(HEADER_BYTES))
    if len(header) != HEADER_BYTES or bytes(header[:8]) != MAGIC:
        raise SystemExit("RYMFS5 not found; run with --format or format inside RYMOS")
    return header


def create_entry(image, header: bytearray, name: str, data: bytes, kind: int) -> bytearray:
    existing = find_entry(header, name)
    if existing is not None:
        index = existing
        if kind == KIND_FILE and entry_kind(header, index) == KIND_DIR:
            raise SystemExit(f"destination is a directory: {name}")
    else:
        index = free_entry(header)
        if index is None:
            raise SystemExit("RYMFS directory is full")

    if kind == KIND_FILE:
        sectors = sectors_for_len(len(data))
        extents = [] if sectors == 0 else alloc_extents(header, sectors, index)
        write_file_data(image, extents, data)
    else:
        extents = []
    set_entry(header, index, name, len(data), kind, extents)
    return header


def write_file_data(image, extents: list[tuple[int, int]], data: bytes) -> None:
    written = 0
    for start_sector, sector_count in extents:
        if written >= len(data):
            break
        chunk = data[written : written + sector_count * SECTOR_SIZE]
        image.seek(start_sector * SECTOR_SIZE)
        image.write(chunk)
        written += len(chunk)


def find_entry(header: bytearray, name: str) -> int | None:
    encoded = name.encode("ascii")
    for index in range(ENTRY_COUNT):
        offset = entry_offset(index)
        if header[offset] == 0:
            continue
        length = header[offset + 1]
        if bytes(header[offset + NAME_OFFSET : offset + NAME_OFFSET + length]) == encoded:
            return index
    return None


def free_entry(header: bytearray) -> int | None:
    for index in range(ENTRY_COUNT):
        if header[entry_offset(index)] == 0:
            return index
    return None


def set_entry(
    header: bytearray, index: int, name: str, size: int, kind: int, extents: list[tuple[int, int]]
) -> None:
    encoded = name.encode("ascii")
    if len(encoded) > NAME_MAX:
        raise SystemExit(f"name too long: {name}")
    if len(extents) > MAX_EXTENTS:
        raise SystemExit(f"too many extents for {name}: {len(extents)} > {MAX_EXTENTS}")
    offset = entry_offset(index)
    mode = MODE_DEFAULT_DIR if kind == KIND_DIR else MODE_DEFAULT_FILE
    header[offset : offset + ENTRY_SIZE] = b"\0" * ENTRY_SIZE
    header[offset] = kind
    header[offset + 1] = len(encoded)
    header[offset + 2 : offset + 6] = size.to_bytes(4, "little")
    header[offset + EXTENT_COUNT_OFFSET] = len(extents)
    for slot, (start_sector, sector_count) in enumerate(extents):
        extent_offset = offset + EXTENTS_OFFSET + slot * EXTENT_ENTRY_SIZE
        header[extent_offset : extent_offset + 4] = start_sector.to_bytes(4, "little")
        header[extent_offset + 4 : extent_offset + 8] = sector_count.to_bytes(4, "little")
    # Host uploads have no tick-counter time source; 0 is the "unknown" sentinel.
    header[offset + CREATED_OFFSET : offset + CREATED_OFFSET + 8] = (0).to_bytes(8, "little")
    header[offset + MODIFIED_OFFSET : offset + MODIFIED_OFFSET + 8] = (0).to_bytes(8, "little")
    header[offset + MODE_OFFSET] = mode
    header[offset + NAME_OFFSET : offset + NAME_OFFSET + len(encoded)] = encoded


def entry_kind(header: bytearray, index: int | None) -> int:
    if index is None:
        return 0
    return header[entry_offset(index)]


def entry_size(header: bytearray, index: int) -> int:
    offset = entry_offset(index)
    return int.from_bytes(header[offset + 2 : offset + 6], "little")


def entry_extents(header: bytearray, index: int) -> list[tuple[int, int]]:
    offset = entry_offset(index)
    count = min(header[offset + EXTENT_COUNT_OFFSET], MAX_EXTENTS)
    extents = []
    for slot in range(count):
        extent_offset = offset + EXTENTS_OFFSET + slot * EXTENT_ENTRY_SIZE
        start = int.from_bytes(header[extent_offset : extent_offset + 4], "little")
        sectors = int.from_bytes(header[extent_offset + 4 : extent_offset + 8], "little")
        if sectors:
            extents.append((start, sectors))
    return extents


def find_free_run(
    header: bytearray, start_from: int, max_len: int, skip_index: int | None
) -> tuple[int, int] | None:
    candidate = start_from
    while True:
        if candidate >= DISK_SECTORS:
            return None
        next_obstacle = None
        blocked = False
        for index in range(ENTRY_COUNT):
            if index == skip_index or header[entry_offset(index)] != KIND_FILE:
                continue
            for used_start, used_sectors in entry_extents(header, index):
                used_end = used_start + used_sectors
                if used_start <= candidate < used_end:
                    candidate = used_end
                    blocked = True
                    break
                if used_start > candidate and (next_obstacle is None or used_start < next_obstacle):
                    next_obstacle = used_start
            if blocked:
                break
        if blocked:
            continue
        end_bound = min(next_obstacle if next_obstacle is not None else DISK_SECTORS, DISK_SECTORS)
        if end_bound <= candidate:
            return None
        take = min(end_bound - candidate, max_len)
        if take == 0:
            return None
        return candidate, take


def alloc_extents(header: bytearray, sectors_needed: int, skip_index: int | None) -> list[tuple[int, int]]:
    extents: list[tuple[int, int]] = []
    remaining = sectors_needed
    candidate = DATA_START
    while remaining > 0:
        if len(extents) >= MAX_EXTENTS:
            raise SystemExit("RYMFS disk too fragmented to fit this file in 4 extents")
        run = find_free_run(header, candidate, remaining, skip_index)
        if run is None:
            raise SystemExit("RYMFS disk has no room for this file")
        run_start, run_len = run
        extents.append((run_start, run_len))
        remaining -= run_len
        candidate = run_start + run_len
    return extents


def sectors_for_len(length: int) -> int:
    if length == 0:
        return 0
    return (length + SECTOR_SIZE - 1) // SECTOR_SIZE


def entry_offset(index: int) -> int:
    return ENTRY_BASE + index * ENTRY_SIZE


def parent_dirs(path: str) -> list[str]:
    parts = path.split("/")[:-1]
    return ["/".join(parts[: index + 1]) for index in range(len(parts))]


def valid_path(path: str) -> bool:
    if not path or len(path.encode("ascii", "ignore")) != len(path) or len(path) > NAME_MAX:
        return False
    if path.startswith("/") or path.endswith("/") or "//" in path:
        return False
    allowed = set("abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789._-/")
    return all(char in allowed for char in path)


if __name__ == "__main__":
    main()
