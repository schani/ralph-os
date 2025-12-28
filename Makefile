.PHONY: all build run run-net debug clean setup help bootloader kernel image programs

# Output files
BUILD_DIR       = target
BOOT_DIR        = bootloader
STAGE1          = $(BUILD_DIR)/stage1.bin
STAGE2          = $(BUILD_DIR)/stage2.bin
KERNEL          = $(BUILD_DIR)/x86_64-ralph_os/release/ralph_os
KERNEL_BIN      = $(BUILD_DIR)/kernel.bin
OS_IMAGE        = $(BUILD_DIR)/ralph_os.img
EXEC_TABLE      = $(BUILD_DIR)/exec_table.bin

# Programs
PROGRAMS_DIR    = programs
PROGRAMS        = hello
PROGRAM_ELFS    = $(patsubst %,$(BUILD_DIR)/programs/%.elf,$(PROGRAMS))

# Tools
NASM            = nasm
OBJCOPY         = $(shell find ~/.rustup -name 'llvm-objcopy' 2>/dev/null | head -1)
QEMU            = qemu-system-x86_64
PYTHON          = python3

# Kernel size limit (must match KERNEL_SECTORS in stage2.asm)
# 400 sectors * 512 bytes = 204800 bytes (includes kernel + exec table + programs)
MAX_KERNEL_SIZE = 204800

# Default target
all: image

# Create build directories
$(BUILD_DIR):
	mkdir -p $(BUILD_DIR)

$(BUILD_DIR)/programs:
	mkdir -p $(BUILD_DIR)/programs

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
$(KERNEL): src/*.rs src/basic/*.rs Cargo.toml kernel.ld
	cargo build --release

# Convert kernel ELF to flat binary
$(KERNEL_BIN): $(KERNEL)
	$(OBJCOPY) -O binary $< $@
	@echo "Kernel size: $$(stat -c %s $@) bytes"

kernel: $(KERNEL_BIN)

# Build a program
$(BUILD_DIR)/programs/%.elf: $(PROGRAMS_DIR)/%/src/main.rs $(PROGRAMS_DIR)/%/Cargo.toml | $(BUILD_DIR)/programs
	@echo "Building program: $*"
	cd $(PROGRAMS_DIR)/$* && cargo build --release
	cp $(PROGRAMS_DIR)/$*/target/x86_64-ralph_program/release/$* $@
	@echo "Program $* size: $$(stat -c %s $@) bytes"

# Build all programs
programs: $(PROGRAM_ELFS)

# Create executable table (header + concatenated ELFs)
$(EXEC_TABLE): $(PROGRAM_ELFS)
	@echo "Creating executable table..."
	$(PYTHON) scripts/make_exec_table.py $@ $(PROGRAM_ELFS)

# Create bootable disk image
$(OS_IMAGE): $(STAGE1) $(STAGE2) $(KERNEL_BIN) $(EXEC_TABLE)
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
	# Append executable table (header + programs)
	cat $(EXEC_TABLE) >> $@
	# Check total size
	@TOTAL_SIZE=$$(stat -c %s $@); \
	KERNEL_START=$$((17 * 512)); \
	CONTENT_SIZE=$$((TOTAL_SIZE - KERNEL_START)); \
	if [ $$CONTENT_SIZE -gt $(MAX_KERNEL_SIZE) ]; then \
		echo "ERROR: Kernel + programs too large! $$CONTENT_SIZE bytes > $(MAX_KERNEL_SIZE) bytes"; \
		echo "Increase KERNEL_SECTORS in stage2.asm or reduce content size."; \
		rm -f $@; \
		exit 1; \
	else \
		echo "Kernel + programs size: $$CONTENT_SIZE bytes (limit: $(MAX_KERNEL_SIZE))"; \
	fi
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

# Run with networking (NE2000 NIC)
run-net: image
	$(QEMU) \
		-drive format=raw,file=$(OS_IMAGE) \
		-serial stdio \
		-display none \
		-no-reboot \
		-netdev user,id=net0 \
		-device ne2k_isa,netdev=net0,irq=10,iobase=0x300

# Run with TAP networking for ping testing (requires sudo)
# First: sudo ip tuntap add dev tap0 mode tap user $$USER
#        sudo ip addr add 10.0.2.1/24 dev tap0
#        sudo ip link set tap0 up
run-net-tap: image
	sudo $(QEMU) \
		-drive format=raw,file=$(OS_IMAGE) \
		-serial stdio \
		-display none \
		-no-reboot \
		-netdev tap,id=net0,ifname=tap0,script=no,downscript=no \
		-device ne2k_isa,netdev=net0,irq=10,iobase=0x300

# VGA flag offset: stage2 starts at byte 512, vga_flag is at offset 876 within stage2
VGA_FLAG_OFFSET = 1388

# Run with VGA memory visualization (patches vga_flag in stage2)
run-vga: image
	@echo "Enabling VGA visualization..."
	@printf '\x01' | dd of=$(OS_IMAGE) bs=1 seek=$(VGA_FLAG_OFFSET) conv=notrunc 2>/dev/null
	$(QEMU) \
		-drive format=raw,file=$(OS_IMAGE) \
		-serial stdio \
		-no-reboot

# Test VGA visualization with automated screenshot
test-vga: image
	@printf '\x01' | dd of=$(OS_IMAGE) bs=1 seek=$(VGA_FLAG_OFFSET) conv=notrunc 2>/dev/null
	@rm -f /tmp/qemu-monitor.sock /tmp/vga-test.ppm /tmp/serial.txt
	@$(QEMU) \
		-drive format=raw,file=$(OS_IMAGE) \
		-serial file:/tmp/serial.txt \
		-display none \
		-device VGA \
		-monitor unix:/tmp/qemu-monitor.sock,server,nowait \
		-no-reboot &
	@sleep 3
	@echo "screendump /tmp/vga-test.ppm" | nc -U /tmp/qemu-monitor.sock 2>/dev/null || true
	@sleep 1
	@pkill -f "qemu.*ralph_os.img" 2>/dev/null || true
	@echo ""
	@echo "=== Serial Output ==="
	@cat /tmp/serial.txt 2>/dev/null || echo "(no output)"
	@echo ""
	@echo "=== VGA Screenshot ==="
	@ls -la /tmp/vga-test.ppm 2>/dev/null && head -2 /tmp/vga-test.ppm || echo "ERROR: Screenshot not created"

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
	rm -f $(STAGE1) $(STAGE2) $(KERNEL_BIN) $(OS_IMAGE) $(EXEC_TABLE)
	rm -rf $(BUILD_DIR)/programs
	for prog in $(PROGRAMS); do \
		rm -rf $(PROGRAMS_DIR)/$$prog/target; \
	done

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
	@echo "  all         - Build everything (default)"
	@echo "  bootloader  - Build bootloader only"
	@echo "  kernel      - Build kernel only"
	@echo "  programs    - Build all programs"
	@echo "  image       - Create bootable disk image"
	@echo "  run         - Build and run in QEMU"
	@echo "  run-net     - Run with NE2000 network (user mode)"
	@echo "  run-net-tap - Run with TAP networking (requires sudo, enables ping)"
	@echo "  run-vga     - Run with VGA memory visualization"
	@echo "  test-vga    - Test VGA visualization with automated screenshot"
	@echo "  debug       - Run with QEMU interrupt logging"
	@echo "  gdb         - Run with GDB server on port 1234"
	@echo "  clean       - Remove build artifacts"
	@echo "  setup       - Install required tools"
	@echo "  help        - Show this message"
