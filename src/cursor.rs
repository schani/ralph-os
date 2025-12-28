//! Mouse Cursor and Tooltip
//!
//! Handles cursor sprite rendering and memory info tooltip display.

use core::sync::atomic::{AtomicI16, AtomicBool, Ordering};
use crate::{vga, font, mouse};
use crate::vga::colors;

/// Cursor sprite size
const CURSOR_WIDTH: usize = 8;
const CURSOR_HEIGHT: usize = 8;

/// Cursor sprite bitmap (1 = white, 0 = transparent/black outline)
/// Arrow pointing up-left
const CURSOR_SPRITE: [u8; 8] = [
    0b11000000,  // XX......
    0b11100000,  // XXX.....
    0b11110000,  // XXXX....
    0b11111000,  // XXXXX...
    0b11111100,  // XXXXXX..
    0b11100000,  // XXX.....
    0b10010000,  // X..X....
    0b00011000,  // ...XX...
];

/// Cursor outline (black border for visibility)
const CURSOR_OUTLINE: [u8; 8] = [
    0b11100000,  // XXX.....
    0b11110000,  // XXXX....
    0b11111000,  // XXXXX...
    0b11111100,  // XXXXXX..
    0b11111110,  // XXXXXXX.
    0b11111100,  // XXXXXX..
    0b11111000,  // XXXXX...
    0b00111100,  // ..XXXX..
];

/// Tooltip dimensions
const TOOLTIP_WIDTH: usize = 152;   // 19 chars * 8 pixels
const TOOLTIP_HEIGHT: usize = 20;   // 2 lines * 8 + 4 padding
const TOOLTIP_PADDING: usize = 2;

/// Saved pixels under cursor (cursor area only)
static mut SAVED_CURSOR: [[u8; CURSOR_WIDTH]; CURSOR_HEIGHT] = [[0; CURSOR_WIDTH]; CURSOR_HEIGHT];

/// Saved pixels under tooltip (need larger buffer)
static mut SAVED_TOOLTIP: [[u8; TOOLTIP_WIDTH]; TOOLTIP_HEIGHT] = [[0; TOOLTIP_WIDTH]; TOOLTIP_HEIGHT];

/// Last cursor position (for erasing)
static LAST_CURSOR_X: AtomicI16 = AtomicI16::new(-100);  // Off-screen initially
static LAST_CURSOR_Y: AtomicI16 = AtomicI16::new(-100);

/// Last tooltip position
static LAST_TOOLTIP_X: AtomicI16 = AtomicI16::new(-100);
static LAST_TOOLTIP_Y: AtomicI16 = AtomicI16::new(-100);
static TOOLTIP_VISIBLE: AtomicBool = AtomicBool::new(false);

/// Memory region boundaries (must match memvis.rs)
const VIS_BASE: usize = 0x100000;
const KERNEL_END: usize = 0x200000;
const HEAP_END: usize = 0x400000;
const PROGRAM_END: usize = 0x1000000;

/// Read a pixel from VGA framebuffer
#[inline]
fn read_pixel(x: usize, y: usize) -> u8 {
    if x >= vga::WIDTH || y >= vga::HEIGHT {
        return 0;
    }
    let offset = y * vga::WIDTH + x;
    unsafe {
        let fb = 0xA0000 as *const u8;
        fb.add(offset).read_volatile()
    }
}

/// Save pixels under cursor area
fn save_cursor_pixels(x: i16, y: i16) {
    let bx = x as usize;
    let by = y as usize;
    for row in 0..CURSOR_HEIGHT {
        for col in 0..CURSOR_WIDTH {
            unsafe {
                SAVED_CURSOR[row][col] = read_pixel(bx + col, by + row);
            }
        }
    }
}

/// Restore saved cursor pixels
fn restore_cursor_pixels(x: i16, y: i16) {
    if x < 0 || y < 0 {
        return;
    }
    let bx = x as usize;
    let by = y as usize;
    for row in 0..CURSOR_HEIGHT {
        for col in 0..CURSOR_WIDTH {
            unsafe {
                vga::set_pixel(bx + col, by + row, SAVED_CURSOR[row][col]);
            }
        }
    }
}

