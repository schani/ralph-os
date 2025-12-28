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

| Command           | Description                              |
|-------------------|------------------------------------------|
| `make build`      | Build the kernel image                   |
| `make run`        | Build and run in QEMU                    |
| `make run-net`    | Run with NE2000 network (user mode)      |
| `make run-net-tap`| Run with TAP networking (ping support)   |
| `make debug`      | Run with QEMU interrupt logging          |
| `make gdb`        | Run with GDB server (port 1234)          |
| `make clean`      | Remove build artifacts                   |
| `make setup`      | Install required Rust tools (run once)   |

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

## Networking

Ralph OS includes a TCP/IP network stack with:
- NE2000 NIC driver
- Ethernet, ARP, IPv4, ICMP, TCP protocols
- Non-blocking socket API for user programs

### Running with Network

```bash
# Run with QEMU user-mode networking (no ping from host)
make run-net
```

Network configuration (QEMU user networking defaults):
- IP: `10.0.2.15`
- Netmask: `255.255.255.0`
- Gateway: `10.0.2.2`

### Pinging Ralph OS

QEMU's user-mode networking doesn't forward inbound ICMP. To ping the VM, use TAP networking:

```bash
# 1. Create TAP interface (one-time setup, requires root)
sudo ip tuntap add dev tap0 mode tap user $USER
sudo ip addr add 10.0.2.1/24 dev tap0
sudo ip link set tap0 up

# 2. Run Ralph OS with TAP networking
make run-net-tap

# 3. In another terminal, ping the VM
ping 10.0.2.15
```

You should see output like:
```
[icmp] Echo request from 10.0.2.1 seq=1
[icmp] Sent echo reply to 10.0.2.1 seq=1
```

To clean up the TAP interface:
```bash
sudo ip link set tap0 down
sudo ip tuntap del dev tap0 mode tap
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
│   ├── allocator.rs        # Linked list heap allocator
│   ├── scheduler.rs        # Cooperative task scheduler
│   ├── api.rs              # Kernel API for programs
│   ├── basic/              # BASIC interpreter
│   └── net/                # TCP/IP network stack
│       ├── ne2000.rs       # NE2000 NIC driver
│       ├── tcp.rs          # TCP state machine
│       └── ...             # Ethernet, ARP, IPv4, ICMP
├── programs/               # User programs (compiled to ELF)
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
3. ~~**Phase 3**: Cooperative multitasking scheduler~~ **DONE**
4. ~~**Phase 4**: BASIC interpreter~~ **DONE**
5. ~~**Phase 5**: ELF program loading~~ **DONE**
6. ~~**Phase 6**: TCP/IP networking~~ **DONE**
7. **Phase 7** (next): Keyboard input and interactive shell
8. **Phase 8**: In-memory filesystem
