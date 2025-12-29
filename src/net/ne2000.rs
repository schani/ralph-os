//! NE2000 NIC driver
//!
//! The NE2000 is a simple ISA/PCI network card that's well-supported by QEMU.
//! This driver implements basic send/receive functionality.
//!
//! ## QEMU Usage
//!
//! ```bash
//! qemu-system-x86_64 ... -netdev user,id=net0 -device ne2k_isa,netdev=net0,irq=10,iobase=0x300
//! ```
//!
//! ## Register Layout
//!
//! The NE2000 uses a paged register model. Page 0 is for normal operation,
//! Page 1 for physical address and multicast filters.

use crate::io::{inb, outb, inw, outw};
use crate::println;
use super::packet;

// ============================================================================
// NE2000 Register Definitions
// ============================================================================

/// Default I/O base for NE2000 ISA
/// Using standard ISA address 0x300 with QEMU's ne2k_isa device
pub const NE2000_IOBASE: u16 = 0x300;

/// NE2000 register offsets (active in all pages)
const CR: u16 = 0x00;       // Command Register

/// Page 0 registers (active when PS1:PS0 = 00)
const CLDA0: u16 = 0x01;    // Current Local DMA Address 0 (read)
const PSTART: u16 = 0x01;   // Page Start (write)
const CLDA1: u16 = 0x02;    // Current Local DMA Address 1 (read)
const PSTOP: u16 = 0x02;    // Page Stop (write)
const BNRY: u16 = 0x03;     // Boundary Pointer
const TSR: u16 = 0x04;      // Transmit Status (read)
const TPSR: u16 = 0x04;     // Transmit Page Start (write)
const NCR: u16 = 0x05;      // Number of Collisions (read)
const TBCR0: u16 = 0x05;    // Transmit Byte Count 0 (write)
const FIFO: u16 = 0x06;     // FIFO (read)
const TBCR1: u16 = 0x06;    // Transmit Byte Count 1 (write)
const ISR: u16 = 0x07;      // Interrupt Status Register
const CRDA0: u16 = 0x08;    // Current Remote DMA Address 0 (read)
const RSAR0: u16 = 0x08;    // Remote Start Address 0 (write)
const CRDA1: u16 = 0x09;    // Current Remote DMA Address 1 (read)
const RSAR1: u16 = 0x09;    // Remote Start Address 1 (write)
const RBCR0: u16 = 0x0A;    // Remote Byte Count 0 (write)
const RBCR1: u16 = 0x0B;    // Remote Byte Count 1 (write)
const RSR: u16 = 0x0C;      // Receive Status (read)
const RCR: u16 = 0x0C;      // Receive Configuration (write)
const CNTR0: u16 = 0x0D;    // Tally Counter 0 (read)
const TCR: u16 = 0x0D;      // Transmit Configuration (write)
const CNTR1: u16 = 0x0E;    // Tally Counter 1 (read)
const DCR: u16 = 0x0E;      // Data Configuration (write)
const CNTR2: u16 = 0x0F;    // Tally Counter 2 (read)
const IMR: u16 = 0x0F;      // Interrupt Mask (write)

/// Page 1 registers (active when PS1:PS0 = 01)
const PAR0: u16 = 0x01;     // Physical Address 0-5
const CURR: u16 = 0x07;     // Current Page

/// Data port for remote DMA
const DATA: u16 = 0x10;

/// Reset port
const RESET: u16 = 0x1F;

// Command Register bits
const CR_STP: u8 = 0x01;    // Stop
const CR_STA: u8 = 0x02;    // Start
const CR_TXP: u8 = 0x04;    // Transmit Packet
const CR_RD0: u8 = 0x08;    // Remote DMA Command bit 0
const CR_RD1: u8 = 0x10;    // Remote DMA Command bit 1
const CR_RD2: u8 = 0x20;    // Remote DMA Command bit 2 (abort)
const CR_PS0: u8 = 0x40;    // Page Select bit 0
const CR_PS1: u8 = 0x80;    // Page Select bit 1

// Remote DMA commands (RD2:RD1:RD0)
const CR_DMA_NONE: u8 = CR_RD2;           // 100 = Abort/Complete
const CR_DMA_READ: u8 = CR_RD0;           // 001 = Remote Read
const CR_DMA_WRITE: u8 = CR_RD1;          // 010 = Remote Write
const CR_DMA_SEND: u8 = CR_RD0 | CR_RD1;  // 011 = Send Packet

