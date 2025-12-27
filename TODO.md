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

## Medium Priority - COMPLETED

### 6. ~~Serial `_print` Creates Unnecessary Instance~~ ✓ DONE
**Location:** `src/serial.rs`

**Fixed:** `_print` now uses the static `SERIAL` instance instead of creating a new one each call. Implemented `fmt::Write` for `&Serial` to enable shared reference usage.

---

### 7. ~~Duplicated Port I/O Functions~~ ✓ DONE
**Location:** `src/io.rs` (new)

**Fixed:** Created `src/io.rs` with shared `inb`, `outb`, and `io_wait` functions. Updated `serial.rs`, `timer.rs`, and `pic.rs` to use `crate::io`.

---

### 8. ~~Unnecessary Box Around Scheduler~~ ✓ DONE
**Location:** `src/scheduler.rs`

**Fixed:** Resolved when implementing issue #2. The `SchedulerCell` pattern stores `Scheduler` directly in `UnsafeCell<Option<Scheduler>>` without heap allocation.

---

### 9. ~~BASIC Interpreter Clones Statements Every Step~~ ✓ DONE
**Location:** `src/basic/interpreter.rs`

**Fixed:** Refactored `execute_statement`, `eval_expr`, and `eval_binary_op` from methods to free functions with split borrows. Now borrows the statement directly from the program BTreeMap without cloning.

---

## Low Priority - COMPLETED

### 11. ~~Context Switch Doesn't Save SIMD State~~ ✓ DONE
**Location:** `src/context_switch.rs`

**Fixed:** Added comprehensive documentation warning that SIMD/SSE state is not preserved across context switches. Documents current mitigations (target spec disables advanced SSE) and future solutions (FXSAVE/FXRSTOR or soft-float).

---

### 12. ~~Bootloader Hardcoded Sector Limit~~ ✓ DONE
**Location:** `Makefile`, `bootloader/stage2.asm:13`

**Fixed:** Added kernel size check in Makefile that fails the build if kernel exceeds 102,400 bytes (200 sectors × 512 bytes). Build now reports kernel size and limit.

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

## Architectural Notes (Not Bugs)

These are intentional design decisions, documented here for clarity:

- **No virtual memory isolation** - All tasks share flat address space. By design for simplicity.
- **No preemptive scheduling** - Cooperative only. Tasks must yield voluntarily.
- **Timer interrupt only** - Only IRQ0/timer is currently handled. Keyboard etc. not yet implemented.
- **Single-core only** - No SMP support.
- **Serial I/O only** - No keyboard/display drivers.
