//! 8259 PIC (Programmable Interrupt Controller) driver
//!
//! Remaps the PIC to avoid conflicts with CPU exceptions and provides
//! functions to manage hardware interrupts.

use crate::io::{inb, io_wait, outb};

// PIC I/O ports
const PIC1_COMMAND: u16 = 0x20;
const PIC1_DATA: u16 = 0x21;
const PIC2_COMMAND: u16 = 0xA0;
const PIC2_DATA: u16 = 0xA1;

// PIC commands
const ICW1_INIT: u8 = 0x10;
const ICW1_ICW4: u8 = 0x01;
const ICW4_8086: u8 = 0x01;
const PIC_EOI: u8 = 0x20;

// Interrupt vector offsets after remapping
pub const PIC1_OFFSET: u8 = 32; // IRQ 0-7  -> interrupts 32-39
pub const PIC2_OFFSET: u8 = 40; // IRQ 8-15 -> interrupts 40-47

/// Initialize and remap the 8259 PICs
///
/// By default, IRQ 0-7 are mapped to interrupts 0x08-0x0F, which conflicts
/// with CPU exceptions. We remap them to 32-47.
pub fn init() {
    unsafe {
        // Save current masks
        let mask1 = inb(PIC1_DATA);
        let mask2 = inb(PIC2_DATA);

        // Start initialization sequence (ICW1)
        outb(PIC1_COMMAND, ICW1_INIT | ICW1_ICW4);
        io_wait();
        outb(PIC2_COMMAND, ICW1_INIT | ICW1_ICW4);
        io_wait();

        // Set vector offsets (ICW2)
        outb(PIC1_DATA, PIC1_OFFSET);
        io_wait();
        outb(PIC2_DATA, PIC2_OFFSET);
        io_wait();

        // Configure cascading (ICW3)
        // Tell Master PIC there's a slave at IRQ2
        outb(PIC1_DATA, 0x04);
        io_wait();
        // Tell Slave PIC its cascade identity
        outb(PIC2_DATA, 0x02);
        io_wait();

        // Set 8086 mode (ICW4)
        outb(PIC1_DATA, ICW4_8086);
        io_wait();
        outb(PIC2_DATA, ICW4_8086);
        io_wait();

        // Restore saved masks (all interrupts masked initially)
        outb(PIC1_DATA, mask1);
        outb(PIC2_DATA, mask2);
    }

    crate::println!("PIC remapped: IRQ0-7 -> {}-{}, IRQ8-15 -> {}-{}",
        PIC1_OFFSET, PIC1_OFFSET + 7,
        PIC2_OFFSET, PIC2_OFFSET + 7);
}

/// Enable a specific IRQ
pub fn enable_irq(irq: u8) {
    unsafe {
        if irq < 8 {
            // Master PIC
            let mask = inb(PIC1_DATA);
            outb(PIC1_DATA, mask & !(1 << irq));
        } else {
            // Slave PIC
            let mask = inb(PIC2_DATA);
            outb(PIC2_DATA, mask & !(1 << (irq - 8)));
            // Also enable IRQ2 on master (cascade)
            let mask1 = inb(PIC1_DATA);
            outb(PIC1_DATA, mask1 & !(1 << 2));
        }
    }
}

/// Disable a specific IRQ
pub fn disable_irq(irq: u8) {
    unsafe {
        if irq < 8 {
            let mask = inb(PIC1_DATA);
            outb(PIC1_DATA, mask | (1 << irq));
        } else {
            let mask = inb(PIC2_DATA);
            outb(PIC2_DATA, mask | (1 << (irq - 8)));
        }
    }
}

/// Send End-Of-Interrupt signal to the PIC(s)
///
/// Must be called at the end of every interrupt handler.
pub fn send_eoi(irq: u8) {
    unsafe {
        if irq >= 8 {
            // IRQ came from slave PIC, send EOI to both
            outb(PIC2_COMMAND, PIC_EOI);
        }
        // Always send EOI to master
        outb(PIC1_COMMAND, PIC_EOI);
    }
}

/// Disable all IRQs (mask all interrupts)
pub fn disable_all() {
    unsafe {
        outb(PIC1_DATA, 0xFF);
        outb(PIC2_DATA, 0xFF);
    }
}

/// Check if an IRQ is spurious
///
/// For IRQ7 on master or IRQ15 on slave, we need to check if it's real.
pub fn is_spurious(irq: u8) -> bool {
    unsafe {
        if irq == 7 {
            // Check master PIC ISR
            outb(PIC1_COMMAND, 0x0B); // Read ISR
            let isr = inb(PIC1_COMMAND);
            return (isr & 0x80) == 0; // Bit 7 = IRQ7
        } else if irq == 15 {
            // Check slave PIC ISR
            outb(PIC2_COMMAND, 0x0B);
            let isr = inb(PIC2_COMMAND);
            if (isr & 0x80) == 0 {
                // Spurious from slave, still need to EOI master
                outb(PIC1_COMMAND, PIC_EOI);
                return true;
            }
        }
        false
    }
}
