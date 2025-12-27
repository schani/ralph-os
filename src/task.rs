//! Task structure and context for cooperative multitasking

use alloc::vec;
use alloc::vec::Vec;

/// Unique identifier for each task
pub type TaskId = usize;

/// Stack size per task (16KB)
pub const STACK_SIZE: usize = 16 * 1024;

/// Task execution state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    /// Ready to run
    Ready,
    /// Currently executing
    Running,
    /// Sleeping until wake_at timestamp
    Sleeping,
    /// Task has completed
    Finished,
}

/// CPU context saved during context switch
///
/// For cooperative scheduling, we only save callee-saved registers.
/// The order must match the assembly in context_switch.rs.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Context {
    /// R15 register
    pub r15: u64,
    /// R14 register
    pub r14: u64,
    /// R13 register
    pub r13: u64,
    /// R12 register
    pub r12: u64,
    /// RBX register
    pub rbx: u64,
    /// RBP register (frame pointer)
    pub rbp: u64,
    /// RSP register (stack pointer) - saved last, restored first
    pub rsp: u64,
}

impl Default for Context {
    fn default() -> Self {
        Context {
            r15: 0,
            r14: 0,
            r13: 0,
            r12: 0,
            rbx: 0,
            rbp: 0,
            rsp: 0,
        }
    }
}

/// A schedulable task
pub struct Task {
    /// Unique task ID
    pub id: TaskId,
    /// Human-readable name
    pub name: &'static str,
    /// Current state
    pub state: TaskState,
    /// Saved CPU context
    pub context: Context,
    /// Task's private stack (heap-allocated)
    pub stack: Vec<u8>,
    /// Timestamp (in ticks) when sleeping task should wake
    pub wake_at: u64,
}

/// Entry point wrapper that calls the actual task function
/// and then marks the task as finished
#[unsafe(naked)]
extern "C" fn task_entry_trampoline() -> ! {
    // R12 contains the actual task entry point
    // We jump to it, and when it returns, we call exit_task
    core::arch::naked_asm!(
        // Call the task entry point stored in R12
        "call r12",
        // Task returned - call exit_task to clean up
        "call {exit_task}",
        // Should never reach here, but halt if we do
        "2:",
        "hlt",
        "jmp 2b",
        exit_task = sym crate::scheduler::exit_task,
    )
}

impl Task {
    /// Create a new task with the given entry point
    pub fn new(id: TaskId, name: &'static str, entry: fn()) -> Self {
        // Allocate stack
        let stack = vec![0u8; STACK_SIZE];

        // Set up initial stack for first context switch
        // Stack grows down, so start at high address
        let stack_top = stack.as_ptr() as usize + STACK_SIZE;

        // Align stack to 16 bytes (x86_64 ABI requirement)
        // We need 16-byte alignment BEFORE the call instruction pushes the return address
        // So we align to 16 and then subtract 8 to account for the return address
        let stack_aligned = (stack_top & !0xF) - 8;

        // Set up the stack with the return address (trampoline)
        let stack_ptr = stack_aligned as *mut u64;
        unsafe {
            // The context switch will pop registers and then `ret`
            // The `ret` will pop this address and jump to it
            *stack_ptr = task_entry_trampoline as *const () as usize as u64;
        }

        // Initial context
        // R12 will hold the actual entry point - the trampoline reads it
        let context = Context {
            rsp: stack_aligned as u64,
            r12: entry as usize as u64,
            ..Default::default()
        };

        Task {
            id,
            name,
            state: TaskState::Ready,
            context,
            stack,
            wake_at: 0,
        }
    }
}
