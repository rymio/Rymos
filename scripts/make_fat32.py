#!/usr/bin/env python3
"""Create a minimal FAT32 UEFI boot image for RYMOS."""

from __future__ import annotations

import argparse
import math
from pathlib import Path

BYTES_PER_SECTOR = 512
SECTORS_PER_CLUSTER = 1
RESERVED_SECTORS = 32
FAT_COUNT = 2
ROOT_CLUSTER = 2
MEDIA_DESCRIPTOR = 0xF8
END_OF_CHAIN = 0x0FFFFFFF


def le16(value: int) -> bytes:
    return value.to_bytes(2, "little")


def le32(value: int) -> bytes:
    return value.to_bytes(4, "little")


def short_entry(name: bytes, attr: int, first_cluster: int, size: int = 0) -> bytes:
    if len(name) != 11:
        raise ValueError(f"FAT short name must be 11 bytes: {name!r}")

    entry = bytearray(32)
    entry[0:11] = name
    entry[11] = attr
    entry[20:22] = le16((first_cluster >> 16) & 0xFFFF)
    entry[26:28] = le16(first_cluster & 0xFFFF)
    entry[28:32] = le32(size)
    return bytes(entry)


def cluster_offset(first_data_sector: int, cluster: int) -> int:
    sector = first_data_sector + (cluster - 2) * SECTORS_PER_CLUSTER
    return sector * BYTES_PER_SECTOR


def write_cluster(image: bytearray, first_data_sector: int, cluster: int, payload: bytes) -> None:
    start = cluster_offset(first_data_sector, cluster)
    length = BYTES_PER_SECTOR * SECTORS_PER_CLUSTER
    image[start : start + length] = payload.ljust(length, b"\x00")[:length]


def calculate_layout(total_sectors: int, file_sizes: list[int]) -> tuple[int, int, int]:
    fat_sectors = 1
    while True:
        data_sectors = total_sectors - RESERVED_SECTORS - FAT_COUNT * fat_sectors
        cluster_count = data_sectors // SECTORS_PER_CLUSTER
        required_fat_sectors = math.ceil((cluster_count + 2) * 4 / BYTES_PER_SECTOR)
        if required_fat_sectors <= fat_sectors:
            file_clusters = sum(
                math.ceil(file_size / (BYTES_PER_SECTOR * SECTORS_PER_CLUSTER))
                for file_size in file_sizes
            )
            if cluster_count < 65525:
                raise ValueError("image is too small for FAT32")
            if cluster_count < 3 + file_clusters:
                raise ValueError("image is too small for BOOTX64.EFI")
            return fat_sectors, data_sectors, cluster_count
        fat_sectors = required_fat_sectors


