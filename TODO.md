# Ralph OS - Technical Debt & Improvements

Issues identified during code review, with proposed solutions where applicable.

---

## High Priority - COMPLETED

### 1. ~~Spinlock Should Assert, Not Spin~~ ✓ DONE
**Location:** `src/allocator.rs:255-270`

**Fixed:** Spinlock now panics on contention instead of spinning, enforcing the single-threaded invariant.

---

### 2. ~~Unsafe SCHEDULER Access Pattern~~ ✓ DONE
**Location:** `src/scheduler.rs`

**Fixed:** Implemented `SchedulerCell` wrapper that:
- Enforces single initialization (panics if init called twice)
- Enforces initialization before use (panics if used before init)
- Centralizes unsafe into a single, documented location
- Uses closure-based `with()` API to prevent holding references across yields

---

### 3. ~~Timer Polling Can Miss Ticks~~ ✓ DONE
**Location:** `src/timer.rs`

**Fixed:** Implemented count accumulation approach:
- Tracks raw PIT counts instead of just wrap detection
- Accumulates sub-tick counts for better accuracy
- Converts to ticks only when a full tick's worth of counts accumulate
- Still limited by polling frequency (can't detect >1 wrap between polls)

---

### 4. ~~Busy-Wait Scheduling Burns CPU~~ ✓ DONE
**Location:** `src/scheduler.rs:207-215`

**Fixed:** Implemented full interrupt support:
- Created `src/idt.rs` with Interrupt Descriptor Table (256 entries)
- Created `src/pic.rs` with 8259 PIC driver (remaps IRQ0-15 to vectors 32-47)
- Created `src/interrupts.rs` with naked ISR stubs and Rust handlers
- Timer interrupt (IRQ0/vector 32) now drives tick counter
- Scheduler uses HLT instruction to sleep between interrupts
- CPU enters low-power state until next timer tick (100 Hz)

---

### 5. ~~Finished Tasks Never Cleaned Up~~ ✓ DONE
**Location:** `src/scheduler.rs`

**Fixed:** Added `reap_finished_tasks()` method that:
- Removes finished tasks from the Vec
- Correctly adjusts current task index
- Called automatically in `schedule()` when finished tasks exist
- Frees task stacks back to heap

---

## Medium Priority

### 6. Serial `_print` Creates Unnecessary Instance
**Location:** `src/serial.rs:151-156`

**Problem:**
```rust
pub fn _print(args: fmt::Arguments) {
    let mut serial = Serial::new(COM1);  // New instance every call
    serial.write_fmt(args).unwrap();
}
```
There's already a `static SERIAL` at line 142, but it's not used here.

**Solution:** Use the existing static (requires mutable access):
```rust
pub fn _print(args: fmt::Arguments) {
    use core::fmt::Write;
    // SERIAL is already initialized, just need Write impl on &Serial
    SERIAL.write_fmt(args).unwrap();
}
```

Note: `Serial::write_str(&self, ...)` already takes `&self`, so this should work. The current `impl Write for Serial` takes `&mut self` unnecessarily.

---

### 7. Duplicated Port I/O Functions
**Location:** `src/serial.rs:22-43`, `src/timer.rs:21-43`

**Problem:** `inb()` and `outb()` are copy-pasted between modules.

**Solution:** Create `src/io.rs`:
```rust
//! Low-level port I/O operations

#[inline]
pub unsafe fn inb(port: u16) -> u8 {
    let value: u8;
    core::arch::asm!(
        "in al, dx",
        out("al") value,
        in("dx") port,
        options(nomem, nostack, preserves_flags)
    );
    value
}

#[inline]
pub unsafe fn outb(port: u16, value: u8) {
    core::arch::asm!(
        "out dx, al",
        in("dx") port,
        in("al") value,
        options(nomem, nostack, preserves_flags)
    );
}
```

Then `use crate::io::{inb, outb};` in serial.rs and timer.rs.

---

### 8. Unnecessary Box Around Scheduler
**Location:** `src/scheduler.rs:13, 168`

**Problem:**
```rust
static mut SCHEDULER: Option<Box<Scheduler>> = None;
// ...
SCHEDULER = Some(Box::new(Scheduler::new()));
```

The scheduler is heap-allocated but never moved after initialization. Adds unnecessary indirection.

**Solution:** Store directly:
```rust
static mut SCHEDULER: Option<Scheduler> = None;
// ...
SCHEDULER = Some(Scheduler::new());
```

Or with the `SchedulerCell` pattern from issue #2, avoid the Option entirely.

---

### 9. BASIC Interpreter Clones Statements Every Step
**Location:** `src/basic/interpreter.rs:146-152`

**Problem:**
```rust
let stmt = match self.program.get(&line_num) {
    Some(s) => s.clone(),  // Clone entire AST node every step
    None => { ... }
};
```

**Solution:** Use reference and restructure to avoid borrow conflicts:
```rust
// Store line_num, look up statement only when needed
// Or use indices into a statement arena
```

This is a minor optimization but matters for tight loops in BASIC programs.

---

## Low Priority / Future Work

### 10. No Stack Overflow Protection
**Location:** `src/task.rs:101`

**Problem:** Task stacks are 16KB heap allocations with no guard pages. Overflow corrupts adjacent memory silently.

**Solution (if virtual memory added):** Map a guard page below each stack that faults on access.

**Solution (current design):** Add stack canaries:
```rust
const STACK_CANARY: u64 = 0xDEAD_BEEF_CAFE_BABE;

impl Task {
    pub fn new(...) -> Self {
        let mut stack = vec![0u8; STACK_SIZE];
        // Write canary at bottom of stack
        let canary_ptr = stack.as_mut_ptr() as *mut u64;
        unsafe { *canary_ptr = STACK_CANARY; }
        // ...
    }

    pub fn check_stack(&self) -> bool {
        let canary_ptr = self.stack.as_ptr() as *const u64;
        unsafe { *canary_ptr == STACK_CANARY }
    }
}
```

Check canaries periodically in scheduler.

---

### 11. Context Switch Doesn't Save SIMD State
**Location:** `src/context_switch.rs`

**Problem:** Only callee-saved GPRs are preserved. SSE/AVX registers (XMM0-15, YMM0-15) are clobbered on context switch. The bootloader enables SSE at `stage2.asm:407-418`.

**Solution:** Either:
1. Save/restore XMM registers in `switch_context` (expensive, ~512 bytes per task)
2. Use `FXSAVE`/`FXRSTOR` instructions
3. Disable SSE entirely via target spec (if Rust core doesn't need it)

For now, document that SIMD is not task-safe.

---

### 12. Bootloader Hardcoded Sector Limit
**Location:** `bootloader/stage2.asm:13`

**Problem:**
```asm
KERNEL_SECTORS equ 200  ; ~100KB max kernel size
```

Kernel larger than 100KB silently truncates.

**Solution:** Calculate required sectors from actual kernel size, or at minimum fail loudly if kernel exceeds limit during build.

---

## Architectural Notes (Not Bugs)

These are intentional design decisions, documented here for clarity:

- **No virtual memory isolation** - All tasks share flat address space. By design for simplicity.
- **No preemptive scheduling** - Cooperative only. Tasks must yield voluntarily.
- **Timer interrupt only** - Only IRQ0/timer is currently handled. Keyboard etc. not yet implemented.
- **Single-core only** - No SMP support.
- **Serial I/O only** - No keyboard/display drivers.
