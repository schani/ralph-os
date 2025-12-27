//! Kernel API for Loaded Programs
//!
//! Provides a stable interface for programs to call kernel functions.
//! Programs receive a pointer to this API struct at startup.

use crate::scheduler;
use crate::task::TaskId;
use crate::executable::{self, LoadedProgram};

/// Kernel API version
pub const API_VERSION: u32 = 1;

/// Kernel API structure passed to programs
///
/// This struct is passed to program entry points. Programs use these
/// function pointers to access kernel services.
#[repr(C)]
pub struct KernelApi {
    /// API version number
    pub version: u32,
    /// Print a string to the console
    pub print: extern "C" fn(*const u8, usize),
    /// Yield to other tasks
    pub yield_now: extern "C" fn(),
    /// Sleep for milliseconds
    pub sleep_ms: extern "C" fn(u64),
    /// Exit the current program
    pub exit: extern "C" fn() -> !,
}

// API implementation functions

extern "C" fn api_print(ptr: *const u8, len: usize) {
    if ptr.is_null() || len == 0 {
        return;
    }

    let bytes = unsafe { core::slice::from_raw_parts(ptr, len) };
    if let Ok(s) = core::str::from_utf8(bytes) {
        crate::print!("{}", s);
    }
}

extern "C" fn api_yield() {
    scheduler::yield_now();
}

extern "C" fn api_sleep(ms: u64) {
    scheduler::sleep_ms(ms);
}

extern "C" fn api_exit() -> ! {
    scheduler::exit_task();
    // exit_task() should never return, but just in case
    loop {
        unsafe { core::arch::asm!("hlt"); }
    }
}

/// Global kernel API instance
pub static KERNEL_API: KernelApi = KernelApi {
    version: API_VERSION,
    print: api_print,
    yield_now: api_yield,
    sleep_ms: api_sleep,
    exit: api_exit,
};

/// Program entry point type
///
/// Programs must have an entry point with this signature.
/// The KernelApi pointer is valid for the lifetime of the program.
pub type ProgramEntry = extern "C" fn(api: &'static KernelApi);

/// Wrapper function that calls the program with the API pointer
///
/// This is what gets registered as the task entry point.
/// It sets up the API pointer and calls the actual program.
fn program_wrapper(entry: usize) {
    let entry_fn: ProgramEntry = unsafe { core::mem::transmute(entry) };
    entry_fn(&KERNEL_API);
}

/// Spawn a program as a task
///
/// Loads the named executable and spawns it as a new task.
/// Returns the task ID on success.
pub fn spawn_program(name: &'static str) -> Result<TaskId, executable::ExecError> {
    // Load the program
    let program = executable::load(name)?;

    // Spawn the task
    let task_id = spawn_program_task(name, &program);

    // Register for cleanup
    executable::register_task(task_id, &program);

    Ok(task_id)
}

/// Spawn a program by name (for dynamic names like from BASIC)
///
/// This version takes a regular &str and uses "program" as the task name.
pub fn spawn_program_dynamic(name: &str) -> Result<TaskId, executable::ExecError> {
    // Load the program
    let program = executable::load(name)?;

    // Spawn the task with a generic static name
    let task_id = spawn_program_task("program", &program);

    // Register for cleanup
    executable::register_task(task_id, &program);

    Ok(task_id)
}

/// Internal: spawn a task for a loaded program
fn spawn_program_task(name: &'static str, program: &LoadedProgram) -> TaskId {
    // We need to create a task that will call program_wrapper with the entry point
    // The trick is we can't capture the entry point in a closure since spawn takes fn()

    // Store the entry point in a static that the task can read
    // This is safe because we're single-threaded cooperative
    set_pending_entry(program.entry);

    // Spawn using a static wrapper
    scheduler::spawn(name, pending_program_entry)
}

// Pending entry point storage
static mut PENDING_ENTRY: usize = 0;

fn set_pending_entry(entry: usize) {
    unsafe { PENDING_ENTRY = entry; }
}

fn get_pending_entry() -> usize {
    unsafe { PENDING_ENTRY }
}

/// Entry point for pending program (reads from PENDING_ENTRY)
fn pending_program_entry() {
    let entry = get_pending_entry();
    program_wrapper(entry);
}