// Interrupt Status Register bits
const ISR_PRX: u8 = 0x01;   // Packet Received
const ISR_PTX: u8 = 0x02;   // Packet Transmitted
const ISR_RXE: u8 = 0x04;   // Receive Error
const ISR_TXE: u8 = 0x08;   // Transmit Error
const ISR_OVW: u8 = 0x10;   // Overwrite Warning
const ISR_CNT: u8 = 0x20;   // Counter Overflow
const ISR_RDC: u8 = 0x40;   // Remote DMA Complete
const ISR_RST: u8 = 0x80;   // Reset Status

// Receive Configuration Register bits
const RCR_SEP: u8 = 0x01;   // Save Errored Packets
const RCR_AR: u8 = 0x02;    // Accept Runt Packets
const RCR_AB: u8 = 0x04;    // Accept Broadcast
const RCR_AM: u8 = 0x08;    // Accept Multicast
const RCR_PRO: u8 = 0x10;   // Promiscuous Physical
const RCR_MON: u8 = 0x20;   // Monitor Mode

// Transmit Configuration Register bits
const TCR_CRC: u8 = 0x01;   // Inhibit CRC
const TCR_LB0: u8 = 0x02;   // Loopback bit 0
const TCR_LB1: u8 = 0x04;   // Loopback bit 1
const TCR_ATD: u8 = 0x08;   // Auto Transmit Disable
const TCR_OFST: u8 = 0x10;  // Collision Offset Enable

// Data Configuration Register bits
const DCR_WTS: u8 = 0x01;   // Word Transfer Select (1=16-bit)
const DCR_BOS: u8 = 0x02;   // Byte Order Select
const DCR_LAS: u8 = 0x04;   // Long Address Select
const DCR_LS: u8 = 0x08;    // Loopback Select
const DCR_AR: u8 = 0x10;    // Auto-Initialize Remote
const DCR_FT0: u8 = 0x20;   // FIFO Threshold bit 0
const DCR_FT1: u8 = 0x40;   // FIFO Threshold bit 1

// NE2000 memory layout (16KB on-chip RAM)
const MEM_START: u8 = 0x40;     // First page of RAM (16KB total)
const MEM_STOP: u8 = 0x80;      // Last page + 1
const TX_START: u8 = 0x40;      // TX buffer start page
const TX_PAGES: u8 = 6;         // 6 pages = 1536 bytes for TX
const RX_START: u8 = 0x46;      // RX ring start (after TX buffer)
const RX_STOP: u8 = 0x80;       // RX ring end

// ============================================================================
// NE2000 Driver State
// ============================================================================

/// NE2000 driver state
pub struct Ne2000 {
    /// I/O base address
    iobase: u16,
    /// MAC address
    mac: [u8; 6],
    /// Next expected receive page
    next_pkt: u8,
    /// Initialized flag
    initialized: bool,
}

/// Global driver instance
static mut NE2000: Ne2000 = Ne2000 {
    iobase: NE2000_IOBASE,
    mac: [0; 6],
    next_pkt: RX_START,
    initialized: false,
};

/// Receive packet header (stored at start of each packet in ring buffer)
#[repr(C, packed)]
struct RxHeader {
    status: u8,     // Receive status
    next: u8,       // Next packet page
    len_lo: u8,     // Length low byte
    len_hi: u8,     // Length high byte
}

// ============================================================================
// Driver Implementation
// ============================================================================

