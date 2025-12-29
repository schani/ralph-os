//! Interrupt handlers for hardware and CPU interrupts
//!
//! Contains assembly stubs that save/restore state and call Rust handlers.

use crate::io::inb;
use crate::mouse;
use crate::net;
use crate::pic;
use crate::timer;

/// Timer interrupt handler (IRQ0 -> interrupt 32)
///
/// This is called by the assembly stub after saving registers.
#[no_mangle]
extern "C" fn timer_handler() {
    // Increment the tick count
    timer::tick();

    // Send End-Of-Interrupt to PIC
    pic::send_eoi(0);
}

/// Spurious interrupt handler
///
/// Handles spurious interrupts from the PIC without doing anything harmful.
#[no_mangle]
extern "C" fn spurious_handler() {
    // Check if it's really spurious
    // For IRQ7, don't send EOI if spurious
    // The is_spurious check handles IRQ15 EOI internally
    if !pic::is_spurious(7) {
        pic::send_eoi(7);
    }
}

/// Keyboard interrupt handler (IRQ1 -> interrupt 33)
///
/// Just reads the scancode to clear the interrupt - we don't process keyboard input.
#[no_mangle]
extern "C" fn keyboard_handler() {
    // Read scancode to clear the keyboard controller buffer
    unsafe { let _ = inb(0x60); }

    // Send End-Of-Interrupt to PIC
    pic::send_eoi(1);
}

/// Timer ISR stub - saves state, calls handler, restores state
#[unsafe(naked)]
#[no_mangle]
pub unsafe extern "C" fn isr_timer() {
    core::arch::naked_asm!(
        // Save all caller-saved registers
        "push rax",
        "push rcx",
        "push rdx",
        "push rsi",
        "push rdi",
        "push r8",
        "push r9",
        "push r10",
        "push r11",

        // Call the Rust handler
        "call {handler}",

        // Restore registers
        "pop r11",
        "pop r10",
        "pop r9",
        "pop r8",
        "pop rdi",
        "pop rsi",
        "pop rdx",
        "pop rcx",
        "pop rax",

        // Return from interrupt
        "iretq",

        handler = sym timer_handler,
    );
}

/// Keyboard ISR stub - saves state, calls handler, restores state
#[unsafe(naked)]
#[no_mangle]
pub unsafe extern "C" fn isr_keyboard() {
    core::arch::naked_asm!(
        // Save all caller-saved registers
        "push rax",
        "push rcx",
        "push rdx",
        "push rsi",
        "push rdi",
        "push r8",
        "push r9",
        "push r10",
        "push r11",

        // Call the Rust handler
        "call {handler}",

        // Restore registers
        "pop r11",
        "pop r10",
        "pop r9",
        "pop r8",
        "pop rdi",
        "pop rsi",
        "pop rdx",
        "pop rcx",
        "pop rax",

        // Return from interrupt
        "iretq",

        handler = sym keyboard_handler,
    );
}

/// NE2000 network card interrupt handler (IRQ10 -> interrupt 42)
///
/// This is called by the assembly stub after saving registers.
#[no_mangle]
extern "C" fn ne2000_handler() {
    // Handle the interrupt (reads packets into buffer pool)
    net::ne2000::handle_interrupt();

    // Send End-Of-Interrupt to both PICs (IRQ10 is on slave PIC)
    pic::send_eoi(10);
}

/// NE2000 ISR stub - saves state, calls handler, restores state
#[unsafe(naked)]
#[no_mangle]
pub unsafe extern "C" fn isr_ne2000() {
    core::arch::naked_asm!(
        // Save all caller-saved registers
        "push rax",
        "push rcx",
        "push rdx",
        "push rsi",
        "push rdi",
        "push r8",
        "push r9",
        "push r10",
        "push r11",

        // Call the Rust handler
        "call {handler}",

        // Restore registers
        "pop r11",
        "pop r10",
        "pop r9",
        "pop r8",
        "pop rdi",
        "pop rsi",
        "pop rdx",
        "pop rcx",
        "pop rax",

        // Return from interrupt
        "iretq",

        handler = sym ne2000_handler,
    );
}

/// PS/2 mouse interrupt handler (IRQ12 -> interrupt 44)
///
/// This is called by the assembly stub after saving registers.
#[no_mangle]
extern "C" fn mouse_handler() {
    // Handle the interrupt (read mouse packet)
    mouse::handle_interrupt();

    // Send End-Of-Interrupt to both PICs (IRQ12 is on slave PIC)
    pic::send_eoi(12);
}

/// Mouse ISR stub - saves state, calls handler, restores state
#[unsafe(naked)]
#[no_mangle]
pub unsafe extern "C" fn isr_mouse() {
    core::arch::naked_asm!(
        // Save all caller-saved registers
        "push rax",
        "push rcx",
        "push rdx",
        "push rsi",
        "push rdi",
        "push r8",
        "push r9",
        "push r10",
        "push r11",

        // Call the Rust handler
        "call {handler}",

        // Restore registers
        "pop r11",
        "pop r10",
        "pop r9",
        "pop r8",
        "pop rdi",
        "pop rsi",
        "pop rdx",
        "pop rcx",
        "pop rax",

        // Return from interrupt
        "iretq",

        handler = sym mouse_handler,
    );
}

/// Spurious ISR stub
#[unsafe(naked)]
#[no_mangle]
pub unsafe extern "C" fn isr_spurious() {
    core::arch::naked_asm!(
        "push rax",
        "push rbx",
        "push rcx",
        "push rdx",
        "push rsi",
        "push rdi",
        "push rbp",
        "push r8",
        "push r9",
        "push r10",
        "push r11",
        "push r12",
        "push r13",
        "push r14",
        "push r15",

        "call spurious_handler",

        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop r11",
        "pop r10",
        "pop r9",
        "pop r8",
        "pop rbp",
        "pop rdi",
        "pop rsi",
        "pop rdx",
        "pop rcx",
        "pop rbx",
        "pop rax",

        "iretq",
    );
}
