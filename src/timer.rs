//! PIT (Programmable Interval Timer) driver
//!
//! Configures the 8253/8254 PIT for time tracking at ~100 Hz.
//! Uses polling to count timer ticks - no interrupts needed.
//!
//! # Accuracy
//! Polling-based timing can miss ticks if poll() isn't called frequently.
//! This implementation accumulates raw PIT counts for better accuracy,
//! but can still drift if poll() is called less than once per tick period.

use core::sync::atomic::{AtomicU64, AtomicU16, Ordering};

// PIT I/O ports
const PIT_CHANNEL0: u16 = 0x40;
const PIT_COMMAND: u16 = 0x43;

// PIT configuration
const PIT_FREQUENCY: u32 = 1_193_182; // Base frequency in Hz
const TARGET_HZ: u32 = 100; // 100 Hz = 10ms per tick
const DIVISOR: u16 = (PIT_FREQUENCY / TARGET_HZ) as u16; // ~11932

// Global state
static TICK_COUNT: AtomicU64 = AtomicU64::new(0);
static LAST_COUNTER: AtomicU16 = AtomicU16::new(0);
// Accumulated counts that haven't yet formed a complete tick
static ACCUMULATED_COUNTS: AtomicU64 = AtomicU64::new(0);

/// Port I/O: Read byte from port
#[inline]
unsafe fn inb(port: u16) -> u8 {
    let value: u8;
    core::arch::asm!(
        "in al, dx",
        out("al") value,
        in("dx") port,
        options(nomem, nostack, preserves_flags)
    );
    value
}

/// Port I/O: Write byte to port
#[inline]
unsafe fn outb(port: u16, value: u8) {
    core::arch::asm!(
        "out dx, al",
        in("dx") port,
        in("al") value,
        options(nomem, nostack, preserves_flags)
    );
}

/// Read the current PIT counter value
fn read_counter() -> u16 {
    unsafe {
        // Latch count for channel 0 (command 0x00)
        outb(PIT_COMMAND, 0x00);
        let low = inb(PIT_CHANNEL0);
        let high = inb(PIT_CHANNEL0);
        ((high as u16) << 8) | (low as u16)
    }
}

/// Initialize the PIT timer
pub fn init() {
    unsafe {
        // Configure PIT channel 0:
        // 0x34 = 00110100
        //   bits 7-6: 00 = channel 0
        //   bits 5-4: 11 = access mode lobyte/hibyte
        //   bits 3-1: 010 = mode 2 (rate generator)
        //   bit 0: 0 = binary mode
        outb(PIT_COMMAND, 0x34);

        // Set divisor (low byte first, then high byte)
        outb(PIT_CHANNEL0, (DIVISOR & 0xFF) as u8);
        outb(PIT_CHANNEL0, ((DIVISOR >> 8) & 0xFF) as u8);
    }

    // Initialize counter tracking
    LAST_COUNTER.store(read_counter(), Ordering::Relaxed);
    ACCUMULATED_COUNTS.store(0, Ordering::Relaxed);
}

/// Poll the timer and update tick count
///
/// Call this periodically in the scheduler loop.
/// The PIT counter counts DOWN from DIVISOR to 0, then wraps.
///
/// This implementation accumulates raw counts for sub-tick accuracy.
/// However, if poll() is not called at least once per tick period (~10ms),
/// ticks will be lost because we cannot detect multiple wraparounds.
pub fn poll() {
    let current = read_counter();
    let last = LAST_COUNTER.swap(current, Ordering::Relaxed);

    // Calculate elapsed counts since last poll.
    // Counter counts DOWN, so elapsed = last - current (normally).
    // If current > last, the counter wrapped around.
    let elapsed = if current <= last {
        // Normal case: counter decreased
        (last - current) as u64
    } else {
        // Wrap-around: counter went from low value back to high value
        // Elapsed = counts from last down to 0, plus counts from DIVISOR down to current
        (last as u64) + (DIVISOR as u64 - current as u64)
    };

    // Accumulate the elapsed counts
    let total = ACCUMULATED_COUNTS.fetch_add(elapsed, Ordering::Relaxed) + elapsed;

    // Convert accumulated counts to ticks
    let new_ticks = total / (DIVISOR as u64);
    if new_ticks > 0 {
        TICK_COUNT.fetch_add(new_ticks, Ordering::Relaxed);
        // Keep only the remainder (sub-tick counts)
        let remainder = total % (DIVISOR as u64);
        // Note: This isn't perfectly atomic, but close enough for single-threaded use
        ACCUMULATED_COUNTS.store(remainder, Ordering::Relaxed);
    }
}

/// Get the current tick count since boot
pub fn ticks() -> u64 {
    TICK_COUNT.load(Ordering::Relaxed)
}

/// Get ticks per second (100)
pub const fn ticks_per_second() -> u64 {
    TARGET_HZ as u64
}

/// Convert milliseconds to ticks
pub fn ms_to_ticks(ms: u64) -> u64 {
    // 100 ticks/sec = 1 tick per 10ms
    // ms * 100 / 1000 = ms / 10
    (ms + 9) / 10 // Round up
}

/// Convert ticks to milliseconds
pub fn ticks_to_ms(t: u64) -> u64 {
    t * 10
}