def build_image(boot_efi: Path, kernel_elf: Path, initrd: Path, output: Path, size_mib: int) -> None:
    boot_payload = boot_efi.read_bytes()
    kernel_payload = kernel_elf.read_bytes()
    initrd_payload = initrd.read_bytes()
    total_sectors = size_mib * 1024 * 1024 // BYTES_PER_SECTOR
    fat_sectors, _data_sectors, cluster_count = calculate_layout(
        total_sectors, [len(boot_payload), len(kernel_payload), len(initrd_payload)]
    )
    first_data_sector = RESERVED_SECTORS + FAT_COUNT * fat_sectors
    image = bytearray(total_sectors * BYTES_PER_SECTOR)

    boot = bytearray(BYTES_PER_SECTOR)
    boot[0:3] = b"\xEB\x58\x90"
    boot[3:11] = b"RYMOS1  "
    boot[11:13] = le16(BYTES_PER_SECTOR)
    boot[13] = SECTORS_PER_CLUSTER
    boot[14:16] = le16(RESERVED_SECTORS)
    boot[16] = FAT_COUNT
    boot[17:19] = le16(0)
    boot[19:21] = le16(0)
    boot[21] = MEDIA_DESCRIPTOR
    boot[22:24] = le16(0)
    boot[24:26] = le16(63)
    boot[26:28] = le16(255)
    boot[28:32] = le32(0)
    boot[32:36] = le32(total_sectors)
    boot[36:40] = le32(fat_sectors)
    boot[40:42] = le16(0)
    boot[42:44] = le16(0)
    boot[44:48] = le32(ROOT_CLUSTER)
    boot[48:50] = le16(1)
    boot[50:52] = le16(6)
    boot[64] = 0x80
    boot[66] = 0x29
    boot[67:71] = le32(0x20260713)
    boot[71:82] = b"RYMOS      "
    boot[82:90] = b"FAT32   "
    boot[510:512] = b"\x55\xAA"
    image[0:BYTES_PER_SECTOR] = boot
    image[6 * BYTES_PER_SECTOR : 7 * BYTES_PER_SECTOR] = boot

    fsinfo = bytearray(BYTES_PER_SECTOR)
    fsinfo[0:4] = le32(0x41615252)
    fsinfo[484:488] = le32(0x61417272)
    fsinfo[488:492] = le32(0xFFFFFFFF)
    fsinfo[492:496] = le32(5)
    fsinfo[508:512] = b"\x00\x00\x55\xAA"
    image[BYTES_PER_SECTOR : 2 * BYTES_PER_SECTOR] = fsinfo
    image[7 * BYTES_PER_SECTOR : 8 * BYTES_PER_SECTOR] = fsinfo

    boot_cluster_count = math.ceil(len(boot_payload) / (BYTES_PER_SECTOR * SECTORS_PER_CLUSTER))
    kernel_cluster_count = math.ceil(len(kernel_payload) / (BYTES_PER_SECTOR * SECTORS_PER_CLUSTER))
    initrd_cluster_count = math.ceil(len(initrd_payload) / (BYTES_PER_SECTOR * SECTORS_PER_CLUSTER))
    boot_clusters = list(range(5, 5 + boot_cluster_count))
    kernel_clusters = list(range(5 + boot_cluster_count, 5 + boot_cluster_count + kernel_cluster_count))
    initrd_clusters = list(
        range(
            5 + boot_cluster_count + kernel_cluster_count,
            5 + boot_cluster_count + kernel_cluster_count + initrd_cluster_count,
        )
    )
    fat_entries = [0] * (cluster_count + 2)
    fat_entries[0] = 0x0FFFFF00 | MEDIA_DESCRIPTOR
    fat_entries[1] = END_OF_CHAIN
    fat_entries[ROOT_CLUSTER] = END_OF_CHAIN
    fat_entries[3] = END_OF_CHAIN
    fat_entries[4] = END_OF_CHAIN
    for clusters in [boot_clusters, kernel_clusters, initrd_clusters]:
        for index, cluster in enumerate(clusters):
            fat_entries[cluster] = clusters[index + 1] if index + 1 < len(clusters) else END_OF_CHAIN

    fat = bytearray(fat_sectors * BYTES_PER_SECTOR)
    for index, value in enumerate(fat_entries):
        fat[index * 4 : index * 4 + 4] = le32(value)

    first_fat = RESERVED_SECTORS * BYTES_PER_SECTOR
    second_fat = first_fat + fat_sectors * BYTES_PER_SECTOR
    image[first_fat : first_fat + len(fat)] = fat
    image[second_fat : second_fat + len(fat)] = fat

    root_dir = (
        short_entry(b"EFI        ", 0x10, 3)
        + short_entry(b"KERNEL  ELF", 0x20, kernel_clusters[0], len(kernel_payload))
        + short_entry(b"INITRD  RFS", 0x20, initrd_clusters[0], len(initrd_payload))
    )
    efi_dir = (
        short_entry(b".          ", 0x10, 3)
        + short_entry(b"..         ", 0x10, ROOT_CLUSTER)
        + short_entry(b"BOOT       ", 0x10, 4)
    )
    boot_dir = (
        short_entry(b".          ", 0x10, 4)
        + short_entry(b"..         ", 0x10, 3)
        + short_entry(b"BOOTX64 EFI", 0x20, boot_clusters[0], len(boot_payload))
    )
    write_cluster(image, first_data_sector, ROOT_CLUSTER, root_dir)
    write_cluster(image, first_data_sector, 3, efi_dir)
    write_cluster(image, first_data_sector, 4, boot_dir)

    cluster_size = BYTES_PER_SECTOR * SECTORS_PER_CLUSTER
    for payload, clusters in [
        (boot_payload, boot_clusters),
        (kernel_payload, kernel_clusters),
        (initrd_payload, initrd_clusters),
    ]:
        for index, cluster in enumerate(clusters):
            chunk = payload[index * cluster_size : (index + 1) * cluster_size]
            write_cluster(image, first_data_sector, cluster, chunk)

    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_bytes(image)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("boot_efi", type=Path)
    parser.add_argument("kernel_elf", type=Path)
    parser.add_argument("initrd", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--size-mib", type=int, default=64)
    args = parser.parse_args()
    build_image(args.boot_efi, args.kernel_elf, args.initrd, args.output, args.size_mib)


if __name__ == "__main__":
    main()
