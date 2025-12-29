//! Gilbert Curve for 320x192 Memory Visualization
//!
//! Generalized Hilbert curve that works for arbitrary rectangles.
//! Provides perfect continuity for the memory visualization.
//!
//! The Gilbert curve (E.N. Gilbert, 1958) extends Hilbert curves to
//! non-square, non-power-of-2 dimensions while maintaining:
//! - Perfect continuity (adjacent indices map to adjacent pixels)
//! - Good locality (nearby indices stay nearby on screen)
//! - Full coverage (every pixel visited exactly once)

use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

/// Screen dimensions for the visualization
pub const WIDTH: usize = 320;
pub const HEIGHT: usize = 192;
pub const TOTAL_PIXELS: usize = WIDTH * HEIGHT; // 61440

/// Lookup tables for O(1) coordinate conversion
/// D_TO_XY[d] = (x, y) for curve index d
static mut D_TO_XY: [(u16, u16); TOTAL_PIXELS] = [(0, 0); TOTAL_PIXELS];

/// XY_TO_D[y][x] = d for screen position (x, y)
static mut XY_TO_D: [[u16; WIDTH]; HEIGHT] = [[0; WIDTH]; HEIGHT];

/// Initialization flag
static INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Initialize the Gilbert curve lookup tables
///
/// Must be called after the heap allocator is initialized (uses Vec temporarily).
/// Safe to call multiple times; subsequent calls are no-ops.
pub fn init() {
    if INITIALIZED.swap(true, Ordering::SeqCst) {
        return; // Already initialized
    }

    // Generate the curve using the recursive Gilbert algorithm
    let mut path = Vec::with_capacity(TOTAL_PIXELS);
    gilbert2d(0, 0, WIDTH as i32, 0, 0, HEIGHT as i32, &mut path);

    // Build lookup tables
    unsafe {
        for (d, &(x, y)) in path.iter().enumerate() {
            D_TO_XY[d] = (x as u16, y as u16);
            XY_TO_D[y as usize][x as usize] = d as u16;
        }
    }

    crate::println!(
        "[gilbert] Initialized {}x{} curve ({} pixels)",
        WIDTH,
        HEIGHT,
        TOTAL_PIXELS
    );
}

/// Check if the Gilbert curve tables are initialized
#[inline]
pub fn is_initialized() -> bool {
    INITIALIZED.load(Ordering::Acquire)
}

/// Convert curve index to screen coordinates
///
/// Returns (x, y) for the given index d in the Gilbert curve.
/// Panics in debug mode if d >= TOTAL_PIXELS.
#[inline]
pub fn d_to_xy(d: usize) -> (usize, usize) {
    debug_assert!(d < TOTAL_PIXELS, "Gilbert index out of bounds: {}", d);
    unsafe {
        let (x, y) = D_TO_XY[d.min(TOTAL_PIXELS - 1)];
        (x as usize, y as usize)
    }
}

/// Convert screen coordinates to curve index
///
/// Returns the Gilbert curve index for position (x, y).
/// Returns TOTAL_PIXELS if coordinates are out of bounds.
#[inline]
pub fn xy_to_d(x: usize, y: usize) -> usize {
    if x >= WIDTH || y >= HEIGHT {
        return TOTAL_PIXELS; // Out of bounds sentinel
    }
    unsafe { XY_TO_D[y][x] as usize }
}

/// Gilbert curve generation algorithm
///
/// Recursively generates a space-filling curve for a rectangle defined by:
/// - (x, y): starting point
/// - (ax, ay): "width" axis vector
/// - (bx, by): "height" axis vector
///
/// The curve visits every point in the rectangle exactly once, with each
/// consecutive pair of points being adjacent (Manhattan distance = 1).
fn gilbert2d(x: i32, y: i32, ax: i32, ay: i32, bx: i32, by: i32, path: &mut Vec<(i32, i32)>) {
    let w = (ax + ay).abs();
    let h = (bx + by).abs();

    // Direction signs for each axis
    let dax = ax.signum();
    let day = ay.signum();
    let dbx = bx.signum();
    let dby = by.signum();

    // Base case: single row
    if h == 1 {
        let mut cx = x;
        let mut cy = y;
        for _ in 0..w {
            path.push((cx, cy));
            cx += dax;
            cy += day;
        }
        return;
    }

    // Base case: single column
    if w == 1 {
        let mut cx = x;
        let mut cy = y;
        for _ in 0..h {
            path.push((cx, cy));
            cx += dbx;
            cy += dby;
        }
        return;
    }

    // Split the rectangle and recurse
    let mut ax2 = ax / 2;
    let mut ay2 = ay / 2;
    let mut bx2 = bx / 2;
    let mut by2 = by / 2;

    let w2 = (ax2 + ay2).abs();
    let h2 = (bx2 + by2).abs();

    if 2 * w > 3 * h {
        // Wide rectangle: split along the width axis
        if (w2 & 1 != 0) && (w > 2) {
            // Adjust for odd width
            ax2 += dax;
            ay2 += day;
        }
        // First half
        gilbert2d(x, y, ax2, ay2, bx, by, path);
        // Second half
        gilbert2d(x + ax2, y + ay2, ax - ax2, ay - ay2, bx, by, path);
    } else {
        // Tall or square rectangle: split along the height axis
        if (h2 & 1 != 0) && (h > 2) {
            // Adjust for odd height
            bx2 += dbx;
            by2 += dby;
        }
        // First quadrant
        gilbert2d(x, y, bx2, by2, ax2, ay2, path);
        // Second quadrant
        gilbert2d(x + bx2, y + by2, ax, ay, bx - bx2, by - by2, path);
        // Third quadrant (mirrored)
        gilbert2d(
            x + (ax - dax) + (bx2 - dbx),
            y + (ay - day) + (by2 - dby),
            -bx2,
            -by2,
            -(ax - ax2),
            -(ay - ay2),
            path,
        );
    }
}