/// Initialize the NE2000 NIC
///
/// Returns true if initialization succeeded.
pub fn init() -> bool {
    unsafe {
        let base = NE2000.iobase;

        // Reset the NIC
        let reset_val = inb(base + RESET);
        outb(base + RESET, reset_val);

        // Wait for reset to complete (poll for RST bit in ISR)
        let mut timeout = 10000;
        while timeout > 0 {
            if inb(base + ISR) & ISR_RST != 0 {
                break;
            }
            timeout -= 1;
        }
        if timeout == 0 {
            println!("  NE2000: Reset timeout");
            return false;
        }

        // Clear interrupt status
        outb(base + ISR, 0xFF);

        // Stop the NIC, abort DMA, select page 0
        outb(base + CR, CR_STP | CR_DMA_NONE);

        // Set data configuration: 16-bit transfers, normal operation
        outb(base + DCR, DCR_WTS | DCR_FT1);

        // Clear remote byte count
        outb(base + RBCR0, 0);
        outb(base + RBCR1, 0);

        // Set receive config: accept broadcast, no errors
        outb(base + RCR, RCR_AB);

        // Set transmit config: normal operation
        outb(base + TCR, 0);

        // Initialize receive buffer ring
        outb(base + PSTART, RX_START);
        outb(base + PSTOP, RX_STOP);
        outb(base + BNRY, RX_START);

        // Switch to page 1 to set CURR and read MAC
        outb(base + CR, CR_STP | CR_DMA_NONE | CR_PS0);

        // Set current page
        outb(base + CURR, RX_START + 1);
        NE2000.next_pkt = RX_START + 1;

        // Read MAC address from PROM (first 6 bytes of on-chip memory)
        // Switch back to page 0 for DMA
        outb(base + CR, CR_STP | CR_DMA_NONE);

        // Set up remote DMA to read PROM at address 0
        outb(base + RSAR0, 0);
        outb(base + RSAR1, 0);
        outb(base + RBCR0, 12);  // Read 12 bytes (MAC is duplicated)
        outb(base + RBCR1, 0);

        // Start remote read
        outb(base + CR, CR_STA | CR_DMA_READ);

        // Read MAC (each byte is sent twice in 16-bit mode)
        for i in 0..6 {
            let word = inw(base + DATA);
            NE2000.mac[i] = word as u8;
        }

        // Abort DMA
        outb(base + CR, CR_STP | CR_DMA_NONE);
        outb(base + ISR, ISR_RDC);

        // Set physical address (page 1)
        outb(base + CR, CR_STP | CR_DMA_NONE | CR_PS0);
        for i in 0..6 {
            outb(base + PAR0 + i as u16, NE2000.mac[i]);
        }

        // Accept all multicast (set all MAR bits)
        for i in 0..8 {
            outb(base + 0x08 + i, 0xFF);
        }

        // Back to page 0
        outb(base + CR, CR_STP | CR_DMA_NONE);

        // Clear all interrupt flags
        outb(base + ISR, 0xFF);

        // Enable interrupts: packet received, transmitted, errors
        outb(base + IMR, ISR_PRX | ISR_PTX | ISR_RXE | ISR_TXE | ISR_OVW);

        // Start the NIC
        outb(base + CR, CR_STA | CR_DMA_NONE);

        NE2000.initialized = true;

        println!("  NE2000: MAC {:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
            NE2000.mac[0], NE2000.mac[1], NE2000.mac[2],
            NE2000.mac[3], NE2000.mac[4], NE2000.mac[5]);

        true
    }
}

/// Get the MAC address
pub fn mac_address() -> [u8; 6] {
    unsafe { NE2000.mac }
}

/// Check if the NIC is initialized
pub fn is_initialized() -> bool {
    unsafe { NE2000.initialized }
}

/// Handle NE2000 interrupt
///
/// Called from the ISR. Reads packets into the packet pool.
/// Returns the number of packets received.
pub fn handle_interrupt() -> usize {
    unsafe {
        let base = NE2000.iobase;
        let mut packets = 0;

        loop {
            // Read and clear interrupt status
            let isr = inb(base + ISR);

            if isr == 0 {
                break;
            }

            // Handle receive
            if isr & ISR_PRX != 0 {
                packets += receive_packets();
                outb(base + ISR, ISR_PRX);
            }

            // Handle transmit complete
            if isr & ISR_PTX != 0 {
                packet::tx_complete();
                outb(base + ISR, ISR_PTX);
            }

            // Handle receive error
            if isr & ISR_RXE != 0 {
                // Just clear for now
                outb(base + ISR, ISR_RXE);
            }

            // Handle transmit error
            if isr & ISR_TXE != 0 {
                outb(base + ISR, ISR_TXE);
            }

            // Handle overwrite warning (ring buffer overflow)
            if isr & ISR_OVW != 0 {
                // Need to reset the NIC - for now just clear
                outb(base + ISR, ISR_OVW);
            }
        }

        packets
    }
}

