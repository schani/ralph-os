//! Interrupt Descriptor Table (IDT) for x86_64
//!
//! Sets up interrupt handlers for CPU exceptions and hardware interrupts.

use core::arch::asm;

/// Number of IDT entries (256 possible interrupts)
const IDT_ENTRIES: usize = 256;

/// IDT entry (Gate Descriptor) for x86_64
#[repr(C, packed)]
#[derive(Clone, Copy)]
struct IdtEntry {
    /// Offset bits 0-15
    offset_low: u16,
    /// Code segment selector
    selector: u16,
    /// Bits 0-2: IST offset, bits 3-7: reserved
    ist: u8,
    /// Gate type, DPL, and present bit
    type_attr: u8,
    /// Offset bits 16-31
    offset_mid: u16,
    /// Offset bits 32-63
    offset_high: u32,
    /// Reserved
    reserved: u32,
}

impl IdtEntry {
    /// Create an empty/null IDT entry
    const fn null() -> Self {
        IdtEntry {
            offset_low: 0,
            selector: 0,
            ist: 0,
            type_attr: 0,
            offset_mid: 0,
            offset_high: 0,
            reserved: 0,
        }
    }

    /// Create an interrupt gate entry
    ///
    /// # Arguments
    /// * `handler` - Address of the interrupt handler function
    /// * `selector` - Code segment selector (usually 0x08 for kernel code)
    /// * `ist` - Interrupt Stack Table index (0 = don't switch stacks)
    fn new(handler: u64, selector: u16, ist: u8) -> Self {
        IdtEntry {
            offset_low: (handler & 0xFFFF) as u16,
            selector,
            ist: ist & 0x7,
            // 0x8E = Present + DPL 0 + Interrupt Gate (0xE)
            type_attr: 0x8E,
            offset_mid: ((handler >> 16) & 0xFFFF) as u16,
            offset_high: ((handler >> 32) & 0xFFFFFFFF) as u32,
            reserved: 0,
        }
    }
}

/// IDT pointer structure for LIDT instruction
#[repr(C, packed)]
struct IdtPointer {
    /// Size of IDT minus 1
    limit: u16,
    /// Base address of IDT
    base: u64,
}

/// The IDT itself - must be static for the CPU to reference
static mut IDT: [IdtEntry; IDT_ENTRIES] = [IdtEntry::null(); IDT_ENTRIES];

/// IDT pointer for LIDT
static mut IDT_PTR: IdtPointer = IdtPointer { limit: 0, base: 0 };

// External interrupt handler stubs defined in interrupts.rs
extern "C" {
    fn isr_timer();
    fn isr_spurious();
}

/// Code segment selector for 64-bit mode (from GDT in bootloader)
const KERNEL_CS: u16 = 0x18;

/// Initialize the IDT
pub fn init() {
    unsafe {
        // Set up timer interrupt (IRQ0 -> interrupt 32 after PIC remapping)
        IDT[32] = IdtEntry::new(isr_timer as *const () as u64, KERNEL_CS, 0);

        // Set up spurious interrupt handler (IRQ7 -> interrupt 39)
        IDT[39] = IdtEntry::new(isr_spurious as *const () as u64, KERNEL_CS, 0);

        // Also handle spurious on IRQ15 (interrupt 47)
        IDT[47] = IdtEntry::new(isr_spurious as *const () as u64, KERNEL_CS, 0);

        // Set up the IDT pointer
        IDT_PTR = IdtPointer {
            limit: (core::mem::size_of::<[IdtEntry; IDT_ENTRIES]>() - 1) as u16,
            base: IDT.as_ptr() as u64,
        };

        // Load the IDT
        asm!(
            "lidt [{}]",
            in(reg) &raw const IDT_PTR,
            options(nostack, preserves_flags)
        );
    }

    crate::println!("IDT loaded");
}

/// Enable hardware interrupts
pub fn enable_interrupts() {
    unsafe {
        asm!("sti", options(nostack, preserves_flags));
    }
}

/// Disable hardware interrupts
pub fn disable_interrupts() {
    unsafe {
        asm!("cli", options(nostack, preserves_flags));
    }
}

/// Check if interrupts are enabled
pub fn are_interrupts_enabled() -> bool {
    let flags: u64;
    unsafe {
        asm!(
            "pushfq",
            "pop {}",
            out(reg) flags,
            options(nomem, preserves_flags)
        );
    }
    // Bit 9 is the IF (Interrupt Flag)
    (flags & (1 << 9)) != 0
}
