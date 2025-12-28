//! PS/2 Mouse Driver
//!
//! Handles PS/2 mouse input via IRQ12. Provides cursor position tracking
//! for the VGA memory visualizer.

use core::sync::atomic::{AtomicBool, AtomicI16, AtomicU8, Ordering};
use crate::io::{inb, outb, io_wait};

/// PS/2 controller ports
const PS2_DATA: u16 = 0x60;
const PS2_STATUS: u16 = 0x64;
const PS2_COMMAND: u16 = 0x64;

/// PS/2 controller commands
const CMD_READ_CONFIG: u8 = 0x20;
const CMD_WRITE_CONFIG: u8 = 0x60;
const CMD_ENABLE_AUX: u8 = 0xA8;
const CMD_WRITE_AUX: u8 = 0xD4;

/// Mouse commands
const MOUSE_SET_DEFAULTS: u8 = 0xF6;
const MOUSE_ENABLE_REPORTING: u8 = 0xF4;

/// Mouse response
const MOUSE_ACK: u8 = 0xFA;

/// Screen boundaries
const SCREEN_WIDTH: i16 = 320;
const SCREEN_HEIGHT: i16 = 200;

/// Mouse state (atomic for IRQ safety)
static MOUSE_X: AtomicI16 = AtomicI16::new(160);  // Start at center
static MOUSE_Y: AtomicI16 = AtomicI16::new(100);
static CURSOR_DIRTY: AtomicBool = AtomicBool::new(true);  // Draw initial cursor
static MOUSE_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Packet accumulator
static PACKET_BYTE_INDEX: AtomicU8 = AtomicU8::new(0);
static PACKET_0: AtomicU8 = AtomicU8::new(0);
static PACKET_1: AtomicU8 = AtomicU8::new(0);
static PACKET_2: AtomicU8 = AtomicU8::new(0);

/// Drain any pending data from the output buffer
fn drain_output_buffer() {
    for _ in 0..100 {
        unsafe {
            let status = inb(PS2_STATUS);
            if status & 0x01 == 0 {
                break;  // Output buffer empty
            }
            let _ = inb(PS2_DATA);  // Discard data
            io_wait();
        }
    }
}

/// Wait for PS/2 controller input buffer to be ready (very short timeout)
fn wait_input_ready() -> bool {
    for _ in 0..100 {
        unsafe {
            if inb(PS2_STATUS) & 0x02 == 0 {
                return true;
            }
        }
        // Small delay without io_wait to be faster
        for _ in 0..10 { unsafe { core::arch::asm!("nop"); } }
    }
    false
}

/// Wait for PS/2 controller output buffer to have data (very short timeout)
fn wait_output_ready() -> bool {
    for _ in 0..100 {
        unsafe {
            if inb(PS2_STATUS) & 0x01 != 0 {
                return true;
            }
        }
        for _ in 0..10 { unsafe { core::arch::asm!("nop"); } }
    }
    false
}

/// Send command to PS/2 controller
fn send_command(cmd: u8) -> bool {
    if !wait_input_ready() {
        return false;
    }
    unsafe {
        outb(PS2_COMMAND, cmd);
    }
    true
}

/// Send command to mouse (via PS/2 controller)
fn send_mouse_command(cmd: u8) -> bool {
    // Tell controller next byte goes to mouse
    if !send_command(CMD_WRITE_AUX) {
        return false;
    }

    if !wait_input_ready() {
        return false;
    }

    unsafe {
        outb(PS2_DATA, cmd);
    }

    // Wait for ACK
    if !wait_output_ready() {
        return false;
    }

    unsafe {
        let response = inb(PS2_DATA);
        response == MOUSE_ACK
    }
}