/// Receive all pending packets from the NIC
fn receive_packets() -> usize {
    unsafe {
        let base = NE2000.iobase;
        let mut count = 0;

        loop {
            // Switch to page 1 to read CURR
            outb(base + CR, CR_STA | CR_DMA_NONE | CR_PS0);
            let curr = inb(base + CURR);
            outb(base + CR, CR_STA | CR_DMA_NONE);

            // If CURR == next_pkt, no more packets
            if curr == NE2000.next_pkt {
                break;
            }

            // Read packet header (4 bytes) via remote DMA using 16-bit reads
            let page = NE2000.next_pkt;
            outb(base + RSAR0, 0);
            outb(base + RSAR1, page);
            outb(base + RBCR0, 4);
            outb(base + RBCR1, 0);
            outb(base + CR, CR_STA | CR_DMA_READ);

            // NE2000 uses 16-bit data transfers
            let word0 = inw(base + DATA);  // status (low) + next (high)
            let word1 = inw(base + DATA);  // length (little endian)

            // Wait for DMA complete
            while inb(base + ISR) & ISR_RDC == 0 {}
            outb(base + ISR, ISR_RDC);

            let status = (word0 & 0xFF) as u8;
            let next = ((word0 >> 8) & 0xFF) as u8;
            let len = word1 as usize;

            // Sanity check length
            if len < 4 || len > 1536 {
                // Bad packet, skip to next
                NE2000.next_pkt = next;
                outb(base + BNRY, if next == RX_START { RX_STOP - 1 } else { next - 1 });
                continue;
            }

            // Get a buffer from the packet pool
            if let Some(buffer) = packet::get_rx_buffer_for_write() {
                // Read packet data (minus 4-byte header)
                let data_len = len - 4;

                outb(base + RSAR0, 4);  // Skip header
                outb(base + RSAR1, page);
                outb(base + RBCR0, (data_len & 0xFF) as u8);
                outb(base + RBCR1, ((data_len >> 8) & 0xFF) as u8);
                outb(base + CR, CR_STA | CR_DMA_READ);

                // Read data (16-bit transfers)
                let words = (data_len + 1) / 2;
                for i in 0..words {
                    let word = inw(base + DATA);
                    let idx = i * 2;
                    if idx < data_len {
                        buffer[idx] = word as u8;
                    }
                    if idx + 1 < data_len {
                        buffer[idx + 1] = (word >> 8) as u8;
                    }
                }

                // Wait for DMA complete
                while inb(base + ISR) & ISR_RDC == 0 {}
                outb(base + ISR, ISR_RDC);

                // Signal buffer ready
                packet::rx_buffer_ready(data_len);
                count += 1;
            }

            // Advance to next packet
            NE2000.next_pkt = next;

            // Update boundary register (one behind next_pkt)
            let bnry = if next == RX_START { RX_STOP - 1 } else { next - 1 };
            outb(base + BNRY, bnry);
        }

        count
    }
}

/// Send a packet
///
/// Returns true if the packet was queued for transmission.
pub fn send(data: &[u8]) -> bool {
    if data.len() > 1500 {
        return false;
    }

    unsafe {
        let base = NE2000.iobase;

        // Wait for previous transmission to complete
        let mut timeout = 10000;
        while timeout > 0 && inb(base + CR) & CR_TXP != 0 {
            timeout -= 1;
        }
        if timeout == 0 {
            return false;
        }

        // Pad to minimum Ethernet frame size (60 bytes without CRC)
        let len = if data.len() < 60 { 60 } else { data.len() };

        // Set up remote DMA to write to TX buffer
        outb(base + RSAR0, 0);
        outb(base + RSAR1, TX_START);
        outb(base + RBCR0, (len & 0xFF) as u8);
        outb(base + RBCR1, ((len >> 8) & 0xFF) as u8);
        outb(base + CR, CR_STA | CR_DMA_WRITE);

        // Write data (16-bit transfers)
        let mut i = 0;
        while i < len {
            let lo = if i < data.len() { data[i] } else { 0 };
            let hi = if i + 1 < data.len() { data[i + 1] } else { 0 };
            let word = (lo as u16) | ((hi as u16) << 8);
            outw(base + DATA, word);
            i += 2;
        }

        // Wait for DMA complete
        while inb(base + ISR) & ISR_RDC == 0 {}
        outb(base + ISR, ISR_RDC);

        // Set transmit page and byte count
        outb(base + TPSR, TX_START);
        outb(base + TBCR0, (len & 0xFF) as u8);
        outb(base + TBCR1, ((len >> 8) & 0xFF) as u8);

        // Start transmission
        outb(base + CR, CR_STA | CR_TXP | CR_DMA_NONE);

        true
    }
}

/// Acknowledge interrupt (clear ISR)
pub fn ack_interrupt() {
    // Already cleared in handle_interrupt
}
