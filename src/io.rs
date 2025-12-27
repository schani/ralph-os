//! Low-level port I/O operations
//!
//! Provides x86 port I/O primitives used by device drivers.

use core::arch::asm;

/// Read a byte from an I/O port
#[inline]
pub unsafe fn inb(port: u16) -> u8 {
    let value: u8;
    asm!(
        "in al, dx",
        out("al") value,
        in("dx") port,
        options(nomem, nostack, preserves_flags)
    );
    value
}

/// Write a byte to an I/O port
#[inline]
pub unsafe fn outb(port: u16, value: u8) {
    asm!(
        "out dx, al",
        in("dx") port,
        in("al") value,
        options(nomem, nostack, preserves_flags)
    );
}

/// Small delay for device operations
///
/// Writes to an unused port (0x80) to create a ~1us delay.
/// Used when devices need time between consecutive I/O operations.
#[inline]
pub unsafe fn io_wait() {
    outb(0x80, 0);
}
