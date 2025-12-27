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
├─────────────────────────────────────┤ 0x01000000 (16MB)
│                                     │
│        Program Region (12MB)        │
│        - Task stacks (16KB each)    │
│        - Loaded ELF programs        │
│        - User heap allocations      │
│        Capacity: ~768 tasks         │
│                                     │
├─────────────────────────────────────┤ 0x00400000 (4MB)
│                                     │
│        Kernel Heap (2MB)            │
│        - Kernel data structures     │
│        - Allocation tracking        │
│                                     │
├─────────────────────────────────────┤ 0x00200000 (2MB)
│                                     │
│        Kernel                       │
│        .text, .rodata, .data, .bss  │
│                                     │
├─────────────────────────────────────┤ 0x00100000 (1MB)
│        BIOS ROM / Video Memory      │
├─────────────────────────────────────┤ 0x000A0000 (640KB)
│        Boot Stack (grows down)      │
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
│   ├── stage1.asm        # Boot sector (16-bit)
│   └── stage2.asm        # Mode transitions (16→32→64-bit)
├── src/
│   ├── main.rs           # Kernel entry, panic handler
│   ├── io.rs             # Port I/O primitives (inb, outb)
│   ├── serial.rs         # UART 16550 driver
│   ├── allocator.rs      # Linked list heap allocator
│   ├── program_alloc.rs  # Bump allocator for program region
│   ├── idt.rs            # Interrupt Descriptor Table
│   ├── pic.rs            # 8259 PIC driver
│   ├── interrupts.rs     # ISR stubs and handlers
│   ├── timer.rs          # PIT timer driver
│   ├── scheduler.rs      # Cooperative scheduler
│   ├── task.rs           # Task struct and context
│   ├── context_switch.rs # Context switch assembly
│   ├── api.rs            # Kernel API for loaded programs
│   ├── executable.rs     # ELF loader and memory tracking
│   ├── elf.rs            # ELF format parser
│   └── basic/            # BASIC interpreter
│       ├── mod.rs        # Module and tasks
│       ├── lexer.rs      # Tokenizer
│       ├── parser.rs     # AST builder
│       └── interpreter.rs# BASIC runtime
├── programs/             # User programs (compiled to ELF)
├── kernel.ld             # Linker script
└── x86_64-ralph_os.json  # Custom target spec
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

## Interrupt Handling

### Interrupt Descriptor Table (IDT)

The IDT contains 256 entries for handling CPU exceptions and hardware interrupts.

```
Vector  │ Source      │ Handler
────────┼─────────────┼───────────────
0-31    │ CPU         │ (not yet implemented)
32      │ IRQ0/Timer  │ timer_handler
33-38   │ IRQ1-6      │ (not yet implemented)
39      │ IRQ7        │ spurious_handler
40-46   │ IRQ8-14     │ (not yet implemented)
47      │ IRQ15       │ spurious_handler
```

### Programmable Interrupt Controller (PIC)

The 8259 PICs are remapped to avoid conflicts with CPU exceptions:

```
PIC1 (Master): IRQ 0-7  → Vectors 32-39
PIC2 (Slave):  IRQ 8-15 → Vectors 40-47
```

### Timer Interrupt

The PIT (8253/8254) is configured for 100 Hz:
- Channel 0, Mode 2 (rate generator)
- Divisor: 11932 (1,193,182 Hz / 100 Hz)
- Interrupt triggers `timer_handler()` which increments tick count

### HLT-Based Sleep

The scheduler uses HLT for efficient sleeping:
```rust
// In scheduler when waiting for sleeping tasks:
if self.has_sleeping_tasks() {
    unsafe { core::arch::asm!("hlt"); }  // Wait for interrupt
    self.wake_sleeping_tasks();
}
```

This puts the CPU in a low-power state until the next timer interrupt.

## Hardware Interaction

### I/O Ports

| Port Range  | Device              |
|-------------|---------------------|
| 0x3F8-0x3FD | COM1 Serial (UART)  |
| 0x40-0x43   | PIT Timer           |
| 0x20-0x21   | PIC1 (Master)       |
| 0xA0-0xA1   | PIC2 (Slave)        |
| 0x60, 0x64  | PS/2 Keyboard ctrl  |
| 0x92        | Fast A20 gate       |