/// Save pixels under tooltip area
fn save_tooltip_pixels(x: i16, y: i16) {
    let bx = x as usize;
    let by = y as usize;
    for row in 0..TOOLTIP_HEIGHT {
        for col in 0..TOOLTIP_WIDTH {
            unsafe {
                SAVED_TOOLTIP[row][col] = read_pixel(bx + col, by + row);
            }
        }
    }
}

/// Restore saved tooltip pixels
fn restore_tooltip_pixels(x: i16, y: i16) {
    if x < 0 || y < 0 {
        return;
    }
    let bx = x as usize;
    let by = y as usize;
    for row in 0..TOOLTIP_HEIGHT {
        for col in 0..TOOLTIP_WIDTH {
            unsafe {
                vga::set_pixel(bx + col, by + row, SAVED_TOOLTIP[row][col]);
            }
        }
    }
}

/// Draw cursor sprite at position
fn draw_cursor_sprite(x: i16, y: i16) {
    let bx = x as usize;
    let by = y as usize;

    for row in 0..CURSOR_HEIGHT {
        let outline_bits = CURSOR_OUTLINE[row];
        let sprite_bits = CURSOR_SPRITE[row];

        for col in 0..CURSOR_WIDTH {
            let mask = 0x80 >> col;
            if sprite_bits & mask != 0 {
                vga::set_pixel(bx + col, by + row, colors::WHITE);
            } else if outline_bits & mask != 0 {
                vga::set_pixel(bx + col, by + row, colors::BLACK);
            }
        }
    }
}

/// Convert pixel position to memory address
fn pixel_to_addr(x: i16, y: i16) -> usize {
    let pixel_index = (y as usize) * vga::WIDTH + (x as usize);
    VIS_BASE + (pixel_index << 8)  // * 256 bytes per pixel
}

/// Get region name and allocation state for an address
fn get_region_info(addr: usize) -> (&'static str, bool) {
    // Read the pixel color to determine allocation state
    let pixel_index = (addr - VIS_BASE) >> 8;
    let py = pixel_index / vga::WIDTH;
    let px = pixel_index % vga::WIDTH;
    let pixel_color = read_pixel(px, py);

    if addr < KERNEL_END {
        ("Kernel", true)  // Kernel is always "allocated"
    } else if addr < HEAP_END {
        let allocated = pixel_color == colors::RED;
        ("Heap", allocated)
    } else if addr < PROGRAM_END {
        let allocated = pixel_color == colors::MAGENTA;
        ("Program", allocated)
    } else {
        ("Unknown", false)
    }
}

/// Calculate tooltip position (flip if near edge)
fn calculate_tooltip_pos(cursor_x: i16, cursor_y: i16) -> (i16, i16) {
    let mut tx = cursor_x + 12;  // Offset from cursor
    let mut ty = cursor_y + 12;

    // Flip horizontally if would go off right edge
    if tx + TOOLTIP_WIDTH as i16 > vga::WIDTH as i16 {
        tx = cursor_x - TOOLTIP_WIDTH as i16 - 4;
    }

    // Flip vertically if would go off bottom edge
    if ty + TOOLTIP_HEIGHT as i16 > vga::HEIGHT as i16 {
        ty = cursor_y - TOOLTIP_HEIGHT as i16 - 4;
    }

    // Clamp to screen
    tx = tx.max(0);
    ty = ty.max(0);

    (tx, ty)
}

