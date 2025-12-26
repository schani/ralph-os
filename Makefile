.PHONY: build run debug clean setup help

# Default target
all: build

# Build the bootable kernel image
build:
	cargo bootimage --release

# Build and run in QEMU
run: build
	qemu-system-x86_64 \
		-drive format=raw,file=target/x86_64-ralph_os/release/bootimage-ralph_os.bin \
		-serial stdio \
		-display none \
		-no-reboot

# Run with QEMU debug output (interrupt logging)
debug: build
	qemu-system-x86_64 \
		-drive format=raw,file=target/x86_64-ralph_os/release/bootimage-ralph_os.bin \
		-serial stdio \
		-display none \
		-no-reboot \
		-d int,cpu_reset

# Run with GDB server (for debugging with gdb)
gdb: build
	qemu-system-x86_64 \
		-drive format=raw,file=target/x86_64-ralph_os/release/bootimage-ralph_os.bin \
		-serial stdio \
		-display none \
		-no-reboot \
		-s -S

# Clean build artifacts
clean:
	cargo clean

# Install required tools and set up environment
setup:
	rustup override set nightly
	rustup component add rust-src --toolchain nightly
	rustup component add llvm-tools-preview --toolchain nightly
	cargo install bootimage

# Show help
help:
	@echo "Ralph OS Build System"
	@echo ""
	@echo "Usage: make [target]"
	@echo ""
	@echo "Targets:"
	@echo "  build   - Build the kernel image (default)"
	@echo "  run     - Build and run in QEMU"
	@echo "  debug   - Run with QEMU interrupt logging"
	@echo "  gdb     - Run with GDB server on port 1234"
	@echo "  clean   - Remove build artifacts"
	@echo "  setup   - Install required tools (run once)"
	@echo "  help    - Show this help message"
