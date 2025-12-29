//! Memory Address Space Visualizer
//!
//! Provides real-time visualization of memory allocation state on VGA display.
//! Each pixel represents 256 bytes of address space from 0x100000 to 0x1000000.
//!
//! Uses a Gilbert curve (generalized Hilbert) to map memory addresses to screen
//! positions, ensuring that adjacent memory addresses map to adjacent pixels.
//!
//! Memory regions:
//! - 0x100000 - 0x1FFFFF: Kernel (blue)
//! - 0x200000 - 0x3FFFFF: Heap (green=free, red=allocated)
//! - 0x400000 - 0xFFFFFF: Program region (cyan=free, magenta=allocated)

use crate::gilbert;
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

/// Convert a memory address to Gilbert curve index
///
/// Returns None if the address is outside the visualized range.
#[inline]
fn addr_to_gilbert_index(addr: usize) -> Option<usize> {
    if addr < VIS_BASE || addr >= VIS_END {
        return None;
    }
    let index = (addr - VIS_BASE) >> 8; // divide by 256
    if index >= gilbert::TOTAL_PIXELS {
        return None;
    }
    Some(index)
}

/// Convert a memory address to screen (x, y) coordinates using Gilbert curve
#[inline]
fn addr_to_xy(addr: usize) -> Option<(usize, usize)> {
    addr_to_gilbert_index(addr).map(|d| gilbert::d_to_xy(d))
}

/// Set a single pixel in both shadow buffer and VGA using screen coordinates
#[inline]
fn set_pixel_xy(x: usize, y: usize, color: u8) {
    if x >= vga::WIDTH || y >= vga::HEIGHT {
        return;
    }
    let screen_index = y * vga::WIDTH + x;
    unsafe {
        if screen_index < SHADOW_BUFFER.len() {
            SHADOW_BUFFER[screen_index] = color;
        }
    }
    vga::set_pixel(x, y, color);
}

/// Fill a range of Gilbert indices with a color
///
/// Sets pixels at the Gilbert curve positions for indices [start_d, end_d)
fn fill_gilbert_range(start_d: usize, end_d: usize, color: u8) {
    let end_d = end_d.min(gilbert::TOTAL_PIXELS);
    for d in start_d..end_d {
        let (x, y) = gilbert::d_to_xy(d);
        set_pixel_xy(x, y, color);
    }
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
/// Draws the initial memory map using Gilbert curve layout:
/// - Kernel region as blue
/// - Heap region as green (free)
/// - Program region as cyan (free)
pub fn init() {
    if !vga::is_enabled() {
        return;
    }

    // Draw kernel region (0x100000 - 0x1FFFFF) as blue
    let kernel_start_d = addr_to_gilbert_index(VIS_BASE).unwrap_or(0);
    let kernel_end_d = addr_to_gilbert_index(KERNEL_END).unwrap_or(0);
    fill_gilbert_range(kernel_start_d, kernel_end_d, colors::BLUE);

    // Draw heap region (0x200000 - 0x3FFFFF) as green (initially all free)
    let heap_start_d = addr_to_gilbert_index(HEAP_START).unwrap_or(0);
    let heap_end_d = addr_to_gilbert_index(HEAP_END).unwrap_or(0);
    fill_gilbert_range(heap_start_d, heap_end_d, colors::GREEN);

    // Draw program region (0x400000 - 0xFFFFFF) as cyan (initially all free)
    let prog_start_d = addr_to_gilbert_index(PROGRAM_START).unwrap_or(0);
    // PROGRAM_END is at the boundary, use TOTAL_PIXELS directly
    let prog_end_d = gilbert::TOTAL_PIXELS;
    fill_gilbert_range(prog_start_d, prog_end_d, colors::CYAN);

    crate::println!("[memvis] Gilbert curve visualization initialized");
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

/// Draw a memory region with the specified color using Gilbert curve layout
fn draw_region(addr: usize, size: usize, color: u8) {
    let start_d = match addr_to_gilbert_index(addr) {
        Some(d) => d,
        None => return,
    };

    // Calculate end Gilbert index (round up to include partial pixels)
    let end_addr = addr.saturating_add(size);
    let end_d = match addr_to_gilbert_index(end_addr.saturating_sub(1)) {
        Some(d) => d + 1,
        None => {
            // End is past our range, clip to max
            if end_addr > VIS_END {
                gilbert::TOTAL_PIXELS
            } else {
                return;
            }
        }
    };

    if end_d > start_d {
        fill_gilbert_range(start_d, end_d, color);
    }
}

/// Update visualization for program allocator initialization
///
/// Called after program_alloc::init() to mark the program region as free.
pub fn on_program_alloc_init() {
    if !vga::is_enabled() {
        return;
    }

    // Redraw program region as free using Gilbert curve
    let prog_start_d = addr_to_gilbert_index(PROGRAM_START).unwrap_or(0);
    let prog_end_d = addr_to_gilbert_index(PROGRAM_END).unwrap_or(gilbert::TOTAL_PIXELS);
    fill_gilbert_range(prog_start_d, prog_end_d, colors::CYAN);
}
