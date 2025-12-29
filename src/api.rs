//! Kernel API for Loaded Programs
//!
//! Provides a stable interface for programs to call kernel functions.
//! Programs receive a pointer to this API struct at startup.

use crate::scheduler;
use crate::task::TaskId;
use crate::executable::{self, LoadedProgram};
use crate::net::tcp;

/// Kernel API version
pub const API_VERSION: u32 = 4;

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
    /// Allocate memory (rounded up to 4KB)
    pub alloc: extern "C" fn(usize) -> *mut u8,
    /// Free memory (kernel tracks size, verifies ownership)
    pub free: extern "C" fn(*mut u8),

    // Network API (v4+)

    /// Create a TCP socket, returns socket handle or -1 on error
    pub net_socket: extern "C" fn() -> i32,
    /// Start TCP connection (non-blocking), returns 0 on success, -1 on error
    pub net_connect: extern "C" fn(sock: i32, ip: u32, port: u16) -> i32,
    /// Get socket status: 0=connecting, 1=connected, 2=closed, -1=error
    pub net_status: extern "C" fn(sock: i32) -> i32,
    /// Send data (non-blocking), returns bytes sent, 0 if buffer full, -1 on error
    pub net_send: extern "C" fn(sock: i32, data: *const u8, len: usize) -> i32,
    /// Receive data (non-blocking), returns bytes read, 0 if no data, -1 on error/closed
    pub net_recv: extern "C" fn(sock: i32, buf: *mut u8, len: usize) -> i32,
    /// Get bytes available to read
    pub net_available: extern "C" fn(sock: i32) -> i32,
    /// Close socket (starts graceful close)
    pub net_close: extern "C" fn(sock: i32),
    /// Listen on port, returns 0 on success, -1 on error
    pub net_listen: extern "C" fn(sock: i32, port: u16) -> i32,
    /// Accept connection (non-blocking), returns new socket, 0 if none pending, -1 on error
    pub net_accept: extern "C" fn(sock: i32) -> i32,
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

extern "C" fn api_alloc(size: usize) -> *mut u8 {
    if size == 0 {
        return core::ptr::null_mut();
    }

    let task_id = match scheduler::current_task_id() {
        Some(id) => id,
        None => return core::ptr::null_mut(),
    };

    match executable::task_alloc(task_id, size) {
        Some(addr) => addr as *mut u8,
        None => core::ptr::null_mut(),
    }
}

extern "C" fn api_free(ptr: *mut u8) {
    if ptr.is_null() {
        return;
    }

    let task_id = match scheduler::current_task_id() {
        Some(id) => id,
        None => return,
    };

    // Kernel looks up size and verifies ownership
    executable::task_free(task_id, ptr as usize);
}

// Network API implementation functions

extern "C" fn api_net_socket() -> i32 {
    match tcp::socket() {
        Some(sock) => sock as i32,
        None => -1,
    }
}

extern "C" fn api_net_connect(sock: i32, ip: u32, port: u16) -> i32 {
    if sock < 0 {
        return -1;
    }
    // Convert IP from u32 to [u8; 4] (network byte order)
    let ip_bytes = ip.to_be_bytes();
    if tcp::connect(sock as usize, &ip_bytes, port) {
        0
    } else {
        -1
    }
}

extern "C" fn api_net_status(sock: i32) -> i32 {
    if sock < 0 {
        return -1;
    }
    let state = tcp::get_state(sock as usize);
    match state {
        tcp::TcpState::SynSent | tcp::TcpState::SynReceived => 0,  // Connecting
        tcp::TcpState::Established => 1,  // Connected
        tcp::TcpState::Closed => 2,  // Closed
        tcp::TcpState::FinWait1 | tcp::TcpState::FinWait2 |
        tcp::TcpState::CloseWait | tcp::TcpState::Closing |
        tcp::TcpState::LastAck | tcp::TcpState::TimeWait => 2,  // Closing/Closed
        tcp::TcpState::Listen => 0,  // Listening (not connected yet)
    }
}

extern "C" fn api_net_send(sock: i32, data: *const u8, len: usize) -> i32 {
    if sock < 0 || data.is_null() {
        return -1;
    }
    let bytes = unsafe { core::slice::from_raw_parts(data, len) };
    tcp::send(sock as usize, bytes) as i32
}

extern "C" fn api_net_recv(sock: i32, buf: *mut u8, len: usize) -> i32 {
    if sock < 0 || buf.is_null() {
        return -1;
    }
    let buffer = unsafe { core::slice::from_raw_parts_mut(buf, len) };
    tcp::recv(sock as usize, buffer) as i32
}

extern "C" fn api_net_available(sock: i32) -> i32 {
    if sock < 0 {
        return 0;
    }
    tcp::available(sock as usize) as i32
}

extern "C" fn api_net_close(sock: i32) {
    if sock >= 0 {
        tcp::close(sock as usize);
    }
}

extern "C" fn api_net_listen(sock: i32, port: u16) -> i32 {
    if sock < 0 {
        return -1;
    }
    if tcp::listen(sock as usize, port) {
        0
    } else {
        -1
    }
}

extern "C" fn api_net_accept(sock: i32) -> i32 {
    if sock < 0 {
        return -1;
    }
    match tcp::accept(sock as usize) {
        Some(new_sock) => new_sock as i32,
        None => 0,  // No pending connection
    }
}

/// Global kernel API instance
pub static KERNEL_API: KernelApi = KernelApi {
    version: API_VERSION,
    print: api_print,
    yield_now: api_yield,
    sleep_ms: api_sleep,
    exit: api_exit,
    alloc: api_alloc,
    free: api_free,
    // Network API
    net_socket: api_net_socket,
    net_connect: api_net_connect,
    net_status: api_net_status,
    net_send: api_net_send,
    net_recv: api_net_recv,
    net_available: api_net_available,
    net_close: api_net_close,
    net_listen: api_net_listen,
    net_accept: api_net_accept,
};

