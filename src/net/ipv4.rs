//! IPv4 protocol implementation
//!
//! Parses and builds IPv4 packets.

use crate::net::{arp, checksum, ethernet, CONFIG};
use crate::println;

/// IPv4 header minimum size (without options)
pub const HEADER_SIZE: usize = 20;

/// Maximum IPv4 packet size we'll handle
pub const MAX_PACKET_SIZE: usize = 1500;

/// IPv4 protocol numbers
pub const PROTO_ICMP: u8 = 1;
pub const PROTO_TCP: u8 = 6;
pub const PROTO_UDP: u8 = 17;

/// Parsed IPv4 header
#[derive(Debug, Clone, Copy)]
pub struct Ipv4Header {
    /// Version (should be 4)
    pub version: u8,
    /// Internet Header Length (in 32-bit words)
    pub ihl: u8,
    /// Type of Service
    pub tos: u8,
    /// Total length of packet (header + data)
    pub total_length: u16,
    /// Identification for fragmentation
    pub identification: u16,
    /// Flags (3 bits) and fragment offset (13 bits)
    pub flags_fragment: u16,
    /// Time To Live
    pub ttl: u8,
    /// Protocol (ICMP=1, TCP=6, UDP=17)
    pub protocol: u8,
    /// Header checksum
    pub checksum: u16,
    /// Source IP address
    pub src_ip: [u8; 4],
    /// Destination IP address
    pub dst_ip: [u8; 4],
}

impl Ipv4Header {
    /// Parse an IPv4 header from raw bytes
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < HEADER_SIZE {
            return None;
        }

        let version = (data[0] >> 4) & 0x0F;
        let ihl = data[0] & 0x0F;

        // Validate version
        if version != 4 {
            return None;
        }

        // IHL must be at least 5 (20 bytes)
        if ihl < 5 {
            return None;
        }

        let header_len = (ihl as usize) * 4;
        if data.len() < header_len {
            return None;
        }

        let tos = data[1];
        let total_length = u16::from_be_bytes([data[2], data[3]]);
        let identification = u16::from_be_bytes([data[4], data[5]]);
        let flags_fragment = u16::from_be_bytes([data[6], data[7]]);
        let ttl = data[8];
        let protocol = data[9];
        let checksum = u16::from_be_bytes([data[10], data[11]]);

        let mut src_ip = [0u8; 4];
        let mut dst_ip = [0u8; 4];
        src_ip.copy_from_slice(&data[12..16]);
        dst_ip.copy_from_slice(&data[16..20]);

        Some(Ipv4Header {
            version,
            ihl,
            tos,
            total_length,
            identification,
            flags_fragment,
            ttl,
            protocol,
            checksum,
            src_ip,
            dst_ip,
        })
    }

    /// Get the header length in bytes
    pub fn header_length(&self) -> usize {
        (self.ihl as usize) * 4
    }

    /// Get the payload portion of an IPv4 packet
    pub fn payload<'a>(&self, data: &'a [u8]) -> &'a [u8] {
        let header_len = self.header_length();
        let total = self.total_length as usize;
        if data.len() >= total && total > header_len {
            &data[header_len..total]
        } else if data.len() > header_len {
            &data[header_len..]
        } else {
            &[]
        }
    }

    /// Check if this packet is for us
    pub fn is_for_us(&self) -> bool {
        self.dst_ip == CONFIG.ip
    }

    /// Verify the header checksum
    pub fn verify_checksum(&self, data: &[u8]) -> bool {
        let header_len = self.header_length();
        if data.len() < header_len {
            return false;
        }
        checksum::verify_checksum(&data[..header_len])
    }

    /// Check if this packet is fragmented
    pub fn is_fragmented(&self) -> bool {
        // More Fragments flag (bit 13) or Fragment Offset != 0
        let mf = (self.flags_fragment & 0x2000) != 0;
        let offset = self.flags_fragment & 0x1FFF;
        mf || offset != 0
    }
}

