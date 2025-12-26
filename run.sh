#!/bin/bash
set -e

echo "Building Ralph OS..."
make image

echo "Starting QEMU..."
qemu-system-x86_64 \
  -drive format=raw,file=target/ralph_os.img \
  -serial stdio \
  -display none \
  -no-reboot
