//! Memory Address Space Visualizer
//!
//! Provides real-time visualization of memory allocation state on VGA display.
//! Each pixel represents 256 bytes of address space from 0x100000 to 0x1000000.
//!
//! Memory regions:
//! - 0x100000 - 0x1FFFFF: Kernel (blue)
//! - 0x200000 - 0x3FFFFF: Heap (green=free, red=allocated)
//! - 0x400000 - 0xFFFFFF: Program region (cyan=free, magenta=allocated)

use crate::vga::{self, colors};

/// Shadow buffer to track memory visualization state
/// This allows us to redraw the screen without losing allocation info
static mut SHADOW_BUFFER: [u8; 320 * 200] = [0; 320 * 200];

/// Base address for visualization (1MB)
const VIS_BASE: usize = 0x100000;

/// End address for visualization (16MB)
const VIS_END: usize = 0x1000000;

/// Bytes per pixel (256 = 2^8, allows fast shifting)
const BYTES_PER_PIXEL: usize = 256;

/// Memory region boundaries
const KERNEL_END: usize = 0x200000;
const HEAP_START: usize = 0x200000;
const HEAP_END: usize = 0x400000;
const PROGRAM_START: usize = 0x400000;
const PROGRAM_END: usize = 0x1000000;

/// Convert a memory address to a pixel index
///
/// Returns None if the address is outside the visualized range.
#[inline]
fn addr_to_pixel(addr: usize) -> Option<usize> {
    if addr < VIS_BASE || addr >= VIS_END {
        return None;
    }
    Some((addr - VIS_BASE) >> 8) // divide by 256
}

/// Fill a range in both shadow buffer and VGA
#[inline]
fn fill_both(start: usize, count: usize, color: u8) {
    // Update shadow buffer
    unsafe {
        for i in start..start + count {
            if i < SHADOW_BUFFER.len() {
                SHADOW_BUFFER[i] = color;
            }
        }
    }
    // Update VGA
    vga::fill_range(start, count, color);
}

/// Get the appropriate "allocated" color for an address
#[inline]
fn alloc_color_for_addr(addr: usize) -> u8 {
    if addr >= PROGRAM_START {
        colors::MAGENTA // Program region allocated
    } else if addr >= HEAP_START {
        colors::RED // Heap allocated
    } else {
        colors::BLUE // Kernel (shouldn't change)
    }
}

/// Get the appropriate "free" color for an address
#[inline]
fn free_color_for_addr(addr: usize) -> u8 {
    if addr >= PROGRAM_START {
        colors::CYAN // Program region free
    } else if addr >= HEAP_START {
        colors::GREEN // Heap free
    } else {
        colors::BLUE // Kernel (shouldn't change)
    }
}

/// Initialize the memory visualizer
///
/// Draws the initial memory map:
/// - Kernel region as blue
/// - Heap region as green (free)
/// - Program region as cyan (free)
pub fn init() {
    if !vga::is_enabled() {
        return;
    }

    // Draw kernel region (0x100000 - 0x1FFFFF) as blue
    let kernel_start_pixel = addr_to_pixel(VIS_BASE).unwrap_or(0);
    let kernel_end_pixel = addr_to_pixel(KERNEL_END).unwrap_or(0);
    fill_both(kernel_start_pixel, kernel_end_pixel - kernel_start_pixel, colors::BLUE);

    // Draw heap region (0x200000 - 0x3FFFFF) as green (initially all free)
    let heap_start_pixel = addr_to_pixel(HEAP_START).unwrap_or(0);
    let heap_end_pixel = addr_to_pixel(HEAP_END).unwrap_or(0);
    fill_both(heap_start_pixel, heap_end_pixel - heap_start_pixel, colors::GREEN);

    // Draw program region (0x400000 - 0xFFFFFF) as cyan (initially all free)
    let prog_start_pixel = addr_to_pixel(PROGRAM_START).unwrap_or(0);
    let prog_end_pixel = addr_to_pixel(PROGRAM_END).unwrap_or(0);
    fill_both(prog_start_pixel, prog_end_pixel - prog_start_pixel, colors::CYAN);

    crate::println!("[memvis] Visualization initialized");
}

/// Get pixel color from shadow buffer (for cursor tooltip)
pub fn get_pixel(x: usize, y: usize) -> u8 {
    let index = y * 320 + x;
    if index < 320 * 200 {
        unsafe { SHADOW_BUFFER[index] }
    } else {
        0
    }
}

/// Redraw the entire memory visualization from shadow buffer
pub fn redraw() {
    if !vga::is_enabled() {
        return;
    }

    // Copy shadow buffer to VGA framebuffer
    unsafe {
        let fb = 0xA0000 as *mut u8;
        for i in 0..SHADOW_BUFFER.len() {
            fb.add(i).write_volatile(SHADOW_BUFFER[i]);
        }
    }
}

/// Called when memory is allocated
///
/// Marks the allocated region with the appropriate "allocated" color.
pub fn on_alloc(addr: usize, size: usize) {
    if !vga::is_enabled() {
        return;
    }

    let color = alloc_color_for_addr(addr);
    draw_region(addr, size, color);
}

/// Called when memory is deallocated
///
/// Marks the freed region with the appropriate "free" color.
pub fn on_dealloc(addr: usize, size: usize) {
    if !vga::is_enabled() {
        return;
    }

    let color = free_color_for_addr(addr);
    draw_region(addr, size, color);
}

/// Draw a memory region with the specified color (updates both shadow and VGA)
fn draw_region(addr: usize, size: usize, color: u8) {
    let start_pixel = match addr_to_pixel(addr) {
        Some(p) => p,
        None => return,
    };

    // Calculate end pixel (round up to include partial pixels)
    let end_addr = addr.saturating_add(size);
    let end_pixel = match addr_to_pixel(end_addr.saturating_sub(1)) {
        Some(p) => p + 1,
        None => {
            // End is past our range, clip to max
            if end_addr > VIS_END {
                addr_to_pixel(VIS_END - 1).map(|p| p + 1).unwrap_or(start_pixel)
            } else {
                return;
            }
        }
    };

    let count = end_pixel.saturating_sub(start_pixel);
    if count > 0 {
        fill_both(start_pixel, count, color);
    }
}

/// Update visualization for program allocator initialization
///
/// Called after program_alloc::init() to mark the program region as free.
pub fn on_program_alloc_init() {
    if !vga::is_enabled() {
        return;
    }

    // Redraw program region as free
    let prog_start_pixel = addr_to_pixel(PROGRAM_START).unwrap_or(0);
    let prog_end_pixel = addr_to_pixel(PROGRAM_END).unwrap_or(0);
    fill_both(prog_start_pixel, prog_end_pixel - prog_start_pixel, colors::CYAN);
}
