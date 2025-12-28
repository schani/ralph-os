#![no_std]
#![no_main]
#![feature(alloc_error_handler)]

extern crate alloc;

mod allocator;
mod api;
mod basic;
mod context_switch;
mod elf;
mod executable;
mod idt;
mod interrupts;
mod io;
mod net;
mod pic;
mod program_alloc;
mod scheduler;
mod serial;
mod task;
mod timer;

use core::panic::PanicInfo;

/// Heap configuration
const HEAP_START: usize = 0x200000;
const HEAP_SIZE: usize = 0x200000;

/// Kernel entry point
#[unsafe(naked)]
#[no_mangle]
#[link_section = ".text.boot"]
pub unsafe extern "C" fn _start() -> ! {
    core::arch::naked_asm!(
        "mov rsp, 0x90000",
        "call kernel_main",
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

    // Welcome
    println!("Hello, Ralph OS!");
    println!("Kernel loaded at 0x100000");

    // Initialize heap
    println!("\nInitializing heap allocator...");
    unsafe {
        allocator::init_heap(HEAP_START, HEAP_SIZE);
    }
    println!(
        "Heap: 0x{:X} - 0x{:X} ({} KB)",
        HEAP_START,
        HEAP_START + HEAP_SIZE,
        HEAP_SIZE / 1024
    );

    // Initialize PIC
    println!("\nInitializing PIC...");
    pic::init();

    // Initialize IDT
    println!("Initializing IDT...");
    idt::init();

    // Initialize timer
    println!("\nInitializing timer...");
    timer::init();
    println!("Timer: {} Hz", timer::ticks_per_second());

    // Enable IRQ0
    pic::enable_irq(0);
    println!("IRQ0 enabled");

    // Enable CPU interrupts
    idt::enable_interrupts();
    println!("Interrupts enabled (STI)");

    // Initialize scheduler
    println!("\nInitializing scheduler...");
    scheduler::init();

    // Initialize network subsystem
    println!("\nInitializing network...");
    net::init();
    if net::ne2000::init() {
        pic::enable_irq(10);  // Enable NE2000 IRQ
        println!("IRQ10 enabled (NE2000)");
    }

    // Initialize executable subsystem
    println!("\nInitializing executable loader...");
    match executable::init() {
        Ok(count) => {
            if count > 0 {
                println!("Available executables:");
                for name in executable::list() {
                    println!("  - {}", name);
                }
            }
        }
        Err(e) => {
            println!("Warning: Failed to initialize executables: {:?}", e);
        }
    }

    // Spawn tasks
    println!("\nSpawning tasks...");
    if net::ne2000::is_initialized() {
        match scheduler::spawn("network", net::network_task) {
            Some(_) => println!("  - network: Network protocol handler"),
            None => println!("  - network: FAILED (out of memory)"),
        }
    }
    match scheduler::spawn("memstats", basic::memstats_task) {
        Some(_) => println!("  - memstats: Memory monitor (BASIC)"),
        None => println!("  - memstats: FAILED (out of memory)"),
    }
    match scheduler::spawn("basic-repl", basic::repl_task) {
        Some(_) => println!("  - basic-repl: Interactive BASIC interpreter"),
        None => println!("  - basic-repl: FAILED (out of memory)"),
    }

    println!("\nStarting scheduler...\n");

    // Run scheduler (never returns)
    scheduler::run()
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
