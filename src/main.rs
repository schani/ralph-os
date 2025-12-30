#![no_std]
#![no_main]
#![feature(alloc_error_handler)]
#![allow(dead_code)]
#![allow(static_mut_refs)]

extern crate alloc;

mod allocator;
mod api;
mod basic;
mod context_switch;
mod cursor;
mod elf;
mod executable;
mod font;
mod gilbert;
mod idt;
mod interrupts;
mod io;
mod meminfo;
mod mouse;
mod net;
mod pic;
mod program_alloc;
mod scheduler;
mod serial;
mod task;
mod timer;
mod vga;
mod memvis;
mod telnet;

use core::panic::PanicInfo;

/// Heap configuration
const HEAP_START: usize = 0x200000;
const HEAP_SIZE: usize = 0x200000;

extern "C" {
    static mut __bss_start: u8;
    static mut __bss_end: u8;
}

const EXEC_TABLE_MAGIC: [u8; 4] = *b"REXE";
const EXEC_TABLE_HEADER_SIZE: usize = 512;
const EXEC_TABLE_MAX_ENTRIES: usize = 15;
const EXEC_TABLE_ENTRY_SIZE: usize = 32;

#[inline]
const fn align_up(value: usize, alignment: usize) -> usize {
    (value + alignment - 1) & !(alignment - 1)
}

#[no_mangle]
extern "C" fn relocate_exec_table() {
    // Exec table is appended after the raw kernel binary in the disk image.
    // The kernel's `.bss` is NOBITS and therefore NOT present in `kernel.bin`,
    // so the appended data can overlap `.bss` in memory. If we then clear `.bss`,
    // we erase the exec table blobs (including `.bas` source).
    //
    // To avoid this, locate the exec table in the low region, copy it to the
    // safe gap between `__bss_end` and `HEAP_START`, and invalidate the original
    // table header so the normal scanner finds the relocated copy.
    const SEARCH_START: usize = 0x100000;
    const SEARCH_END: usize = HEAP_START;

    unsafe {
        let bss_end = core::ptr::addr_of!(__bss_end) as usize;
        let dest_base = align_up(bss_end, 512);
        if dest_base >= HEAP_START {
            return;
        }

        let mut addr = SEARCH_START;
        while addr + 4 <= SEARCH_END {
            let magic = core::ptr::read(addr as *const [u8; 4]);
            if magic != EXEC_TABLE_MAGIC {
                addr += 4;
                continue;
            }

            // Quick header validation.
            let version = core::ptr::read_unaligned((addr + 4) as *const u32);
            let count = core::ptr::read_unaligned((addr + 8) as *const u32) as usize;
            if version != 1 || count > EXEC_TABLE_MAX_ENTRIES {
                addr += 4;
                continue;
            }

            // Compute total table size including 512-byte aligned blobs.
            let mut total = EXEC_TABLE_HEADER_SIZE;
            for i in 0..count {
                let entry = addr + 16 + i * EXEC_TABLE_ENTRY_SIZE;
                let offset = core::ptr::read_unaligned((entry + 16) as *const u32) as usize;
                let size = core::ptr::read_unaligned((entry + 20) as *const u32) as usize;
                if offset < EXEC_TABLE_HEADER_SIZE || size == 0 {
                    total = 0;
                    break;
                }
                let end = offset.saturating_add(align_up(size, 512));
                if end > total {
                    total = end;
                }
            }
            if total == 0 {
                addr += 4;
                continue;
            }

            let dest_end = dest_base.saturating_add(total);
            if dest_end > HEAP_START {
                return;
            }

            core::ptr::copy_nonoverlapping(addr as *const u8, dest_base as *mut u8, total);
            core::ptr::write_bytes(addr as *mut u8, 0, 4);
            return;
        }
    }
}

#[no_mangle]
extern "C" fn zero_bss() {
    unsafe {
        let start = core::ptr::addr_of_mut!(__bss_start) as *mut u8;
        let end = core::ptr::addr_of_mut!(__bss_end) as *mut u8;
        let size = end as usize - start as usize;
        core::ptr::write_bytes(start, 0, size);
    }
}

/// Kernel entry point
#[unsafe(naked)]
#[no_mangle]
#[link_section = ".text.boot"]
pub unsafe extern "C" fn _start() -> ! {
    core::arch::naked_asm!(
        "mov rsp, 0x90000",
        "call {relocate_exec_table}",
        "call {zero_bss}",
        "call {kernel_main}",
        "2:",
        "hlt",
        "jmp 2b",
        relocate_exec_table = sym relocate_exec_table,
        zero_bss = sym zero_bss,
        kernel_main = sym kernel_main,
    )
}

/// Main kernel function
#[no_mangle]
pub extern "C" fn kernel_main() -> ! {
    // Initialize serial port
    serial::init();

    // Initialize VGA if debug mode enabled
    vga::init();

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

    // Initialize Gilbert curve tables (required for memory visualization)
    if vga::is_enabled() {
        gilbert::init();
    }

    // Initialize memory visualizer (draws initial state if VGA enabled)
    memvis::init();

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

    // Initialize mouse (only useful in VGA mode)
    if vga::is_enabled() {
        println!("\nInitializing mouse...");
        if mouse::init() {
            pic::enable_irq(12);  // Enable PS/2 mouse IRQ
            println!("IRQ12 enabled (PS/2 mouse)");
            cursor::init();  // Draw initial cursor
        }
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
        match scheduler::spawn("telnetd", telnet::telnetd_task) {
            Some(_) => println!("  - telnetd: Telnet BASIC sessions on port 23"),
            None => println!("  - telnetd: FAILED (out of memory)"),
        }
    }
    match scheduler::spawn("basic-repl", basic::repl_task) {
        Some(_) => println!("  - basic-repl: Interactive BASIC interpreter"),
        None => println!("  - basic-repl: FAILED (out of memory)"),
    }

    // Mark scheduler as ready for per-task allocation tracking
    allocator::mark_scheduler_ready();

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
