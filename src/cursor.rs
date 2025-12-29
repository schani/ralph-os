//! Mouse Cursor and Tooltip
//!
//! Handles cursor sprite rendering and memory info tooltip display.
//! Queries actual allocator data structures to show real allocation boundaries.

use crate::{vga, font, mouse, memvis, allocator, program_alloc, executable, gilbert};
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
const TOOLTIP_WIDTH: usize = 184;   // 23 chars * 8 pixels (for 7-digit hex addresses)
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

/// Convert pixel position to memory address using Gilbert curve
fn pixel_to_addr(x: i16, y: i16) -> usize {
    if x < 0 || y < 0 {
        return VIS_BASE;
    }

    let x = x as usize;
    let y = y as usize;

    // Handle cursor in unused bottom 8 rows (y >= 192)
    if x >= gilbert::WIDTH || y >= gilbert::HEIGHT {
        return PROGRAM_END; // Beyond visualized memory
    }

    // Use Gilbert curve to convert (x, y) to curve index
    let d = gilbert::xy_to_d(x, y);
    VIS_BASE + (d << 8) // * 256 bytes per pixel
}

/// Memory region info returned by find_memory_region
struct MemoryRegionInfo {
    start: usize,
    end: usize,
    region_name: &'static str,
    is_allocated: bool,
}

/// Find the memory region that contains the given address.
/// Queries actual allocator data structures, not pixels.
fn find_memory_region(addr: usize) -> MemoryRegionInfo {
    // Kernel region: 0x100000 - 0x200000 (always "allocated")
    if addr < KERNEL_END {
        return MemoryRegionInfo {
            start: VIS_BASE,
            end: KERNEL_END,
            region_name: "Kernel",
            is_allocated: true,
        };
    }

    // Heap region: 0x200000 - 0x400000
    if addr < HEAP_END {
        // Check if it's an allocation
        if let Some((start, end)) = allocator::find_allocation(addr) {
            return MemoryRegionInfo {
                start,
                end,
                region_name: "Heap",
                is_allocated: true,
            };
        }
        // Check if it's a free region
        if let Some((start, end)) = allocator::find_free_region(addr) {
            return MemoryRegionInfo {
                start,
                end,
                region_name: "Heap",
                is_allocated: false,
            };
        }
        // Fallback (shouldn't happen)
        return MemoryRegionInfo {
            start: KERNEL_END,
            end: HEAP_END,
            region_name: "Heap",
            is_allocated: false,
        };
    }

    // Program region: 0x400000 - 0x1000000
    if addr < PROGRAM_END {
        // First check if it's a known program (has a name)
        if let Some((start, end, name)) = executable::find_program_by_addr(addr) {
            return MemoryRegionInfo {
                start,
                end,
                region_name: name,
                is_allocated: true,
            };
        }
        // Check if it's an allocation (stack or heap block without program name)
        if let Some((start, end)) = program_alloc::find_allocation(addr) {
            return MemoryRegionInfo {
                start,
                end,
                region_name: "Stack",
                is_allocated: true,
            };
        }
        // Check if it's a free region
        if let Some((start, end)) = program_alloc::find_free_region(addr) {
            return MemoryRegionInfo {
                start,
                end,
                region_name: "Program",
                is_allocated: false,
            };
        }
        // Fallback (shouldn't happen)
        return MemoryRegionInfo {
            start: HEAP_END,
            end: PROGRAM_END,
            region_name: "Program",
            is_allocated: false,
        };
    }

    // Beyond visualized region
    MemoryRegionInfo {
        start: addr,
        end: addr + BYTES_PER_PIXEL,
        region_name: "Unknown",
        is_allocated: false,
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

    // Draw "0xXXXXXXX-0xXXXXXXX" (7 hex digits to show up to 0x1000000)
    font::draw_hex_bg(text_x, line1_y, start_addr, 7, colors::WHITE, colors::DARK_GRAY);
    font::draw_char_bg(text_x + 72, line1_y, '-', colors::WHITE, colors::DARK_GRAY);
    font::draw_hex_bg(text_x + 80, line1_y, end_addr, 7, colors::WHITE, colors::DARK_GRAY);

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

    // Get memory info by querying the actual allocators
    let addr = pixel_to_addr(x, y);
    let region_info = find_memory_region(addr);

    // Redraw the entire memory visualization
    memvis::redraw();

    // Draw cursor on top
    draw_cursor_sprite(x, y);

    // Draw tooltip on top
    let (tooltip_x, tooltip_y) = calculate_tooltip_pos(x, y);
    draw_tooltip(
        tooltip_x,
        tooltip_y,
        region_info.start,
        region_info.end,
        region_info.region_name,
        region_info.is_allocated,
    );

    mouse::clear_dirty();
}

/// Initialize cursor (draw initial cursor)
pub fn init() {
    if !vga::is_enabled() {
        return;
    }

    let (x, y) = mouse::position();

    // Get memory info by querying the actual allocators
    let addr = pixel_to_addr(x, y);
    let region_info = find_memory_region(addr);

    // Draw cursor
    draw_cursor_sprite(x, y);

    // Draw tooltip
    let (tooltip_x, tooltip_y) = calculate_tooltip_pos(x, y);
    draw_tooltip(
        tooltip_x,
        tooltip_y,
        region_info.start,
        region_info.end,
        region_info.region_name,
        region_info.is_allocated,
    );

    mouse::clear_dirty();
}
