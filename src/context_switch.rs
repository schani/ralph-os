//! Context switching assembly for cooperative multitasking
//!
//! Switches execution between tasks by saving/restoring callee-saved registers.
//!
//! # SIMD State Warning
//!
//! This context switch implementation only saves/restores general-purpose
//! callee-saved registers (r15, r14, r13, r12, rbx, rbp, rsp). It does NOT
//! save SSE/AVX state (XMM0-15, YMM0-15).
//!
//! **Implications:**
//! - SIMD registers are NOT preserved across context switches
//! - Tasks using SIMD/floating-point may see corrupted values after yielding
//! - The Rust compiler may use SSE for memcpy/floating-point operations
//!
//! **Current mitigations:**
//! - Target spec disables advanced SSE extensions (-sse3, -sse4, -avx, etc.)
//! - Most BASIC interpreter code uses only integer operations
//!
//! **Future solutions (if SIMD support needed):**
//! - Use FXSAVE/FXRSTOR to save 512 bytes of FPU/SSE state per task
//! - Or add `+soft-float` to target spec to disable hardware FP entirely

use crate::task::Context;

/// Switch from the current task's context to the next task's context
///
/// This function saves all callee-saved registers to `current`, then
/// restores them from `next` and returns to the new task.
///
/// # Arguments
/// * `current` - Pointer to save the current context into
/// * `next` - Pointer to restore the next context from
///
/// # Safety
/// - Both pointers must be valid and properly aligned
/// - The `next` context must have been previously set up correctly
#[unsafe(naked)]
pub unsafe extern "C" fn switch_context(
    _current: *mut Context,
    _next: *const Context,
) {
    // Arguments: rdi = current, rsi = next
    //
    // Context struct layout (must match task.rs):
    //   offset 0:  r15
    //   offset 8:  r14
    //   offset 16: r13
    //   offset 24: r12
    //   offset 32: rbx
    //   offset 40: rbp
    //   offset 48: rsp
    core::arch::naked_asm!(
        // Save current context
        // Save callee-saved registers to current context struct
        "mov [rdi + 0], r15",
        "mov [rdi + 8], r14",
        "mov [rdi + 16], r13",
        "mov [rdi + 24], r12",
        "mov [rdi + 32], rbx",
        "mov [rdi + 40], rbp",
        "mov [rdi + 48], rsp",

        // Load next context
        // Restore callee-saved registers from next context struct
        "mov r15, [rsi + 0]",
        "mov r14, [rsi + 8]",
        "mov r13, [rsi + 16]",
        "mov r12, [rsi + 24]",
        "mov rbx, [rsi + 32]",
        "mov rbp, [rsi + 40]",
        "mov rsp, [rsi + 48]",

        // Return to new task
        // The new RSP points to a stack with a return address on top
        "ret",
    )
}
