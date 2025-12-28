//! Pre-allocated packet buffer pool
//!
//! Provides interrupt-safe packet buffers for the NIC driver.
//!
//! ## Design
//!
//! Since the kernel allocator is not interrupt-safe, we pre-allocate
//! all packet buffers at init time. The IRQ handler and network task
//! communicate via atomic indices into a fixed ring buffer.
//!
//! ## Memory Layout
//!
//! ```text
//! RX Ring: [PacketBuffer; 16] = 16 * 1536 = ~24 KB
//! TX Ring: [PacketBuffer; 8]  =  8 * 1536 = ~12 KB
//! Total: ~37 KB (statically allocated)
//! ```

use core::sync::atomic::{AtomicU8, AtomicUsize, Ordering};

/// Maximum Ethernet frame size (MTU 1500 + Ethernet header + some padding)
pub const PACKET_SIZE: usize = 1536;

/// Number of receive buffers
pub const RX_BUFFER_COUNT: usize = 16;

/// Number of transmit buffers
pub const TX_BUFFER_COUNT: usize = 8;

/// Buffer state flags
pub const BUFFER_EMPTY: u8 = 0;
pub const BUFFER_FULL: u8 = 1;
pub const BUFFER_IN_USE: u8 = 2;

/// A single packet buffer with metadata
#[repr(C)]
pub struct PacketBuffer {
    /// Packet data
    pub data: [u8; PACKET_SIZE],
    /// Actual data length
    pub len: u16,
    /// Buffer state (atomic for ISR safety)
    pub flags: AtomicU8,
}

impl PacketBuffer {
    /// Create a new empty packet buffer
    const fn new() -> Self {
        PacketBuffer {
            data: [0; PACKET_SIZE],
            len: 0,
            flags: AtomicU8::new(BUFFER_EMPTY),
        }
    }
}

/// Packet pool for RX/TX operations
pub struct PacketPool {
    /// Receive buffers (ISR writes, task reads)
    rx_buffers: [PacketBuffer; RX_BUFFER_COUNT],
    /// Transmit buffers (task writes, ISR reads)
    tx_buffers: [PacketBuffer; TX_BUFFER_COUNT],

    /// Next RX buffer to fill (written by ISR)
    rx_head: AtomicUsize,
    /// Next RX buffer to process (written by task)
    rx_tail: AtomicUsize,

    /// Next TX buffer to send (written by task)
    tx_head: AtomicUsize,
    /// Next TX buffer available (written by ISR after send complete)
    tx_tail: AtomicUsize,

    /// Statistics
    rx_count: AtomicUsize,
    tx_count: AtomicUsize,
    rx_dropped: AtomicUsize,
}

impl PacketPool {
    /// Create a new packet pool (const for static allocation)
    const fn new() -> Self {
        // Rust doesn't have const array initialization with non-Copy types easily,
        // so we use a macro-like repetition
        PacketPool {
            rx_buffers: [
                PacketBuffer::new(), PacketBuffer::new(), PacketBuffer::new(), PacketBuffer::new(),
                PacketBuffer::new(), PacketBuffer::new(), PacketBuffer::new(), PacketBuffer::new(),
                PacketBuffer::new(), PacketBuffer::new(), PacketBuffer::new(), PacketBuffer::new(),
                PacketBuffer::new(), PacketBuffer::new(), PacketBuffer::new(), PacketBuffer::new(),
            ],
            tx_buffers: [
                PacketBuffer::new(), PacketBuffer::new(), PacketBuffer::new(), PacketBuffer::new(),
                PacketBuffer::new(), PacketBuffer::new(), PacketBuffer::new(), PacketBuffer::new(),
            ],
            rx_head: AtomicUsize::new(0),
            rx_tail: AtomicUsize::new(0),
            tx_head: AtomicUsize::new(0),
            tx_tail: AtomicUsize::new(0),
            rx_count: AtomicUsize::new(0),
            tx_count: AtomicUsize::new(0),
            rx_dropped: AtomicUsize::new(0),
        }
    }
}

