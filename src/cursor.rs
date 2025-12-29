//! Mouse Cursor and Tooltip
//!
//! Handles cursor sprite rendering and memory info tooltip display.
//! Simply redraws the entire screen on each update - no save/restore needed.

use crate::{vga, font, mouse, memvis};
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
const TOOLTIP_WIDTH: usize = 168;   // 21 chars * 8 pixels (wider for full addresses)
const TOOLTIP_HEIGHT: usize = 20;   // 2 lines * 8 + 4 padding
const TOOLTIP_PADDING: usize = 2;

/// Bytes per pixel (must match memvis.rs)
const BYTES_PER_PIXEL: usize = 256;

/// Memory region boundaries (must match memvis.rs)
const VIS_BASE: usize = 0x100000;
const KERNEL_END: usize = 0x200000;
const HEAP_END: usize = 0x400000;
const PROGRAM_END: usize = 0x1000000;

/// Draw cursor sprite at position
fn draw_cursor_sprite(x: i16, y: i16) {
    if x < 0 || y < 0 {
        return;
    }
    let bx = x as usize;
    let by = y as usize;

    for row in 0..CURSOR_HEIGHT {
        let outline_bits = CURSOR_OUTLINE[row];
        let sprite_bits = CURSOR_SPRITE[row];

        for col in 0..CURSOR_WIDTH {
            let px = bx + col;
            let py = by + row;
            if px < vga::WIDTH && py < vga::HEIGHT {
                let mask = 0x80 >> col;
                if sprite_bits & mask != 0 {
                    vga::set_pixel(px, py, colors::WHITE);
                } else if outline_bits & mask != 0 {
                    vga::set_pixel(px, py, colors::BLACK);
                }
            }
        }
    }
}

/// Convert pixel position to memory address
fn pixel_to_addr(x: i16, y: i16) -> usize {
    let pixel_index = (y as usize) * vga::WIDTH + (x as usize);
    VIS_BASE + (pixel_index << 8)  // * 256 bytes per pixel
}

/// Convert pixel index to memory address
fn pixel_index_to_addr(index: usize) -> usize {
    VIS_BASE + (index * BYTES_PER_PIXEL)
}

/// Find the contiguous region of the same color that contains the cursor position.
/// Returns (start_addr, end_addr) of the region.
fn find_contiguous_region(x: i16, y: i16) -> (usize, usize) {
    let cursor_index = (y as usize) * vga::WIDTH + (x as usize);
    let cursor_color = memvis::get_pixel(x as usize, y as usize);
    let max_pixels = vga::WIDTH * vga::HEIGHT;

    // Scan backward to find start of region
    let mut start_index = cursor_index;
    while start_index > 0 {
        let prev_index = start_index - 1;
        let prev_y = prev_index / vga::WIDTH;
        let prev_x = prev_index % vga::WIDTH;
        let prev_color = memvis::get_pixel(prev_x, prev_y);
        if prev_color != cursor_color {
            break;
        }
        start_index = prev_index;
    }

    // Scan forward to find end of region
    let mut end_index = cursor_index;
    while end_index + 1 < max_pixels {
        let next_index = end_index + 1;
        let next_y = next_index / vga::WIDTH;
        let next_x = next_index % vga::WIDTH;
        let next_color = memvis::get_pixel(next_x, next_y);
        if next_color != cursor_color {
            break;
        }
        end_index = next_index;
    }

    let start_addr = pixel_index_to_addr(start_index);
    let end_addr = pixel_index_to_addr(end_index) + BYTES_PER_PIXEL - 1;

    (start_addr, end_addr)
}

/// Get region name and allocation state for an address
fn get_region_info(addr: usize, x: i16, y: i16) -> (&'static str, bool) {
    // Read the pixel color from shadow buffer (not VGA, which may have cursor overlay)
    let pixel_color = memvis::get_pixel(x as usize, y as usize);

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
fn draw_tooltip(x: i16, y: i16, start_addr: usize, end_addr: usize, region: &str, allocated: bool) {
    if x < 0 || y < 0 {
        return;
    }
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

    // Line 1: Address range of entire contiguous region
    let text_x = bx + TOOLTIP_PADDING + 2;
    let line1_y = by + TOOLTIP_PADDING + 1;

    // Draw "0xXXXXXX-0xXXXXXX"
    font::draw_hex_bg(text_x, line1_y, start_addr, 6, colors::WHITE, colors::DARK_GRAY);
    font::draw_char_bg(text_x + 64, line1_y, '-', colors::WHITE, colors::DARK_GRAY);
    font::draw_hex_bg(text_x + 72, line1_y, end_addr, 6, colors::WHITE, colors::DARK_GRAY);

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

    // Get cursor position
    let (x, y) = mouse::position();

    // Get memory info BEFORE redrawing (so we read the correct pixel color)
    let addr = pixel_to_addr(x, y);
    let (region, allocated) = get_region_info(addr, x, y);
    let (start_addr, end_addr) = find_contiguous_region(x, y);

    // Redraw the entire memory visualization
    memvis::redraw();

    // Draw cursor on top
    draw_cursor_sprite(x, y);

    // Draw tooltip on top
    let (tooltip_x, tooltip_y) = calculate_tooltip_pos(x, y);
    draw_tooltip(tooltip_x, tooltip_y, start_addr, end_addr, region, allocated);

    mouse::clear_dirty();
}

/// Initialize cursor (draw initial cursor)
pub fn init() {
    if !vga::is_enabled() {
        return;
    }

    let (x, y) = mouse::position();

    // Get memory info
    let addr = pixel_to_addr(x, y);
    let (region, allocated) = get_region_info(addr, x, y);
    let (start_addr, end_addr) = find_contiguous_region(x, y);

    // Draw cursor
    draw_cursor_sprite(x, y);

    // Draw tooltip
    let (tooltip_x, tooltip_y) = calculate_tooltip_pos(x, y);
    draw_tooltip(tooltip_x, tooltip_y, start_addr, end_addr, region, allocated);

    mouse::clear_dirty();
}
