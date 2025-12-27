# Ralph OS

A simple x86_64 operating system written in Rust.

## Design Decisions

### No External Dependencies

Ralph OS is implemented entirely from scratch with **no external crates**. This includes:
- Custom bootloader (16-bit → 32-bit → 64-bit mode transitions)
- Custom serial driver (direct UART 16550 register access)
- Custom memory allocator
- All other OS components

Only Rust's `core` library is used. The `alloc` crate is enabled once we have a custom allocator.

### Architecture
- **Target**: x86_64 only
- **Single core**: No SMP support
- **Cooperative multitasking**: Tasks yield control voluntarily via `yield_now()`

### Memory Model
- **Flat address space**: No virtual memory, all tasks share the same linear address space
- **Linked list allocator**: Supports allocation and freeing (no paging)
- **Identity mapped**: Physical addresses = virtual addresses

### Application Model
- **Initially**: Apps are Rust modules compiled into the kernel
  - Any function can be spawned as a task
  - Multiple instances of the same app can run concurrently
  - All tasks share the same memory space
- **Later**: Dynamic loading of external executables

### I/O
- **Serial output only**: COM1 (0x3F8) via UART 16550
- **No graphics**: Run with `qemu -display none -serial stdio`

### Bootloader
- Custom two-stage bootloader written in assembly
- Stage 1: Boot sector (512 bytes), loads stage 2
- Stage 2: Sets up protected mode, long mode, page tables, loads kernel
- Identity-mapped page tables (physical = virtual addresses)

## Quick Start

```bash
# One-time setup (installs Rust nightly, bootimage tool)
make setup

# Build and run
make run
```

## Building

### Prerequisites

1. **Rust nightly toolchain** (with `rust-src` and `llvm-tools-preview`)
2. **NASM** assembler
3. **QEMU** for x86_64 emulation

Install everything with:
```bash
# Install system packages (Ubuntu/Debian)
sudo apt install qemu-system-x86 nasm

# Install Rust tools (run once)
make setup
```

### Make Targets

| Command       | Description                              |
|---------------|------------------------------------------|
| `make build`  | Build the kernel image                   |
| `make run`    | Build and run in QEMU                    |
| `make debug`  | Run with QEMU interrupt logging          |
| `make gdb`    | Run with GDB server (port 1234)          |
| `make clean`  | Remove build artifacts                   |
| `make setup`  | Install required Rust tools (run once)   |

### Manual Build

```bash
# Build bootable image
make image

# Run in QEMU
qemu-system-x86_64 \
  -drive format=raw,file=target/ralph_os.img \
  -serial stdio \
  -display none \
  -no-reboot
```

### Debugging with GDB

```bash
# Terminal 1: Start QEMU with GDB server
make gdb

# Terminal 2: Connect with GDB
gdb -ex "target remote :1234"
```

## Project Structure

```
ralph_os/
├── bootloader/
│   ├── stage1.asm          # Boot sector (512 bytes, 16-bit)
│   └── stage2.asm          # Mode transitions (16→32→64-bit)
├── src/
│   ├── main.rs             # Kernel entry point
│   ├── serial.rs           # Custom UART 16550 driver
│   └── allocator.rs        # Linked list heap allocator
├── Cargo.toml              # Project manifest (no dependencies!)
├── Makefile                # Build commands
├── kernel.ld               # Kernel linker script
├── x86_64-ralph_os.json    # Custom target specification
├── .cargo/config.toml      # Build configuration
├── README.md               # This file
├── ARCHITECTURE.md         # Technical architecture documentation
└── run.sh                  # Build and run script
```

See [ARCHITECTURE.md](ARCHITECTURE.md) for detailed technical documentation.

## Roadmap

1. ~~**Phase 1**: Hello World kernel with serial output~~ **DONE**
2. ~~**Phase 2**: Heap allocator (linked list)~~ **DONE**
3. **Phase 3** (next): Cooperative multitasking scheduler
4. **Phase 4**: Keyboard input and interactive shell
5. **Phase 5**: In-memory filesystem