/// Global packet pool (statically allocated)
static mut PACKET_POOL: PacketPool = PacketPool::new();

/// Initialize the packet pool
///
/// Must be called before enabling NIC interrupts.
pub fn init() {
    // Pool is statically initialized, but we reset counters here
    unsafe {
        PACKET_POOL.rx_head.store(0, Ordering::SeqCst);
        PACKET_POOL.rx_tail.store(0, Ordering::SeqCst);
        PACKET_POOL.tx_head.store(0, Ordering::SeqCst);
        PACKET_POOL.tx_tail.store(0, Ordering::SeqCst);
        PACKET_POOL.rx_count.store(0, Ordering::SeqCst);
        PACKET_POOL.tx_count.store(0, Ordering::SeqCst);
        PACKET_POOL.rx_dropped.store(0, Ordering::SeqCst);
    }

    crate::println!("  Packet pool: {} RX + {} TX buffers ({} bytes each)",
        RX_BUFFER_COUNT, TX_BUFFER_COUNT, PACKET_SIZE);
}

// ============================================================================
// RX Buffer Operations (ISR writes head, task reads tail)
// ============================================================================

/// Get a buffer to receive a packet into (called from ISR)
///
/// Returns a mutable slice to write packet data into, or None if full.
/// After writing, call `rx_buffer_ready()` to mark it available.
pub fn get_rx_buffer_for_write() -> Option<&'static mut [u8]> {
    unsafe {
        let head = PACKET_POOL.rx_head.load(Ordering::Acquire);
        let tail = PACKET_POOL.rx_tail.load(Ordering::Acquire);

        // Check if buffer is full
        let next_head = (head + 1) % RX_BUFFER_COUNT;
        if next_head == tail {
            PACKET_POOL.rx_dropped.fetch_add(1, Ordering::Relaxed);
            return None;
        }

        // Return the buffer at head
        Some(&mut PACKET_POOL.rx_buffers[head].data)
    }
}

/// Mark the current RX buffer as ready for processing (called from ISR)
///
/// Must be called after writing packet data via `get_rx_buffer_for_write()`.
pub fn rx_buffer_ready(len: usize) {
    unsafe {
        let head = PACKET_POOL.rx_head.load(Ordering::Acquire);

        // Set length and mark as full
        PACKET_POOL.rx_buffers[head].len = len as u16;
        PACKET_POOL.rx_buffers[head].flags.store(BUFFER_FULL, Ordering::Release);

        // Advance head
        let next_head = (head + 1) % RX_BUFFER_COUNT;
        PACKET_POOL.rx_head.store(next_head, Ordering::Release);
        PACKET_POOL.rx_count.fetch_add(1, Ordering::Relaxed);
    }
}

/// Get the next received packet for processing (called from network task)
///
/// Returns (data slice, length) or None if no packets available.
pub fn get_rx_packet() -> Option<(&'static [u8], usize)> {
    unsafe {
        let head = PACKET_POOL.rx_head.load(Ordering::Acquire);
        let tail = PACKET_POOL.rx_tail.load(Ordering::Acquire);

        // Check if buffer is empty
        if head == tail {
            return None;
        }

        // Check if buffer is ready
        if PACKET_POOL.rx_buffers[tail].flags.load(Ordering::Acquire) != BUFFER_FULL {
            return None;
        }

        let len = PACKET_POOL.rx_buffers[tail].len as usize;
        let data = &PACKET_POOL.rx_buffers[tail].data[..len];

        Some((data, len))
    }
}

/// Release the current RX buffer after processing (called from network task)
pub fn release_rx_buffer() {
    unsafe {
        let tail = PACKET_POOL.rx_tail.load(Ordering::Acquire);

        // Mark as empty
        PACKET_POOL.rx_buffers[tail].flags.store(BUFFER_EMPTY, Ordering::Release);

        // Advance tail
        let next_tail = (tail + 1) % RX_BUFFER_COUNT;
        PACKET_POOL.rx_tail.store(next_tail, Ordering::Release);
    }
}