/// Draw tooltip box with memory info
fn draw_tooltip(x: i16, y: i16, addr: usize, region: &str, allocated: bool) {
    let bx = x as usize;
    let by = y as usize;

    // Draw background
    vga::fill_rect(bx, by, TOOLTIP_WIDTH, TOOLTIP_HEIGHT, colors::DARK_GRAY);

    // Draw border
    vga::hline(bx, by, TOOLTIP_WIDTH, colors::WHITE);
    vga::hline(bx, by + TOOLTIP_HEIGHT - 1, TOOLTIP_WIDTH, colors::WHITE);
    for row in 0..TOOLTIP_HEIGHT {
        vga::set_pixel(bx, by + row, colors::WHITE);
        vga::set_pixel(bx + TOOLTIP_WIDTH - 1, by + row, colors::WHITE);
    }

    // Line 1: Address range (each pixel = 256 bytes)
    let addr_end = addr + 255;
    let text_x = bx + TOOLTIP_PADDING + 2;
    let line1_y = by + TOOLTIP_PADDING + 1;

    // Draw "0xXXXXXX-0xXXXXXX"
    font::draw_hex_bg(text_x, line1_y, addr, 6, colors::WHITE, colors::DARK_GRAY);
    font::draw_char_bg(text_x + 64, line1_y, '-', colors::WHITE, colors::DARK_GRAY);
    font::draw_hex_bg(text_x + 72, line1_y, addr_end, 6, colors::WHITE, colors::DARK_GRAY);

    // Line 2: Region name and state
    let line2_y = line1_y + 10;
    font::draw_string_bg(text_x, line2_y, region, colors::WHITE, colors::DARK_GRAY);

    // Draw allocation state
    let state_str = if allocated { " (used)" } else { " (free)" };
    let state_x = text_x + region.len() * 8;
    font::draw_string_bg(state_x, line2_y, state_str, colors::LIGHT_GRAY, colors::DARK_GRAY);
}

/// Update cursor and tooltip (called from timer tick)
pub fn update() {
    if !vga::is_enabled() || !mouse::is_initialized() {
        return;
    }

    if !mouse::cursor_dirty() {
        return;
    }

    // Get new position
    let (new_x, new_y) = mouse::position();
    let old_x = LAST_CURSOR_X.load(Ordering::Relaxed);
    let old_y = LAST_CURSOR_Y.load(Ordering::Relaxed);

    // Erase old tooltip first (it's on top of everything)
    if TOOLTIP_VISIBLE.load(Ordering::Relaxed) {
        let tooltip_x = LAST_TOOLTIP_X.load(Ordering::Relaxed);
        let tooltip_y = LAST_TOOLTIP_Y.load(Ordering::Relaxed);
        restore_tooltip_pixels(tooltip_x, tooltip_y);
        TOOLTIP_VISIBLE.store(false, Ordering::Relaxed);
    }

    // Erase old cursor
    restore_cursor_pixels(old_x, old_y);

    // Save pixels under new cursor position
    save_cursor_pixels(new_x, new_y);

    // Calculate new tooltip position
    let (tooltip_x, tooltip_y) = calculate_tooltip_pos(new_x, new_y);

    // Save pixels under new tooltip position
    save_tooltip_pixels(tooltip_x, tooltip_y);

    // Draw cursor
    draw_cursor_sprite(new_x, new_y);

    // Draw tooltip
    let addr = pixel_to_addr(new_x, new_y);
    let (region, allocated) = get_region_info(addr);
    draw_tooltip(tooltip_x, tooltip_y, addr, region, allocated);

    // Update state
    LAST_CURSOR_X.store(new_x, Ordering::Relaxed);
    LAST_CURSOR_Y.store(new_y, Ordering::Relaxed);
    LAST_TOOLTIP_X.store(tooltip_x, Ordering::Relaxed);
    LAST_TOOLTIP_Y.store(tooltip_y, Ordering::Relaxed);
    TOOLTIP_VISIBLE.store(true, Ordering::Relaxed);

    mouse::clear_dirty();
}

/// Initialize cursor (draw initial cursor)
pub fn init() {
    if !vga::is_enabled() {
        return;
    }

    let (x, y) = mouse::position();
    save_cursor_pixels(x, y);
    draw_cursor_sprite(x, y);

    let (tooltip_x, tooltip_y) = calculate_tooltip_pos(x, y);
    save_tooltip_pixels(tooltip_x, tooltip_y);

    let addr = pixel_to_addr(x, y);
    let (region, allocated) = get_region_info(addr);
    draw_tooltip(tooltip_x, tooltip_y, addr, region, allocated);

    LAST_CURSOR_X.store(x, Ordering::Relaxed);
    LAST_CURSOR_Y.store(y, Ordering::Relaxed);
    LAST_TOOLTIP_X.store(tooltip_x, Ordering::Relaxed);
    LAST_TOOLTIP_Y.store(tooltip_y, Ordering::Relaxed);
    TOOLTIP_VISIBLE.store(true, Ordering::Relaxed);

    mouse::clear_dirty();
}
