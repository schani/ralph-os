//! Hello World - Example Ralph OS Program
//!
//! This is a minimal program that demonstrates the executable loading system.

#![no_std]
#![no_main]

use core::panic::PanicInfo;

/// Kernel API structure (must match kernel's api.rs)
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

/// Print a string using the kernel API
fn print(api: &KernelApi, s: &str) {
    (api.print)(s.as_ptr(), s.len());
}

/// Program entry point
///
/// This function is called by the kernel with a pointer to the kernel API
/// and a NULL-terminated array of argument strings.
#[no_mangle]
pub extern "C" fn _start(api: &'static KernelApi, argv: *const *const u8) -> ! {
    print(api, "Hello from a dynamically loaded program!\n");

    // Print arguments
    print(api, "Arguments:\n");
    let mut i = 0;
    unsafe {
        while !(*argv.add(i)).is_null() {
            print(api, "  argv[");
            print_digit(api, i);
            print(api, "] = \"");

            // Print the null-terminated string
            let mut ptr = *argv.add(i);
            while *ptr != 0 {
                let s = core::slice::from_raw_parts(ptr, 1);
                (api.print)(s.as_ptr(), 1);
                ptr = ptr.add(1);
            }

            print(api, "\"\n");
            i += 1;
        }
    }

    print(api, "API version: ");
    print_digit(api, api.version as usize);
    print(api, "\n");

    // Demonstrate yielding
    print(api, "Yielding to other tasks...\n");
    (api.yield_now)();

    // Demonstrate sleeping
    print(api, "Sleeping for 500ms...\n");
    (api.sleep_ms)(500);

    print(api, "Hello program finished!\n");

    // Exit cleanly
    (api.exit)()
}

/// Print a single digit (0-9)
fn print_digit(api: &KernelApi, n: usize) {
    if n < 10 {
        let digit = b'0' + n as u8;
        let s = [digit];
        (api.print)(s.as_ptr(), 1);
    } else {
        print(api, "??");
    }
}

/// Panic handler - required for no_std
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    // Can't do much without kernel API access here
    loop {
        core::hint::spin_loop();
    }
}
