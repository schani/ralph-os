#!/bin/bash
set -e

echo "Building Ralph OS..."
cargo bootimage --release

echo "Starting QEMU..."
qemu-system-x86_64 \
  -drive format=raw,file=target/x86_64-ralph_os/release/bootimage-ralph_os.bin \
  -serial stdio \
  -display none \
  -no-reboot