/// Initialize the PS/2 mouse
pub fn init() -> bool {
    // Drain any pending data first
    drain_output_buffer();

    // Enable auxiliary port (mouse)
    if !send_command(CMD_ENABLE_AUX) {
        crate::println!("[mouse] Failed to enable aux port");
        return false;
    }

    drain_output_buffer();

    // Get and modify compaq status byte to enable IRQ12
    // Command 0x20 reads, 0x60 writes the "command byte"
    // But that hangs, so we use command 0xD4 to talk to mouse directly

    // Just send mouse commands - QEMU should have IRQ12 enabled by default
    if !send_mouse_command(MOUSE_SET_DEFAULTS) {
        crate::println!("[mouse] Failed to set defaults");
        return false;
    }

    if !send_mouse_command(MOUSE_ENABLE_REPORTING) {
        crate::println!("[mouse] Failed to enable reporting");
        return false;
    }

    MOUSE_INITIALIZED.store(true, Ordering::Release);
    crate::println!("[mouse] PS/2 mouse initialized");
    true
}

/// Check if mouse is initialized
#[inline]
pub fn is_initialized() -> bool {
    MOUSE_INITIALIZED.load(Ordering::Acquire)
}

/// Handle mouse interrupt (called from IRQ12 handler)
pub fn handle_interrupt() {
    // Check if data is from mouse (bit 5 of status = aux data)
    let status = unsafe { inb(PS2_STATUS) };
    if status & 0x20 == 0 {
        // Not mouse data, ignore
        return;
    }

    let data = unsafe { inb(PS2_DATA) };
    let byte_index = PACKET_BYTE_INDEX.load(Ordering::Relaxed);

    match byte_index {
        0 => {
            // First byte must have bit 3 set (always 1 in standard packet)
            if data & 0x08 != 0 {
                PACKET_0.store(data, Ordering::Relaxed);
                PACKET_BYTE_INDEX.store(1, Ordering::Relaxed);
            }
            // Otherwise discard (out of sync)
        }
        1 => {
            PACKET_1.store(data, Ordering::Relaxed);
            PACKET_BYTE_INDEX.store(2, Ordering::Relaxed);
        }
        2 => {
            PACKET_2.store(data, Ordering::Relaxed);
            PACKET_BYTE_INDEX.store(0, Ordering::Relaxed);

            // Complete packet - process it
            process_packet();
        }
        _ => {
            // Should never happen, reset
            PACKET_BYTE_INDEX.store(0, Ordering::Relaxed);
        }
    }
}

/// Process a complete 3-byte mouse packet
fn process_packet() {
    let flags = PACKET_0.load(Ordering::Relaxed);
    let dx_raw = PACKET_1.load(Ordering::Relaxed);
    let dy_raw = PACKET_2.load(Ordering::Relaxed);

    // Check for overflow (bits 6 and 7 of flags)
    if flags & 0xC0 != 0 {
        return; // Discard overflow packets
    }

    // Extract signed deltas (9-bit with sign extension)
    let dx: i16 = if flags & 0x10 != 0 {
        // X sign bit set - negative
        dx_raw as i16 | 0xFF00u16 as i16
    } else {
        dx_raw as i16
    };

    let dy: i16 = if flags & 0x20 != 0 {
        // Y sign bit set - negative
        dy_raw as i16 | 0xFF00u16 as i16
    } else {
        dy_raw as i16
    };

    // Update cursor position
    // Note: PS/2 Y axis is inverted (positive = up)
    let old_x = MOUSE_X.load(Ordering::Relaxed);
    let old_y = MOUSE_Y.load(Ordering::Relaxed);

    let new_x = (old_x + dx).clamp(0, SCREEN_WIDTH - 1);
    let new_y = (old_y - dy).clamp(0, SCREEN_HEIGHT - 1);  // Invert Y

    MOUSE_X.store(new_x, Ordering::Relaxed);
    MOUSE_Y.store(new_y, Ordering::Relaxed);

    // Mark cursor as needing redraw
    if new_x != old_x || new_y != old_y {
        CURSOR_DIRTY.store(true, Ordering::Release);
    }
}

/// Get current cursor position
#[inline]
pub fn position() -> (i16, i16) {
    (
        MOUSE_X.load(Ordering::Relaxed),
        MOUSE_Y.load(Ordering::Relaxed),
    )
}

/// Check if cursor needs redrawing
#[inline]
pub fn cursor_dirty() -> bool {
    CURSOR_DIRTY.load(Ordering::Acquire)
}

/// Clear the dirty flag after redrawing
#[inline]
pub fn clear_dirty() {
    CURSOR_DIRTY.store(false, Ordering::Release);
}
