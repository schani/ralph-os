#!/usr/bin/env python3
"""
Generate executable table for Ralph OS

This script creates a binary executable table header that the kernel uses
to find embedded executables. It also concatenates all the ELF files.

Usage: make_exec_table.py <output> <elf1> [elf2] ...

The output is a single binary file containing:
- 512-byte header with magic, version, count, and entries
- Concatenated ELF files (padded to 512-byte alignment)
"""

import sys
import os
import struct

# Constants matching kernel code
MAGIC = b"REXE"
VERSION = 1
MAX_EXECUTABLES = 15
HEADER_SIZE = 512
SECTOR_SIZE = 512

def align_up(value, alignment):
    """Align value up to the given alignment."""
    return (value + alignment - 1) & ~(alignment - 1)

def make_entry(name, offset, size):
    """Create a 32-byte executable entry."""
    # Name: 16 bytes, null-terminated
    name_bytes = name.encode('utf-8')[:15]
    name_padded = name_bytes.ljust(16, b'\x00')

    # offset: u32, size: u32, reserved: u32 * 2
    return struct.pack('<16sIIII', name_padded, offset, size, 0, 0)

def main():
    if len(sys.argv) < 2:
        print(f"Usage: {sys.argv[0]} <output> [elf1] [elf2] ...")
        sys.exit(1)

    output_path = sys.argv[1]
    elf_paths = sys.argv[2:]

    if len(elf_paths) > MAX_EXECUTABLES:
        print(f"Error: Too many executables ({len(elf_paths)} > {MAX_EXECUTABLES})")
        sys.exit(1)

    # Collect ELF info
    elfs = []
    for path in elf_paths:
        if not os.path.exists(path):
            print(f"Error: File not found: {path}")
            sys.exit(1)

        name = os.path.splitext(os.path.basename(path))[0]
        size = os.path.getsize(path)
        elfs.append((name, path, size))
        print(f"  {name}: {size} bytes")

    # Build header
    # Magic (4) + version (4) + count (4) + reserved (4) = 16 bytes
    # Entries: 15 * 32 = 480 bytes
    # Total: 496 bytes (fits in 512-byte sector)

    header = bytearray(HEADER_SIZE)

    # Magic
    header[0:4] = MAGIC

    # Version
    struct.pack_into('<I', header, 4, VERSION)

    # Count
    struct.pack_into('<I', header, 8, len(elfs))

    # Reserved
    struct.pack_into('<I', header, 12, 0)

    # Calculate offsets (relative to header start)
    # First ELF starts right after header (512 bytes)
    current_offset = HEADER_SIZE

    entries = []
    for name, path, size in elfs:
        entries.append((name, current_offset, size))
        # Align next ELF to sector boundary
        current_offset += align_up(size, SECTOR_SIZE)

    # Write entries
    for i, (name, offset, size) in enumerate(entries):
        entry_offset = 16 + i * 32
        entry_data = make_entry(name, offset, size)
        header[entry_offset:entry_offset + 32] = entry_data

    # Write output file
    with open(output_path, 'wb') as f:
        # Write header
        f.write(header)

        # Write ELF files with padding
        for name, path, size in elfs:
            with open(path, 'rb') as elf:
                data = elf.read()
                f.write(data)

                # Pad to sector boundary
                padding = align_up(size, SECTOR_SIZE) - size
                if padding > 0:
                    f.write(b'\x00' * padding)

    total_size = os.path.getsize(output_path)
    print(f"Created {output_path}: {total_size} bytes ({len(elfs)} executables)")

if __name__ == '__main__':
    main()
