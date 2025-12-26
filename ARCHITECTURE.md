# Ralph OS Architecture

## Overview

Ralph OS is a minimal x86_64 operating system written in Rust. It runs on a single core with cooperative multitasking, where all tasks share a flat memory address space.

## Boot Process

```
BIOS → bootloader crate (stage 1 & 2) → kernel_main()
```

1. **BIOS**: Loads first sector from disk
2. **Bootloader Stage 1**: Sets up basic environment, loads stage 2
3. **Bootloader Stage 2**:
   - Switches to long mode (64-bit)
   - Sets up initial page tables (identity mapped)
   - Loads kernel ELF into memory
   - Jumps to kernel entry point
4. **kernel_main()**: Receives `BootInfo` struct with memory map

## Memory Layout

```
┌─────────────────────────────────────┐ High memory
│                                     │
│  Available RAM (from BootInfo)      │
│  - Used for heap allocation         │
│                                     │
├─────────────────────────────────────┤
│  Kernel code & data                 │
│  - .text, .rodata, .data, .bss      │
├─────────────────────────────────────┤
│  Bootloader reserved                │
├─────────────────────────────────────┤
│  BIOS/Hardware reserved             │
│  - VGA buffer at 0xB8000            │
│  - ROM, ACPI tables, etc.           │
└─────────────────────────────────────┘ 0x0000
```

**Key design choice**: No virtual memory. Physical addresses = virtual addresses (identity mapped). All tasks share the same address space.

## Module Structure

```
src/
├── main.rs          # Entry point, panic handler
└── serial.rs        # Serial port driver
```

### main.rs

- `#![no_std]` - No standard library
- `#![no_main]` - No Rust runtime entry point
- `entry_point!(kernel_main)` - Bootloader macro sets up entry
- `kernel_main(&'static BootInfo)` - Kernel starts here
- `#[panic_handler]` - Custom panic handler prints to serial

### serial.rs

Serial port driver for COM1 (I/O port 0x3F8).

- `SERIAL1: Mutex<SerialPort>` - Global serial port instance (lazy_static)
- `serial_print!` / `serial_println!` - Macros for formatted output
- Uses `uart_16550` crate for hardware abstraction

## Hardware Interaction

### I/O Ports Used

| Port    | Purpose              |
|---------|----------------------|
| 0x3F8   | COM1 serial (data)   |
| 0x3F9   | COM1 interrupt enable|
| 0x3FA   | COM1 FIFO control    |
| 0x3FB   | COM1 line control    |
| 0x3FC   | COM1 modem control   |
| 0x3FD   | COM1 line status     |

### CPU Instructions

- `hlt` - Halt CPU until next interrupt (power saving in idle loop)
- `in` / `out` - Port I/O for serial communication

## Build System

```
cargo bootimage --release
      │
      ├── Builds kernel as ELF binary
      │   └── Target: x86_64-ralph_os.json (custom)
      │
      └── Builds bootloader
          └── Combines into bootable disk image
```

**Custom target** (`x86_64-ralph_os.json`):
- Disables red zone (required for interrupt handlers)
- Disables MMX/SSE3+ (keeps SSE/SSE2 for ABI compatibility)
- Uses `panic = "abort"` (no unwinding)
- Links with `rust-lld`

## Dependencies

| Crate              | Purpose                          |
|--------------------|----------------------------------|
| `bootloader`       | BIOS bootloader, sets up long mode |
| `uart_16550`       | Serial port hardware driver      |
| `spin`             | Spinlock for interrupt-safe sync |
| `x86_64`           | CPU primitives (port I/O, hlt)   |
| `lazy_static`      | Static initialization with locks |

## Future Architecture (Planned)

### Phase 2: Memory Allocator
- Parse memory map from `BootInfo`
- Linked list allocator implementing `GlobalAlloc`
- Enables `Vec`, `Box`, `String`, etc.

### Phase 3: Cooperative Multitasking
```
┌──────────────────────────────────────────┐
│              Scheduler                    │
│  ┌──────┐  ┌──────┐  ┌──────┐           │
│  │Task 1│  │Task 2│  │Task 3│  ...      │
│  └──────┘  └──────┘  └──────┘           │
│      │         │         │               │
│      └─────────┴─────────┘               │
│           yield_now()                    │
└──────────────────────────────────────────┘
```

- Tasks are Rust functions/closures
- Voluntary yielding via `yield_now()`
- Round-robin scheduling
- Shared address space (no context switch overhead for memory)

### Phase 4: Shell
- PS/2 keyboard driver via interrupts
- IDT (Interrupt Descriptor Table) setup
- Line-buffered input
- Command parsing and dispatch

### Phase 5: Filesystem
- In-memory filesystem
- Simple directory tree structure
- Basic file operations