### CPU Features Used

| Feature          | Purpose                    |
|------------------|----------------------------|
| Long Mode        | 64-bit execution           |
| PAE              | Required for long mode     |
| 2MB Huge Pages   | Simplified page tables     |
| EFER MSR         | Long mode enable           |
| IDT/LIDT         | Interrupt handling         |
| HLT              | Low-power wait             |

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

## Memory Allocators

### Kernel Heap (`src/allocator.rs`)

A first-fit linked list allocator for kernel data structures:

```
Region: 0x200000 - 0x400000 (2MB)

Free Block Structure:
┌──────────────────────────────────────┐
│ size: usize (8 bytes)                │
│ next: Option<NonNull<FreeBlock>>     │
├──────────────────────────────────────┤
│ ... usable memory ...                │
└──────────────────────────────────────┘
```

- First-fit allocation with block splitting
- Deallocation with adjacent block merging
- Spinlock wrapper for safety

### Program Region (`src/program_alloc.rs`)

A simple bump allocator for program memory:

```
Region: 0x400000 - 0x1000000 (12MB)

Used for:
- Task stacks (16KB each, ~768 max tasks)
- Loaded ELF programs
- User heap allocations (via API)
```

All allocations are tracked per-task and auto-freed on exit.

## Cooperative Multitasking

### Task Structure (`src/task.rs`)

```rust
pub struct Task {
    id: TaskId,
    name: &'static str,
    state: TaskState,        // Ready, Running, Sleeping, Finished
    context: Context,        // Saved CPU registers
    stack_base: usize,       // Stack in program region
    stack_size: usize,
    wake_at: u64,            // Timer tick to wake (if sleeping)
}
```

### Scheduler (`src/scheduler.rs`)

Round-robin cooperative scheduler:
- Tasks yield voluntarily via `yield_now()` or `sleep_ms()`
- Timer-driven wake for sleeping tasks (100 Hz)
- HLT-based idle when all tasks sleeping
- Auto-cleanup of finished tasks

### Context Switch (`src/context_switch.rs`)

Saves/restores callee-saved registers (R12-R15, RBX, RBP, RSP).

## Executable Loading

### ELF Loader (`src/elf.rs`, `src/executable.rs`)

Loads position-independent ELF64 executables:
1. Parse ELF header and program headers
2. Allocate memory in program region
3. Load PT_LOAD segments
4. Return entry point address

### Kernel API (`src/api.rs`)

Programs receive a pointer to the KernelApi struct:

```rust
pub struct KernelApi {
    version: u32,                              // API version (currently 2)
    print: extern "C" fn(*const u8, usize),    // Print string
    yield_now: extern "C" fn(),                // Yield to scheduler
    sleep_ms: extern "C" fn(u64),              // Sleep milliseconds
    exit: extern "C" fn() -> !,                // Exit program
    alloc: extern "C" fn(usize) -> *mut u8,    // Allocate (4KB aligned)
    free: extern "C" fn(*mut u8),              // Free (kernel tracks size)
}
```

### Per-Task Memory Tracking (`src/executable.rs`)

```rust
struct TaskAllocations {
    stack: (usize, usize),              // Always present
    program: Option<(usize, usize)>,    // Only for ELF programs
    heap_blocks: Vec<(usize, usize)>,   // User allocations via alloc()
}
```

Safety features:
- Ownership verification on free()
- Auto-cleanup when task exits
- 4KB allocation granularity

## BASIC Interpreter

Interactive BASIC REPL with line-numbered programs:

### Supported Commands
- `PRINT expr` - Output values
- `LET var = expr` - Variable assignment
- `INPUT var` - Read user input
- `FOR/NEXT` - Counting loops
- `IF/THEN/ELSE` - Conditionals
- `GOTO line` - Jump to line
- `GOSUB/RETURN` - Subroutines
- `SPAWN "name"` - Run program in background
- `RUN/LIST/NEW` - Program control

### Tasks
- `memstats_task` - Periodic heap usage display
- `repl_task` - Interactive interpreter
