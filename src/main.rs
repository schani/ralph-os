#![no_std]
#![no_main]
#![feature(alloc_error_handler)]

extern crate alloc;

mod allocator;
mod serial;

use alloc::string::String;
use alloc::vec::Vec;
use core::panic::PanicInfo;

/// Heap configuration
/// Heap starts at 2MB and is 2MB in size (2MB - 4MB range)
const HEAP_START: usize = 0x200000;
const HEAP_SIZE: usize = 0x200000; // 2MB

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

    // Initialize heap allocator
    println!("\nInitializing heap allocator...");
    unsafe {
        allocator::init_heap(HEAP_START, HEAP_SIZE);
    }
    println!("Heap: 0x{:X} - 0x{:X} ({} KB)",
             HEAP_START, HEAP_START + HEAP_SIZE, HEAP_SIZE / 1024);

    // Test heap allocation
    test_heap_allocation();

    println!("\nCustom bootloader + kernel - no external dependencies!");

    // Halt
    loop {
        hlt();
    }
}

/// Test heap allocation with Vec and String
fn test_heap_allocation() {
    println!("\n--- Heap Allocation Tests ---");

    // Test Vec allocation
    print!("Testing Vec<u32>... ");
    let mut numbers: Vec<u32> = Vec::new();
    for i in 0..10 {
        numbers.push(i * i);
    }
    println!("OK (len={})", numbers.len());

    // Verify contents
    print!("  Squares: ");
    for (i, &n) in numbers.iter().enumerate() {
        if i > 0 {
            print!(", ");
        }
        print!("{}", n);
    }
    println!();

    // Test String allocation
    print!("Testing String... ");
    let mut greeting = String::from("Hello");
    greeting.push_str(", ");
    greeting.push_str("Ralph OS");
    greeting.push('!');
    println!("OK (len={})", greeting.len());
    println!("  String: \"{}\"", greeting);

    // Test Box allocation
    print!("Testing Box<[u8; 1024]>... ");
    let boxed_array = alloc::boxed::Box::new([0u8; 1024]);
    println!("OK (1KB allocated)");
    // Use the array to prevent optimization
    let _ = boxed_array[0];

    // Test multiple allocations and deallocations
    print!("Testing alloc/dealloc cycle... ");
    for _ in 0..50 {
        let v: Vec<u64> = (0..20).collect();
        let _ = v.len(); // Use to prevent optimization
    }
    println!("OK (50 cycles)");

    println!("--- All heap tests passed ---");
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

/// Allocation error handler (called when allocation fails)
#[alloc_error_handler]
fn alloc_error(layout: core::alloc::Layout) -> ! {
    panic!("Allocation failed: {:?}", layout);
}
