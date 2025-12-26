#![no_std]
#![no_main]

mod serial;

use core::panic::PanicInfo;

/// Kernel entry point - called from bootloader
/// Must be at 0x100000 (start of .text section)
#[unsafe(naked)]
#[no_mangle]
#[link_section = ".text.boot"]
pub unsafe extern "C" fn _start() -> ! {
    // Set up a known-good stack and call kernel_main
    core::arch::naked_asm!(
        "mov rsp, 0x90000",     // Set stack pointer
        "call kernel_main",     // Call Rust main
        "2:",
        "hlt",
        "jmp 2b",
    )
}

/// Main kernel function
#[no_mangle]
pub extern "C" fn kernel_main() -> ! {
    // Initialize serial port
    serial::init();

    // Print welcome message
    println!("Hello, Ralph OS!");
    println!("Kernel loaded at 0x100000");
    println!("Custom bootloader + kernel - no external dependencies!");

    // Halt
    loop {
        hlt();
    }
}

/// Halt the CPU until the next interrupt
#[inline]
fn hlt() {
    unsafe {
        core::arch::asm!("hlt", options(nomem, nostack, preserves_flags));
    }
}

/// Panic handler
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("\n!!! KERNEL PANIC !!!");
    println!("{}", info);
    loop {
        hlt();
    }
}