// ============================================================================
// TX Buffer Operations (task writes, driver sends)
// ============================================================================

/// Get a buffer to prepare a packet for transmission
///
/// Returns a mutable slice to write packet data into, or None if full.
pub fn get_tx_buffer() -> Option<&'static mut [u8]> {
    unsafe {
        let head = PACKET_POOL.tx_head.load(Ordering::Acquire);
        let tail = PACKET_POOL.tx_tail.load(Ordering::Acquire);

        // Check if all buffers are in use
        let next_head = (head + 1) % TX_BUFFER_COUNT;
        if next_head == tail {
            return None;
        }

        Some(&mut PACKET_POOL.tx_buffers[head].data)
    }
}

/// Queue a packet for transmission
///
/// Must be called after writing packet data via `get_tx_buffer()`.
/// Returns the buffer index for the driver.
pub fn tx_buffer_ready(len: usize) -> usize {
    unsafe {
        let head = PACKET_POOL.tx_head.load(Ordering::Acquire);

        // Set length and mark as ready to send
        PACKET_POOL.tx_buffers[head].len = len as u16;
        PACKET_POOL.tx_buffers[head].flags.store(BUFFER_FULL, Ordering::Release);

        // Advance head
        let next_head = (head + 1) % TX_BUFFER_COUNT;
        PACKET_POOL.tx_head.store(next_head, Ordering::Release);
        PACKET_POOL.tx_count.fetch_add(1, Ordering::Relaxed);

        head
    }
}

/// Get the next packet to transmit (called by driver)
///
/// Returns (data slice, length, buffer index) or None if nothing to send.
pub fn get_tx_packet() -> Option<(&'static [u8], usize, usize)> {
    unsafe {
        let head = PACKET_POOL.tx_head.load(Ordering::Acquire);
        let tail = PACKET_POOL.tx_tail.load(Ordering::Acquire);

        // Check if buffer is empty
        if head == tail {
            return None;
        }

        // Check if buffer is ready
        if PACKET_POOL.tx_buffers[tail].flags.load(Ordering::Acquire) != BUFFER_FULL {
            return None;
        }

        let len = PACKET_POOL.tx_buffers[tail].len as usize;
        let data = &PACKET_POOL.tx_buffers[tail].data[..len];

        Some((data, len, tail))
    }
}

/// Mark a TX buffer as sent (called by driver after transmission)
pub fn tx_complete() {
    unsafe {
        let tail = PACKET_POOL.tx_tail.load(Ordering::Acquire);

        // Mark as empty
        PACKET_POOL.tx_buffers[tail].flags.store(BUFFER_EMPTY, Ordering::Release);

        // Advance tail
        let next_tail = (tail + 1) % TX_BUFFER_COUNT;
        PACKET_POOL.tx_tail.store(next_tail, Ordering::Release);
    }
}

// ============================================================================
// Statistics
// ============================================================================

/// Get packet statistics
pub fn stats() -> (usize, usize, usize) {
    unsafe {
        (
            PACKET_POOL.rx_count.load(Ordering::Relaxed),
            PACKET_POOL.tx_count.load(Ordering::Relaxed),
            PACKET_POOL.rx_dropped.load(Ordering::Relaxed),
        )
    }
}

/// Check if there are packets pending to receive
pub fn has_rx_pending() -> bool {
    unsafe {
        let head = PACKET_POOL.rx_head.load(Ordering::Acquire);
        let tail = PACKET_POOL.rx_tail.load(Ordering::Acquire);
        head != tail
    }
}

/// Check if there are packets pending to transmit
pub fn has_tx_pending() -> bool {
    unsafe {
        let head = PACKET_POOL.tx_head.load(Ordering::Acquire);
        let tail = PACKET_POOL.tx_tail.load(Ordering::Acquire);
        head != tail
    }
}
