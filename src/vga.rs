//! VGA Mode 13h Driver
//!
//! Provides direct framebuffer access for 320x200x256 VGA graphics mode.
//! Used for memory visualization when debug mode is enabled.

use core::sync::atomic::{AtomicBool, Ordering};

/// VGA framebuffer address (linear, identity-mapped)
const FRAMEBUFFER: usize = 0xA0000;

/// Screen dimensions
pub const WIDTH: usize = 320;
pub const HEIGHT: usize = 200;

/// Total pixels
pub const TOTAL_PIXELS: usize = WIDTH * HEIGHT;

/// Magic address where bootloader stores VGA mode status
const VGA_STATUS_ADDR: usize = 0x501;

/// VGA mode 13h identifier
const VGA_MODE_13H: u8 = 0x13;

/// Color palette indices (using VGA default palette)
pub mod colors {
    pub const BLACK: u8 = 0;
    pub const BLUE: u8 = 1;
    pub const GREEN: u8 = 2;
    pub const CYAN: u8 = 3;
    pub const RED: u8 = 4;
    pub const MAGENTA: u8 = 5;
    pub const BROWN: u8 = 6;
    pub const LIGHT_GRAY: u8 = 7;
    pub const DARK_GRAY: u8 = 8;
    pub const LIGHT_BLUE: u8 = 9;
    pub const LIGHT_GREEN: u8 = 10;
    pub const LIGHT_CYAN: u8 = 11;
    pub const LIGHT_RED: u8 = 12;
    pub const LIGHT_MAGENTA: u8 = 13;
    pub const YELLOW: u8 = 14;
    pub const WHITE: u8 = 15;
}

/// Static flag indicating whether VGA mode is active
static VGA_ENABLED: AtomicBool = AtomicBool::new(false);

/// Initialize the VGA driver
///
/// Checks if the bootloader set VGA mode 13h by reading the status byte
/// at address 0x501. Must be called early in kernel initialization.
pub fn init() {
    // Read VGA status from magic address set by bootloader
    let status = unsafe { *(VGA_STATUS_ADDR as *const u8) };

    if status == VGA_MODE_13H {
        VGA_ENABLED.store(true, Ordering::Release);

        // Clear screen to black
        clear(colors::BLACK);

        crate::println!("[vga] Mode 13h active (320x200x256)");
    }
}

/// Check if VGA mode is enabled
#[inline]
pub fn is_enabled() -> bool {
    VGA_ENABLED.load(Ordering::Acquire)
}

/// Set a single pixel at (x, y) to the specified color
///
/// Does nothing if VGA is not enabled or coordinates are out of bounds.
#[inline]
pub fn set_pixel(x: usize, y: usize, color: u8) {
    if !is_enabled() || x >= WIDTH || y >= HEIGHT {
        return;
    }

    let offset = y * WIDTH + x;
    unsafe {
        let fb = FRAMEBUFFER as *mut u8;
        fb.add(offset).write_volatile(color);
    }
}

/// Set a pixel by linear index (0..64000)
///
/// Does nothing if VGA is not enabled or index is out of bounds.
#[inline]
pub fn set_pixel_index(index: usize, color: u8) {
    if !is_enabled() || index >= TOTAL_PIXELS {
        return;
    }

    unsafe {
        let fb = FRAMEBUFFER as *mut u8;
        fb.add(index).write_volatile(color);
    }
}

/// Fill a range of pixels with a color
///
/// Fills pixels from start_index to start_index + count.
/// Clips to screen bounds.
pub fn fill_range(start_index: usize, count: usize, color: u8) {
    if !is_enabled() || start_index >= TOTAL_PIXELS {
        return;
    }

    let end = (start_index + count).min(TOTAL_PIXELS);
    let fb = FRAMEBUFFER as *mut u8;

    unsafe {
        for i in start_index..end {
            fb.add(i).write_volatile(color);
        }
    }
}

/// Fill a rectangular region with a color
pub fn fill_rect(x: usize, y: usize, w: usize, h: usize, color: u8) {
    if !is_enabled() {
        return;
    }

    for row in y..(y + h).min(HEIGHT) {
        for col in x..(x + w).min(WIDTH) {
            let offset = row * WIDTH + col;
            unsafe {
                let fb = FRAMEBUFFER as *mut u8;
                fb.add(offset).write_volatile(color);
            }
        }
    }
}

/// Clear the entire screen to a color
pub fn clear(color: u8) {
    if !is_enabled() {
        return;
    }

    let fb = FRAMEBUFFER as *mut u8;
    unsafe {
        for i in 0..TOTAL_PIXELS {
            fb.add(i).write_volatile(color);
        }
    }
}

/// Draw a horizontal line
pub fn hline(x: usize, y: usize, length: usize, color: u8) {
    if !is_enabled() || y >= HEIGHT {
        return;
    }

    let start = y * WIDTH + x;
    let end = (start + length).min(y * WIDTH + WIDTH);

    let fb = FRAMEBUFFER as *mut u8;
    unsafe {
        for i in start..end {
            fb.add(i).write_volatile(color);
        }
    }
}
