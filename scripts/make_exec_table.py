#!/usr/bin/env python3
"""
Generate embedded file table for Ralph OS

This script creates a binary table header that the kernel uses to find
embedded files. It concatenates all provided files, and records their
name/offset/size in a small header.

Usage: make_exec_table.py <output> <file1> [file2] ...

The output is a single binary file containing:
- 512-byte header with magic, version, count, and entries
- Concatenated file blobs (padded to 512-byte alignment)
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
        print(f"Usage: {sys.argv[0]} <output> [file1] [file2] ...")
        sys.exit(1)

    output_path = sys.argv[1]
    file_paths = sys.argv[2:]

    if len(file_paths) > MAX_EXECUTABLES:
        print(f"Error: Too many entries ({len(file_paths)} > {MAX_EXECUTABLES})")
        sys.exit(1)

    # Collect file info
    files = []
    for path in file_paths:
        if not os.path.exists(path):
            print(f"Error: File not found: {path}")
            sys.exit(1)

        base = os.path.basename(path)
        ext = os.path.splitext(base)[1].lower()

        # Keep `.bas` extension so BASIC can request "name.bas" explicitly.
        # For ELF executables, keep the historical behavior of stripping extension.
        if ext == ".bas":
            name = base
        else:
            name = os.path.splitext(base)[0]
        size = os.path.getsize(path)
        files.append((name, path, size))
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
    struct.pack_into('<I', header, 8, len(files))

    # Reserved
    struct.pack_into('<I', header, 12, 0)

    # Calculate offsets (relative to header start)
    # First file starts right after header (512 bytes)
    current_offset = HEADER_SIZE

    entries = []
    for name, path, size in files:
        entries.append((name, current_offset, size))
        # Align next file to sector boundary
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

        # Write files with padding
        for name, path, size in files:
            with open(path, 'rb') as fp:
                data = fp.read()
                f.write(data)

                # Pad to sector boundary
                padding = align_up(size, SECTOR_SIZE) - size
                if padding > 0:
                    f.write(b'\x00' * padding)

    total_size = os.path.getsize(output_path)
    print(f"Created {output_path}: {total_size} bytes ({len(files)} entries)")

if __name__ == '__main__':
    main()