/// Build an IPv4 packet header
///
/// Returns the number of bytes written (always 20, no options).
pub fn build_header(
    buffer: &mut [u8],
    protocol: u8,
    dst_ip: &[u8; 4],
    payload_len: usize,
) -> usize {
    if buffer.len() < HEADER_SIZE {
        return 0;
    }

    let total_length = (HEADER_SIZE + payload_len) as u16;

    // Version (4) and IHL (5 = 20 bytes, no options)
    buffer[0] = 0x45;
    // Type of Service (0 = default)
    buffer[1] = 0x00;
    // Total Length
    buffer[2..4].copy_from_slice(&total_length.to_be_bytes());
    // Identification (use a simple counter or 0)
    buffer[4..6].copy_from_slice(&identification().to_be_bytes());
    // Flags (Don't Fragment) and Fragment Offset (0)
    buffer[6..8].copy_from_slice(&0x4000u16.to_be_bytes());
    // TTL (64 is common)
    buffer[8] = 64;
    // Protocol
    buffer[9] = protocol;
    // Checksum (0 for now, calculate after)
    buffer[10] = 0;
    buffer[11] = 0;
    // Source IP
    buffer[12..16].copy_from_slice(&CONFIG.ip);
    // Destination IP
    buffer[16..20].copy_from_slice(dst_ip);

    // Calculate checksum
    let cksum = checksum::internet_checksum(&buffer[..HEADER_SIZE]);
    buffer[10..12].copy_from_slice(&cksum.to_be_bytes());

    HEADER_SIZE
}

/// Get next packet identification number
fn identification() -> u16 {
    static mut ID: u16 = 0;
    unsafe {
        ID = ID.wrapping_add(1);
        ID
    }
}

/// Process a received IPv4 packet
///
/// Validates the packet and dispatches to the appropriate protocol handler.
pub fn process_packet(data: &[u8]) {
    let Some(header) = Ipv4Header::parse(data) else {
        return;
    };

    // Check destination
    if !header.is_for_us() {
        return;
    }

    // Verify checksum
    if !header.verify_checksum(data) {
        println!("[ipv4] Bad checksum, dropping");
        return;
    }

    // We don't handle fragmented packets
    if header.is_fragmented() {
        println!("[ipv4] Fragmented packet, dropping");
        return;
    }

    // Get payload
    let payload = header.payload(data);

    // Dispatch based on protocol
    match header.protocol {
        PROTO_ICMP => {
            super::icmp::process_packet(&header, payload);
        }
        PROTO_TCP => {
            // TODO: Process TCP packet
            println!("[ipv4] TCP packet from {}.{}.{}.{}",
                header.src_ip[0], header.src_ip[1], header.src_ip[2], header.src_ip[3]);
        }
        _ => {
            // Unknown protocol, ignore
        }
    }
}

/// Send an IPv4 packet
///
/// Handles ARP resolution and Ethernet framing.
/// Returns true if the packet was sent (or queued for ARP).
pub fn send_packet(dst_ip: &[u8; 4], protocol: u8, payload: &[u8]) -> bool {
    // Resolve destination MAC via ARP
    let dst_mac = match arp::resolve(dst_ip) {
        Some(mac) => mac,
        None => {
            // ARP request sent, caller should retry later
            return false;
        }
    };

    // Build IPv4 packet
    let mut packet = [0u8; MAX_PACKET_SIZE];
    let header_len = build_header(&mut packet, protocol, dst_ip, payload.len());

    if header_len == 0 || header_len + payload.len() > MAX_PACKET_SIZE {
        return false;
    }

    // Copy payload
    packet[header_len..header_len + payload.len()].copy_from_slice(payload);

    // Send via Ethernet
    ethernet::send_frame(&dst_mac, ethernet::ETHERTYPE_IPV4, &packet[..header_len + payload.len()])
}
