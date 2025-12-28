//! ARP (Address Resolution Protocol) implementation
//!
//! Handles ARP requests and replies for IPv4 over Ethernet.

use crate::net::{ethernet, ne2000, CONFIG};
use crate::println;

/// ARP header size
pub const HEADER_SIZE: usize = 28;

/// ARP operation codes
pub const ARP_REQUEST: u16 = 1;
pub const ARP_REPLY: u16 = 2;

/// Hardware type for Ethernet
const HTYPE_ETHERNET: u16 = 1;

/// Protocol type for IPv4
const PTYPE_IPV4: u16 = 0x0800;

/// Hardware address length for Ethernet
const HLEN_ETHERNET: u8 = 6;

/// Protocol address length for IPv4
const PLEN_IPV4: u8 = 4;

/// ARP cache entry
#[derive(Clone, Copy)]
pub struct ArpEntry {
    /// IP address
    pub ip: [u8; 4],
    /// MAC address
    pub mac: [u8; 6],
    /// Time when entry was added (in timer ticks)
    pub timestamp: u64,
    /// Is this entry valid?
    pub valid: bool,
}

impl ArpEntry {
    const fn empty() -> Self {
        ArpEntry {
            ip: [0; 4],
            mac: [0; 6],
            timestamp: 0,
            valid: false,
        }
    }
}

/// ARP cache size
const ARP_CACHE_SIZE: usize = 16;

/// ARP cache entry timeout (5 minutes in ticks at 100 Hz)
const ARP_TIMEOUT_TICKS: u64 = 5 * 60 * 100;

/// ARP cache
static mut ARP_CACHE: [ArpEntry; ARP_CACHE_SIZE] = [ArpEntry::empty(); ARP_CACHE_SIZE];

/// Parsed ARP packet
#[derive(Debug, Clone, Copy)]
pub struct ArpPacket {
    /// Hardware type (1 = Ethernet)
    pub htype: u16,
    /// Protocol type (0x0800 = IPv4)
    pub ptype: u16,
    /// Hardware address length (6 for Ethernet)
    pub hlen: u8,
    /// Protocol address length (4 for IPv4)
    pub plen: u8,
    /// Operation (1 = request, 2 = reply)
    pub operation: u16,
    /// Sender hardware address (MAC)
    pub sha: [u8; 6],
    /// Sender protocol address (IP)
    pub spa: [u8; 4],
    /// Target hardware address (MAC)
    pub tha: [u8; 6],
    /// Target protocol address (IP)
    pub tpa: [u8; 4],
}

impl ArpPacket {
    /// Parse an ARP packet from raw bytes
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < HEADER_SIZE {
            return None;
        }

        let htype = u16::from_be_bytes([data[0], data[1]]);
        let ptype = u16::from_be_bytes([data[2], data[3]]);
        let hlen = data[4];
        let plen = data[5];
        let operation = u16::from_be_bytes([data[6], data[7]]);

        // Validate for Ethernet/IPv4
        if htype != HTYPE_ETHERNET || ptype != PTYPE_IPV4 {
            return None;
        }
        if hlen != HLEN_ETHERNET || plen != PLEN_IPV4 {
            return None;
        }

        let mut sha = [0u8; 6];
        let mut spa = [0u8; 4];
        let mut tha = [0u8; 6];
        let mut tpa = [0u8; 4];

        sha.copy_from_slice(&data[8..14]);
        spa.copy_from_slice(&data[14..18]);
        tha.copy_from_slice(&data[18..24]);
        tpa.copy_from_slice(&data[24..28]);

        Some(ArpPacket {
            htype,
            ptype,
            hlen,
            plen,
            operation,
            sha,
            spa,
            tha,
            tpa,
        })
    }

    /// Check if this ARP request is for our IP
    pub fn is_for_our_ip(&self) -> bool {
        self.tpa == CONFIG.ip
    }
}

/// Build an ARP packet into a buffer
///
/// Returns the number of bytes written.
pub fn build_packet(
    buffer: &mut [u8],
    operation: u16,
    target_mac: &[u8; 6],
    target_ip: &[u8; 4],
) -> usize {
    if buffer.len() < HEADER_SIZE {
        return 0;
    }

    let our_mac = ne2000::mac_address();

    // Hardware type (Ethernet)
    buffer[0..2].copy_from_slice(&HTYPE_ETHERNET.to_be_bytes());
    // Protocol type (IPv4)
    buffer[2..4].copy_from_slice(&PTYPE_IPV4.to_be_bytes());
    // Hardware address length
    buffer[4] = HLEN_ETHERNET;
    // Protocol address length
    buffer[5] = PLEN_IPV4;
    // Operation
    buffer[6..8].copy_from_slice(&operation.to_be_bytes());
    // Sender hardware address (our MAC)
    buffer[8..14].copy_from_slice(&our_mac);
    // Sender protocol address (our IP)
    buffer[14..18].copy_from_slice(&CONFIG.ip);
    // Target hardware address
    buffer[18..24].copy_from_slice(target_mac);
    // Target protocol address
    buffer[24..28].copy_from_slice(target_ip);

    HEADER_SIZE
}

