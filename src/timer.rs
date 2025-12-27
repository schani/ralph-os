//! PIT (Programmable Interval Timer) driver
//!
//! Configures the 8253/8254 PIT for time tracking at ~100 Hz.
//! Uses polling to count timer ticks - no interrupts needed.

use core::sync::atomic::{AtomicU64, Ordering};

// PIT I/O ports
const PIT_CHANNEL0: u16 = 0x40;
const PIT_COMMAND: u16 = 0x43;

// PIT configuration
const PIT_FREQUENCY: u32 = 1_193_182; // Base frequency in Hz
const TARGET_HZ: u32 = 100; // 100 Hz = 10ms per tick
const DIVISOR: u16 = (PIT_FREQUENCY / TARGET_HZ) as u16; // ~11932

// Global tick counter
static TICK_COUNT: AtomicU64 = AtomicU64::new(0);
static LAST_COUNT: AtomicU64 = AtomicU64::new(0);

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

    // Initialize last count
    LAST_COUNT.store(read_counter() as u64, Ordering::Relaxed);
}

/// Poll the timer and update tick count
///
/// Call this periodically in the scheduler loop.
/// The PIT counter counts DOWN from DIVISOR to 0, then wraps.
pub fn poll() {
    let current = read_counter() as u64;
    let last = LAST_COUNT.load(Ordering::Relaxed);

    // Counter counts DOWN, so wrap-around is when current > last
    // (because it went from a low value back to a high value)
    if current > last {
        // Wrapped around - increment tick count
        TICK_COUNT.fetch_add(1, Ordering::Relaxed);
    }

    LAST_COUNT.store(current, Ordering::Relaxed);
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