/// Program entry point type
///
/// Programs must have an entry point with this signature.
/// The KernelApi pointer is valid for the lifetime of the program.
/// argv is a NULL-terminated array of pointers to null-terminated strings.
pub type ProgramEntry = extern "C" fn(api: &'static KernelApi, argv: *const *const u8);

/// Wrapper function that calls the program with the API pointer and argv
///
/// This is what gets registered as the task entry point.
/// It sets up the API pointer and argv, then calls the actual program.
fn program_wrapper(entry: usize) {
    let entry_fn: ProgramEntry = unsafe { core::mem::transmute(entry) };
    let argv = get_pending_argv();
    entry_fn(&KERNEL_API, argv);
}

/// Spawn a program as a task with arguments
///
/// Loads the named executable and spawns it as a new task.
/// The program name becomes argv[0], extra_args become argv[1..].
/// Returns the task ID on success.
pub fn spawn_program(name: &'static str, extra_args: &[&str]) -> Result<TaskId, executable::ExecError> {
    // Load the program
    let program = executable::load(name)?;

    // Spawn the task
    let task_id = spawn_program_task(name, &program)
        .ok_or(executable::ExecError::AllocationFailed)?;

    // Register program memory for cleanup
    executable::register_task_program(task_id, program.base_addr, program.size, name);

    // Allocate and set up argv in the task's memory
    let argv = allocate_args_for_task(task_id, name, extra_args)
        .ok_or(executable::ExecError::AllocationFailed)?;
    set_pending_argv(argv);

    Ok(task_id)
}

/// Spawn a program by name with arguments (for dynamic names like from BASIC)
///
/// This version takes a regular &str and uses "program" as the task name.
/// The program name becomes argv[0], extra_args become argv[1..].
pub fn spawn_program_dynamic(name: &str, extra_args: &[&str]) -> Result<TaskId, executable::ExecError> {
    // Load the program
    let program = executable::load(name)?;

    // Spawn the task with a generic static name
    let task_id = spawn_program_task("program", &program)
        .ok_or(executable::ExecError::AllocationFailed)?;

    // Register program memory for cleanup
    executable::register_task_program(task_id, program.base_addr, program.size, &program.name);

    // Allocate and set up argv in the task's memory
    let argv = allocate_args_for_task(task_id, name, extra_args)
        .ok_or(executable::ExecError::AllocationFailed)?;
    set_pending_argv(argv);

    Ok(task_id)
}

/// Internal: spawn a task for a loaded program
fn spawn_program_task(name: &'static str, program: &LoadedProgram) -> Option<TaskId> {
    // We need to create a task that will call program_wrapper with the entry point
    // The trick is we can't capture the entry point in a closure since spawn takes fn()

    // Store the entry point in a static that the task can read
    // This is safe because we're single-threaded cooperative
    set_pending_entry(program.entry);

    // Spawn using a static wrapper
    scheduler::spawn(name, pending_program_entry)
}

// Pending entry point and argv storage
static mut PENDING_ENTRY: usize = 0;
static mut PENDING_ARGV: *const *const u8 = core::ptr::null();

fn set_pending_entry(entry: usize) {
    unsafe { PENDING_ENTRY = entry; }
}

fn get_pending_entry() -> usize {
    unsafe { PENDING_ENTRY }
}

fn set_pending_argv(argv: *const *const u8) {
    unsafe { PENDING_ARGV = argv; }
}

fn get_pending_argv() -> *const *const u8 {
    unsafe { PENDING_ARGV }
}

/// Allocate argv array and strings in the task's memory
///
/// Creates a NULL-terminated argv array where argv[0] is the program name.
/// All memory is allocated in the task's program region for auto-cleanup.
fn allocate_args_for_task(
    task_id: TaskId,
    program_name: &str,
    extra_args: &[&str],
) -> Option<*const *const u8> {
    let total_args = 1 + extra_args.len(); // program_name + extra_args
    let ptr_size = core::mem::size_of::<*const u8>();
    let argv_size = (total_args + 1) * ptr_size; // +1 for NULL terminator

    let strings_size = program_name.len() + 1
        + extra_args.iter().map(|s| s.len() + 1).sum::<usize>();
    let total_size = argv_size + strings_size;

    let base = executable::task_alloc(task_id, total_size)?;

    // Layout: [argv pointers...][NULL][string data...]
    let argv_base = base as *mut *const u8;
    let mut strings_ptr = (base + argv_size) as *mut u8;

    unsafe {
        // Copy program name as argv[0]
        core::ptr::copy_nonoverlapping(program_name.as_ptr(), strings_ptr, program_name.len());
        *strings_ptr.add(program_name.len()) = 0;
        *argv_base = strings_ptr as *const u8;
        strings_ptr = strings_ptr.add(program_name.len() + 1);

        // Copy extra args as argv[1..]
        for (i, arg) in extra_args.iter().enumerate() {
            core::ptr::copy_nonoverlapping(arg.as_ptr(), strings_ptr, arg.len());
            *strings_ptr.add(arg.len()) = 0;
            *argv_base.add(i + 1) = strings_ptr as *const u8;
            strings_ptr = strings_ptr.add(arg.len() + 1);
        }

        // NULL terminate argv array
        *argv_base.add(total_args) = core::ptr::null();
    }

    Some(argv_base as *const *const u8)
}

/// Entry point for pending program (reads from PENDING_ENTRY)
fn pending_program_entry() {
    let entry = get_pending_entry();
    program_wrapper(entry);
}
