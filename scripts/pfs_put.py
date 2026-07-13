#!/usr/bin/env python3
"""Upload a host file into the RYMOS persistent filesystem image."""

from __future__ import annotations

import argparse
from pathlib import Path


SECTOR_SIZE = 512
MAGIC = b"RYMFS3\0\0"
HEADER_SECTORS = 8
HEADER_BYTES = HEADER_SECTORS * SECTOR_SIZE
ENTRY_COUNT = 96
ENTRY_SIZE = 40
ENTRY_BASE = 16
NAME_MAX = 30
SECTORS_PER_FILE = 524288
FILE_MAX = SECTORS_PER_FILE * SECTOR_SIZE
DATA_START = HEADER_SECTORS
DISK_SECTORS = 8_388_608
KIND_FILE = 1
KIND_DIR = 2


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
    header[8] = 3
    header[9] = ENTRY_COUNT
    return header


def read_header(image) -> bytearray:
    image.seek(0)
    header = bytearray(image.read(HEADER_BYTES))
    if len(header) != HEADER_BYTES or bytes(header[:8]) != MAGIC:
        raise SystemExit("RYMFS3 not found; run with --format or format inside RYMOS")
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
        start_sector = 0 if sectors == 0 else alloc_extent(header, sectors, index)
        write_file_data(image, start_sector, data)
    else:
        start_sector = 0
    set_entry(header, index, name, len(data), kind, start_sector)
    return header


def write_file_data(image, start_sector: int, data: bytes) -> None:
    if not data:
        return
    start = start_sector * SECTOR_SIZE
    image.seek(start)
    image.write(data)


def find_entry(header: bytearray, name: str) -> int | None:
    encoded = name.encode("ascii")
    for index in range(ENTRY_COUNT):
        offset = entry_offset(index)
        if header[offset] == 0:
            continue
        length = header[offset + 1]
        if bytes(header[offset + 10 : offset + 10 + length]) == encoded:
            return index
    return None


def free_entry(header: bytearray) -> int | None:
    for index in range(ENTRY_COUNT):
        if header[entry_offset(index)] == 0:
            return index
    return None


def set_entry(header: bytearray, index: int, name: str, size: int, kind: int, start_sector: int) -> None:
    encoded = name.encode("ascii")
    if len(encoded) > NAME_MAX:
        raise SystemExit(f"name too long: {name}")
    offset = entry_offset(index)
    header[offset : offset + ENTRY_SIZE] = b"\0" * ENTRY_SIZE
    header[offset] = kind
    header[offset + 1] = len(encoded)
    header[offset + 2 : offset + 6] = size.to_bytes(4, "little")
    header[offset + 6 : offset + 10] = start_sector.to_bytes(4, "little")
    header[offset + 10 : offset + 10 + len(encoded)] = encoded


def entry_kind(header: bytearray, index: int | None) -> int:
    if index is None:
        return 0
    return header[entry_offset(index)]


def entry_size(header: bytearray, index: int) -> int:
    offset = entry_offset(index)
    return int.from_bytes(header[offset + 2 : offset + 6], "little")


def entry_start(header: bytearray, index: int) -> int:
    offset = entry_offset(index)
    return int.from_bytes(header[offset + 6 : offset + 10], "little")


def alloc_extent(header: bytearray, sectors: int, skip_index: int | None) -> int:
    candidate = DATA_START
    while candidate + sectors <= DISK_SECTORS:
        overlaps = False
        for index in range(ENTRY_COUNT):
            if index == skip_index or header[entry_offset(index)] != KIND_FILE:
                continue
            used_start = entry_start(header, index)
            used_sectors = sectors_for_len(entry_size(header, index))
            if used_start == 0 or used_sectors == 0:
                continue
            used_end = used_start + used_sectors
            candidate_end = candidate + sectors
            if candidate < used_end and used_start < candidate_end:
                candidate = used_end
                overlaps = True
                break
        if not overlaps:
            return candidate
    raise SystemExit("RYMFS disk has no contiguous free extent")


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