/// Process a received ARP packet
///
/// Handles ARP requests (sends reply) and ARP replies (updates cache).
pub fn process_packet(data: &[u8]) {
    let Some(arp) = ArpPacket::parse(data) else {
        return;
    };

    // Update cache with sender's info (we learned a mapping)
    update_cache(&arp.spa, &arp.sha);

    match arp.operation {
        ARP_REQUEST => {
            // Is this request for our IP?
            if arp.is_for_our_ip() {
                println!(
                    "[arp] Request for {}.{}.{}.{} from {}.{}.{}.{}",
                    arp.tpa[0], arp.tpa[1], arp.tpa[2], arp.tpa[3],
                    arp.spa[0], arp.spa[1], arp.spa[2], arp.spa[3]
                );
                send_reply(&arp.sha, &arp.spa);
            }
        }
        ARP_REPLY => {
            println!(
                "[arp] Reply: {}.{}.{}.{} is {:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
                arp.spa[0], arp.spa[1], arp.spa[2], arp.spa[3],
                arp.sha[0], arp.sha[1], arp.sha[2], arp.sha[3], arp.sha[4], arp.sha[5]
            );
        }
        _ => {}
    }
}

/// Send an ARP reply
fn send_reply(dst_mac: &[u8; 6], dst_ip: &[u8; 4]) {
    let mut arp_data = [0u8; HEADER_SIZE];
    build_packet(&mut arp_data, ARP_REPLY, dst_mac, dst_ip);

    if ethernet::send_frame(dst_mac, ethernet::ETHERTYPE_ARP, &arp_data) {
        println!(
            "[arp] Sent reply to {}.{}.{}.{}",
            dst_ip[0], dst_ip[1], dst_ip[2], dst_ip[3]
        );
    }
}

/// Send an ARP request for an IP address
pub fn send_request(target_ip: &[u8; 4]) {
    let mut arp_data = [0u8; HEADER_SIZE];
    let zero_mac = [0u8; 6];
    build_packet(&mut arp_data, ARP_REQUEST, &zero_mac, target_ip);

    if ethernet::send_frame(&ethernet::BROADCAST_MAC, ethernet::ETHERTYPE_ARP, &arp_data) {
        println!(
            "[arp] Sent request for {}.{}.{}.{}",
            target_ip[0], target_ip[1], target_ip[2], target_ip[3]
        );
    }
}

/// Look up a MAC address in the ARP cache
pub fn lookup(ip: &[u8; 4]) -> Option<[u8; 6]> {
    let now = crate::timer::ticks();

    unsafe {
        for entry in ARP_CACHE.iter() {
            if entry.valid && entry.ip == *ip {
                // Check if entry hasn't expired
                if now.wrapping_sub(entry.timestamp) < ARP_TIMEOUT_TICKS {
                    return Some(entry.mac);
                }
            }
        }
    }

    None
}

/// Update the ARP cache with a new mapping
fn update_cache(ip: &[u8; 4], mac: &[u8; 6]) {
    let now = crate::timer::ticks();

    unsafe {
        // First, check if we already have this IP
        for entry in ARP_CACHE.iter_mut() {
            if entry.valid && entry.ip == *ip {
                entry.mac = *mac;
                entry.timestamp = now;
                return;
            }
        }

        // Find an empty slot or the oldest entry
        let mut oldest_idx = 0;
        let mut oldest_time = u64::MAX;

        for (i, entry) in ARP_CACHE.iter().enumerate() {
            if !entry.valid {
                oldest_idx = i;
                break;
            }
            if entry.timestamp < oldest_time {
                oldest_time = entry.timestamp;
                oldest_idx = i;
            }
        }

        // Add new entry
        ARP_CACHE[oldest_idx] = ArpEntry {
            ip: *ip,
            mac: *mac,
            timestamp: now,
            valid: true,
        };
    }
}

/// Expire old entries from the ARP cache
pub fn expire_old_entries() {
    let now = crate::timer::ticks();

    unsafe {
        for entry in ARP_CACHE.iter_mut() {
            if entry.valid && now.wrapping_sub(entry.timestamp) >= ARP_TIMEOUT_TICKS {
                entry.valid = false;
            }
        }
    }
}

/// Resolve an IP address to a MAC address
///
/// Returns the MAC address if it's in the cache, or None if an ARP request
/// needs to be sent. The caller should retry after a delay.
pub fn resolve(ip: &[u8; 4]) -> Option<[u8; 6]> {
    // Check if IP is on our network
    let on_local_network = (ip[0] & CONFIG.netmask[0]) == (CONFIG.ip[0] & CONFIG.netmask[0])
        && (ip[1] & CONFIG.netmask[1]) == (CONFIG.ip[1] & CONFIG.netmask[1])
        && (ip[2] & CONFIG.netmask[2]) == (CONFIG.ip[2] & CONFIG.netmask[2])
        && (ip[3] & CONFIG.netmask[3]) == (CONFIG.ip[3] & CONFIG.netmask[3]);

    // If not on local network, resolve gateway instead
    let target_ip = if on_local_network { *ip } else { CONFIG.gateway };

    // Check cache first
    if let Some(mac) = lookup(&target_ip) {
        return Some(mac);
    }

    // Send ARP request
    send_request(&target_ip);
    None
}
