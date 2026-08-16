#!/usr/bin/env python3
"""Append a FAT16 EFI System Partition to the BlueOS BIOS image.

The result is a 64 MiB MBR-partitioned image. Sector zero remains the custom
BlueOS BIOS loader, while UEFI firmware finds EFI/BOOT/BOOTX64.EFI in the ESP.
No host filesystem tools or root privileges are required.
"""

from __future__ import annotations

import math
import pathlib
import struct
import sys

SECTOR = 512
DISK_SECTORS = 131_072  # 64 MiB
ESP_START = 2_048       # 1 MiB alignment
ESP_SECTORS = DISK_SECTORS - ESP_START
SECTORS_PER_CLUSTER = 4
RESERVED = 1
FATS = 2
ROOT_ENTRIES = 512
ROOT_SECTORS = ROOT_ENTRIES * 32 // SECTOR


def short_entry(name: bytes, attr: int, cluster: int, size: int = 0) -> bytes:
    if len(name) != 11:
        raise ValueError(f"short FAT name must be 11 bytes: {name!r}")
    entry = bytearray(32)
    entry[0:11] = name
    entry[11] = attr
    struct.pack_into("<H", entry, 26, cluster)
    struct.pack_into("<I", entry, 28, size)
    return bytes(entry)


def directory_cluster(own: int, parent: int, entries: list[bytes], cluster_size: int) -> bytes:
    data = bytearray(cluster_size)
    data[0:32] = short_entry(b".          ", 0x10, own)
    data[32:64] = short_entry(b"..         ", 0x10, parent)
    offset = 64
    for entry in entries:
        data[offset:offset + 32] = entry
        offset += 32
    return bytes(data)


def main() -> int:
    if len(sys.argv) != 3:
        print(f"usage: {sys.argv[0]} BIOS_IMAGE BOOTX64.EFI", file=sys.stderr)
        return 2

    image_path = pathlib.Path(sys.argv[1])
    efi_path = pathlib.Path(sys.argv[2])
    image = bytearray(image_path.read_bytes())
    efi = efi_path.read_bytes()
    if len(image) > ESP_START * SECTOR:
        raise SystemExit("BIOS image overlaps the EFI partition at LBA 2048")
    if image[510:512] != b"\x55\xaa":
        raise SystemExit("BIOS image has no 55 AA boot signature")

    # Determine FAT size; the FAT itself changes the number of data clusters.
    fat_sectors = 1
    while True:
        data_sectors = ESP_SECTORS - RESERVED - FATS * fat_sectors - ROOT_SECTORS
        clusters = data_sectors // SECTORS_PER_CLUSTER
        required = math.ceil((clusters + 2) * 2 / SECTOR)
        if required == fat_sectors:
            break
        fat_sectors = required
    if not 4_085 <= clusters < 65_525:
        raise SystemExit(f"partition does not form FAT16 ({clusters} clusters)")

    cluster_size = SECTOR * SECTORS_PER_CLUSTER
    file_clusters = max(1, math.ceil(len(efi) / cluster_size))
    first_file_cluster = 4
    last_file_cluster = first_file_cluster + file_clusters - 1
    if last_file_cluster >= clusters + 2:
        raise SystemExit("BOOTX64.EFI does not fit in the EFI partition")

    image.extend(b"\0" * (DISK_SECTORS * SECTOR - len(image)))

    # Legacy MBR partition table. The boot code occupies bytes 0..445.
    partition = struct.pack(
        "<B3sB3sII",
        0x00,
        b"\xfe\xff\xff",
        0xEF,
        b"\xfe\xff\xff",
        ESP_START,
        ESP_SECTORS,
    )
    image[446:462] = partition
    image[462:510] = b"\0" * 48
    image[510:512] = b"\x55\xaa"

    base = ESP_START * SECTOR
    boot = bytearray(SECTOR)
    boot[0:3] = b"\xeb\x3c\x90"
    boot[3:11] = b"BLUEOS  "
    struct.pack_into("<H", boot, 11, SECTOR)
    boot[13] = SECTORS_PER_CLUSTER
    struct.pack_into("<H", boot, 14, RESERVED)
    boot[16] = FATS
    struct.pack_into("<H", boot, 17, ROOT_ENTRIES)
    struct.pack_into("<H", boot, 19, 0)  # use 32-bit total sector count
    boot[21] = 0xF8
    struct.pack_into("<H", boot, 22, fat_sectors)
    struct.pack_into("<H", boot, 24, 63)
    struct.pack_into("<H", boot, 26, 255)
    struct.pack_into("<I", boot, 28, ESP_START)
    struct.pack_into("<I", boot, 32, ESP_SECTORS)
    boot[36] = 0x80
    boot[38] = 0x29
    struct.pack_into("<I", boot, 39, 0xB10E_0502)
    boot[43:54] = b"BLUEOS     "
    boot[54:62] = b"FAT16   "
    boot[510:512] = b"\x55\xaa"
    image[base:base + SECTOR] = boot

    fat = bytearray(fat_sectors * SECTOR)
    struct.pack_into("<H", fat, 0, 0xFFF8)
    struct.pack_into("<H", fat, 2, 0xFFFF)
    struct.pack_into("<H", fat, 2 * 2, 0xFFFF)  # EFI directory
    struct.pack_into("<H", fat, 3 * 2, 0xFFFF)  # BOOT directory
    for cluster in range(first_file_cluster, last_file_cluster + 1):
        following = 0xFFFF if cluster == last_file_cluster else cluster + 1
        struct.pack_into("<H", fat, cluster * 2, following)
    for copy in range(FATS):
        offset = base + (RESERVED + copy * fat_sectors) * SECTOR
        image[offset:offset + len(fat)] = fat

    root_offset = base + (RESERVED + FATS * fat_sectors) * SECTOR
    root = bytearray(ROOT_SECTORS * SECTOR)
    root[0:32] = short_entry(b"BLUEOS     ", 0x08, 0)
    root[32:64] = short_entry(b"EFI        ", 0x10, 2)
    image[root_offset:root_offset + len(root)] = root

    data_offset = root_offset + len(root)

    def cluster_offset(cluster: int) -> int:
        return data_offset + (cluster - 2) * cluster_size

    efi_dir = directory_cluster(2, 0, [short_entry(b"BOOT       ", 0x10, 3)], cluster_size)
    boot_dir = directory_cluster(
        3,
        2,
        [short_entry(b"BOOTX64 EFI", 0x20, first_file_cluster, len(efi))],
        cluster_size,
    )
    image[cluster_offset(2):cluster_offset(2) + cluster_size] = efi_dir
    image[cluster_offset(3):cluster_offset(3) + cluster_size] = boot_dir
    file_offset = cluster_offset(first_file_cluster)
    image[file_offset:file_offset + len(efi)] = efi

    image_path.write_bytes(image)
    print(
        f"Hybrid image: {image_path} ({len(image)} bytes), "
        f"ESP LBA {ESP_START}, FAT16, BOOTX64.EFI {len(efi)} bytes"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
