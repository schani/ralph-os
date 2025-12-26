.PHONY: all build run debug clean setup help bootloader kernel image

# Output files
BUILD_DIR       = target
BOOT_DIR        = bootloader
STAGE1          = $(BUILD_DIR)/stage1.bin
STAGE2          = $(BUILD_DIR)/stage2.bin
KERNEL          = $(BUILD_DIR)/x86_64-ralph_os/release/ralph_os
KERNEL_BIN      = $(BUILD_DIR)/kernel.bin
OS_IMAGE        = $(BUILD_DIR)/ralph_os.img

# Tools
NASM            = nasm
OBJCOPY         = $(shell find ~/.rustup -name 'llvm-objcopy' 2>/dev/null | head -1)
QEMU            = qemu-system-x86_64

# Default target
all: image

# Create build directory
$(BUILD_DIR):
	mkdir -p $(BUILD_DIR)

# Build stage 1 bootloader
$(STAGE1): $(BOOT_DIR)/stage1.asm | $(BUILD_DIR)
	$(NASM) -f bin -o $@ $<

# Build stage 2 bootloader (padded to 8KB = 16 sectors)
$(STAGE2): $(BOOT_DIR)/stage2.asm | $(BUILD_DIR)
	$(NASM) -f bin -o $@ $<
	@# Pad to exactly 8KB (16 sectors)
	@truncate -s 8192 $@

# Build bootloader (both stages)
bootloader: $(STAGE1) $(STAGE2)

# Build kernel
$(KERNEL): src/*.rs Cargo.toml kernel.ld
	cargo build --release

# Convert kernel ELF to flat binary
$(KERNEL_BIN): $(KERNEL)
	$(OBJCOPY) -O binary $< $@

kernel: $(KERNEL_BIN)

# Create bootable disk image
$(OS_IMAGE): $(STAGE1) $(STAGE2) $(KERNEL_BIN)
	@echo "Creating disk image..."
	# Start with stage 1 (512 bytes)
	cp $(STAGE1) $@
	# Append stage 2 (8KB = 16 sectors)
	cat $(STAGE2) >> $@
	# Pad to sector 17 (where kernel starts)
	@CURRENT_SIZE=$$(stat -c %s $@); \
	KERNEL_OFFSET=$$((17 * 512)); \
	if [ $$CURRENT_SIZE -lt $$KERNEL_OFFSET ]; then \
		dd if=/dev/zero bs=1 count=$$((KERNEL_OFFSET - CURRENT_SIZE)) >> $@ 2>/dev/null; \
	fi
	# Append kernel
	cat $(KERNEL_BIN) >> $@
	# Pad to 1.44MB floppy size (optional, good for compatibility)
	@CURRENT_SIZE=$$(stat -c %s $@); \
	FLOPPY_SIZE=$$((1474560)); \
	if [ $$CURRENT_SIZE -lt $$FLOPPY_SIZE ]; then \
		dd if=/dev/zero bs=1 count=$$((FLOPPY_SIZE - CURRENT_SIZE)) >> $@ 2>/dev/null; \
	fi
	@echo "Created $(OS_IMAGE) ($$(stat -c %s $@) bytes)"

image: $(OS_IMAGE)

# Build everything
build: image

# Run in QEMU
run: image
	$(QEMU) \
		-drive format=raw,file=$(OS_IMAGE) \
		-serial stdio \
		-display none \
		-no-reboot

# Run with QEMU debug output
debug: image
	$(QEMU) \
		-drive format=raw,file=$(OS_IMAGE) \
		-serial stdio \
		-display none \
		-no-reboot \
		-d int,cpu_reset

# Run with GDB server
gdb: image
	$(QEMU) \
		-drive format=raw,file=$(OS_IMAGE) \
		-serial stdio \
		-display none \
		-no-reboot \
		-s -S

# Clean build artifacts
clean:
	cargo clean
	rm -f $(STAGE1) $(STAGE2) $(KERNEL_BIN) $(OS_IMAGE)

# Install required tools
setup:
	rustup override set nightly
	rustup component add rust-src llvm-tools-preview
	@echo "Also ensure NASM is installed: sudo apt install nasm"

# Show help
help:
	@echo "Ralph OS Build System (No External Dependencies)"
	@echo ""
	@echo "Usage: make [target]"
	@echo ""
	@echo "Targets:"
	@echo "  all       - Build everything (default)"
	@echo "  bootloader- Build bootloader only"
	@echo "  kernel    - Build kernel only"
	@echo "  image     - Create bootable disk image"
	@echo "  run       - Build and run in QEMU"
	@echo "  debug     - Run with QEMU interrupt logging"
	@echo "  gdb       - Run with GDB server on port 1234"
	@echo "  clean     - Remove build artifacts"
	@echo "  setup     - Install required tools"
	@echo "  help      - Show this message"
