//! Network subsystem for Ralph OS
//!
//! Provides TCP/IP networking with an NE2000 NIC driver.
//!
//! ## Architecture
//!
//! - All packet processing happens in `network_task()`
//! - IRQ handler only copies packets to pre-allocated ring buffer
//! - User programs use non-blocking socket API

pub mod arp;
pub mod checksum;
pub mod ethernet;
pub mod ne2000;
pub mod packet;

use crate::println;

/// Network configuration
pub struct NetConfig {
    /// Our IP address
    pub ip: [u8; 4],
    /// Subnet mask
    pub netmask: [u8; 4],
    /// Gateway IP
    pub gateway: [u8; 4],
}

/// Default network configuration (for QEMU user networking)
pub static CONFIG: NetConfig = NetConfig {
    ip: [10, 0, 2, 15],       // QEMU user net default
    netmask: [255, 255, 255, 0],
    gateway: [10, 0, 2, 2],
};

/// Initialize the network subsystem
///
/// This must be called before enabling interrupts.
/// It pre-allocates all packet buffers.
pub fn init() {
    println!("Initializing network subsystem...");

    // Initialize packet buffer pool
    packet::init();

    println!("  IP: {}.{}.{}.{}", CONFIG.ip[0], CONFIG.ip[1], CONFIG.ip[2], CONFIG.ip[3]);
    println!("  Netmask: {}.{}.{}.{}", CONFIG.netmask[0], CONFIG.netmask[1], CONFIG.netmask[2], CONFIG.netmask[3]);
    println!("  Gateway: {}.{}.{}.{}", CONFIG.gateway[0], CONFIG.gateway[1], CONFIG.gateway[2], CONFIG.gateway[3]);
}

/// Main network task entry point
///
/// This task handles all protocol processing:
/// - Ethernet frame parsing
/// - ARP request/reply
/// - IPv4 routing
/// - ICMP ping reply
/// - TCP state machine
pub fn network_task() {
    println!("[net] Network task started");

    loop {
        // Process received packets
        while let Some((data, len)) = packet::get_rx_packet() {
            process_rx_packet(data, len);
            packet::release_rx_buffer();
        }

        // TODO: Process TCP timers

        // Process ARP cache expiry
        arp::expire_old_entries();

        // Sleep for 10ms (100 Hz polling)
        crate::scheduler::sleep_ms(10);
    }
}

/// Process a received packet
fn process_rx_packet(data: &[u8], len: usize) {
    // Parse Ethernet header
    let Some(eth_header) = ethernet::EthernetHeader::parse(&data[..len]) else {
        return;
    };

    // Check if frame is for us
    if !eth_header.is_for_us() {
        return;
    }

    // Get payload
    let payload = ethernet::EthernetHeader::payload(&data[..len]);

    // Dispatch based on EtherType
    match eth_header.ethertype {
        ethernet::ETHERTYPE_ARP => {
            arp::process_packet(payload);
        }
        ethernet::ETHERTYPE_IPV4 => {
            // TODO: Process IPv4 packet
            println!("[net] IPv4 packet ({} bytes)", payload.len());
        }
        _ => {
            // Unknown protocol, ignore
        }
    }
}
