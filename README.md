# Ralph OS

A simple x86_64 operating system written in Rust.

## Design Decisions

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
- Uses the `bootloader` crate (v0.9)
- Handles BIOS boot, setting up long mode, loading the kernel

## Quick Start

```bash
# One-time setup (installs Rust nightly, bootimage tool)
make setup

# Build and run
make run
```

## Building

### Prerequisites

1. **Rust nightly toolchain**
2. **QEMU** for x86_64 emulation
3. **bootimage** cargo subcommand

Install everything with:
```bash
# Install QEMU (Ubuntu/Debian)
sudo apt install qemu-system-x86

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
cargo bootimage --release

# Run in QEMU
qemu-system-x86_64 \
  -drive format=raw,file=target/x86_64-ralph_os/release/bootimage-ralph_os.bin \
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
├── Cargo.toml              # Project manifest
├── Makefile                # Build commands
├── x86_64-ralph_os.json    # Custom target specification
├── .cargo/config.toml      # Build configuration
├── run.sh                  # Build and run script
└── src/
    ├── main.rs             # Kernel entry point
    └── serial.rs           # Serial port driver
```

## Roadmap

1. ~~**Phase 1**: Hello World kernel with serial output~~ **DONE**
2. **Phase 2** (next): Heap allocator (linked list)
3. **Phase 3**: Cooperative multitasking scheduler
4. **Phase 4**: Keyboard input and interactive shell
5. **Phase 5**: In-memory filesystem
