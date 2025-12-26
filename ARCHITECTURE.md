# Ralph OS Architecture

## Overview

Ralph OS is a minimal x86_64 operating system written in Rust with **no external dependencies**. Everything is implemented from scratch, including the bootloader, serial driver, and all OS components.

## Boot Process

```
BIOS → Stage 1 (boot sector) → Stage 2 → Kernel
       16-bit real mode         16→32→64-bit    64-bit long mode
```

### Stage 1 Bootloader (`bootloader/stage1.asm`)
- **Location**: Sector 0, loaded by BIOS at 0x7C00
- **Size**: 512 bytes (boot sector)
- **Mode**: 16-bit real mode
- **Responsibilities**:
  1. Set up segment registers (DS, ES, SS)
  2. Set up stack at 0x7C00 (grows down)
  3. Load stage 2 from disk (sectors 2-17, using BIOS INT 13h)
  4. Jump to stage 2 at 0x7E00

### Stage 2 Bootloader (`bootloader/stage2.asm`)
- **Location**: Sectors 2-17, loaded at 0x7E00
- **Size**: 8KB (16 sectors)
- **Responsibilities**:
  1. Enable A20 line (keyboard controller method)
  2. Load kernel from disk to temporary buffer at 0x10000
  3. Set up GDT (Global Descriptor Table)
  4. Switch to 32-bit protected mode
  5. Copy kernel from 0x10000 to 0x100000 (1MB)
  6. Set up identity-mapped page tables
  7. Enable PAE and long mode (via EFER MSR)
  8. Switch to 64-bit long mode
  9. Jump to kernel at 0x100000

### Kernel Entry (`src/main.rs`)
- **Location**: 0x100000 (1MB)
- **Entry point**: `_start` (naked function)
- **Responsibilities**:
  1. Set up stack pointer
  2. Call `kernel_main()`
  3. Initialize serial port
  4. Print welcome message
  5. Halt loop

## Memory Layout

```
┌─────────────────────────────────────┐ 0xFFFFFFFF (4GB)
│                                     │
│        (Unmapped)                   │
│                                     │
├─────────────────────────────────────┤ 0x00400000 (4MB)
│                                     │
│        Available RAM                │
│        (future: heap, tasks)        │
│                                     │
├─────────────────────────────────────┤ 0x00200000 (2MB)
│                                     │
│        Kernel                       │
│        .text, .rodata, .data, .bss  │
│                                     │
├─────────────────────────────────────┤ 0x00100000 (1MB)
│        BIOS ROM / Video Memory      │
├─────────────────────────────────────┤ 0x000A0000 (640KB)
│        Stack (grows down)           │
│        ← RSP starts at 0x90000      │
├─────────────────────────────────────┤ ~0x00090000
│                                     │
│        Free (temp kernel buffer)    │
│                                     │
├─────────────────────────────────────┤ 0x00010000 (64KB)
│        Stage 2 Bootloader           │
├─────────────────────────────────────┤ 0x00007E00
│        Stage 1 (Boot Sector)        │
├─────────────────────────────────────┤ 0x00007C00
│        Free                         │
├─────────────────────────────────────┤ 0x00005000
│        Page Tables (16KB)           │
│        PT    @ 0x4000               │
│        PD    @ 0x3000               │
│        PDPT  @ 0x2000               │
│        PML4  @ 0x1000               │
├─────────────────────────────────────┤ 0x00001000
│        Real Mode IVT + BDA          │
└─────────────────────────────────────┘ 0x00000000
```

## Disk Layout

```
┌──────────────────────────────────────┐
│ Sector 0     │ Stage 1 (512 bytes)   │
├──────────────┼───────────────────────┤
│ Sectors 1-16 │ Stage 2 (8KB)         │
├──────────────┼───────────────────────┤
│ Sectors 17+  │ Kernel binary         │
└──────────────┴───────────────────────┘
```

## Module Structure

```
ralph_os/
├── bootloader/
│   ├── stage1.asm      # Boot sector (16-bit)
│   └── stage2.asm      # Mode transitions (16→32→64-bit)
├── src/
│   ├── main.rs         # Kernel entry, panic handler
│   └── serial.rs       # UART 16550 driver
├── kernel.ld           # Linker script
└── x86_64-ralph_os.json # Custom target spec
```

### serial.rs - Custom Serial Driver

Implements UART 16550 from scratch:
- `inb()` / `outb()` - Port I/O via inline assembly
- `Serial::init()` - Configure 115200 baud, 8N1
- `Serial::write_byte()` - Blocking write with TX empty check
- `print!` / `println!` - Formatted output macros

## GDT (Global Descriptor Table)

```
Selector  │ Description
──────────┼─────────────────────────
0x00      │ Null descriptor
0x08      │ 32-bit code segment
0x10      │ 32-bit data segment
0x18      │ 64-bit code segment
0x20      │ 64-bit data segment
```

## Page Tables (Identity Mapped)

```
PML4[0] → PDPT[0] → PD[0] → 2MB huge page (0x000000-0x1FFFFF)
                    PD[1] → 2MB huge page (0x200000-0x3FFFFF)
```

First 4MB identity mapped using 2MB huge pages.

## Hardware Interaction

### I/O Ports

| Port Range  | Device              |
|-------------|---------------------|
| 0x3F8-0x3FD | COM1 Serial (UART)  |
| 0x60, 0x64  | PS/2 Keyboard ctrl  |
| 0x92        | Fast A20 gate       |

### CPU Features Used

| Feature          | Purpose                    |
|------------------|----------------------------|
| Long Mode        | 64-bit execution           |
| PAE              | Required for long mode     |
| 2MB Huge Pages   | Simplified page tables     |
| EFER MSR         | Long mode enable           |

## Build Process

```
make image
    │
    ├── nasm stage1.asm → stage1.bin (512 bytes)
    ├── nasm stage2.asm → stage2.bin (8KB, padded)
    ├── cargo build → kernel ELF
    ├── objcopy → kernel.bin (flat binary)
    │
    └── Combine: stage1 + stage2 + kernel → ralph_os.img
```

## Future Architecture (Planned)

### Phase 2: Memory Allocator
- Parse available memory regions
- Linked list allocator implementing `GlobalAlloc`
- Enable `alloc` crate for `Vec`, `Box`, `String`

### Phase 3: Cooperative Multitasking
- Task struct with saved register state
- Round-robin scheduler
- `yield_now()` for voluntary yielding
- All tasks share flat address space

### Phase 4: Shell
- PS/2 keyboard driver
- IDT setup for interrupts
- Line-buffered input
- Command parsing

### Phase 5: Filesystem
- In-memory filesystem
- Directory tree structure
- Basic file operations
