//! ICMP (Internet Control Message Protocol) implementation
//!
//! Handles ICMP echo request/reply (ping).

use crate::net::{checksum, ipv4};
use crate::println;

/// ICMP header size
pub const HEADER_SIZE: usize = 8;

/// ICMP types
pub const TYPE_ECHO_REPLY: u8 = 0;
pub const TYPE_ECHO_REQUEST: u8 = 8;

/// Parsed ICMP header
#[derive(Debug, Clone, Copy)]
pub struct IcmpHeader {
    /// ICMP type
    pub icmp_type: u8,
    /// ICMP code
    pub code: u8,
    /// Checksum
    pub checksum: u16,
    /// Identifier (for echo request/reply)
    pub identifier: u16,
    /// Sequence number (for echo request/reply)
    pub sequence: u16,
}

impl IcmpHeader {
    /// Parse an ICMP header from raw bytes
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < HEADER_SIZE {
            return None;
        }

        let icmp_type = data[0];
        let code = data[1];
        let checksum = u16::from_be_bytes([data[2], data[3]]);
        let identifier = u16::from_be_bytes([data[4], data[5]]);
        let sequence = u16::from_be_bytes([data[6], data[7]]);

        Some(IcmpHeader {
            icmp_type,
            code,
            checksum,
            identifier,
            sequence,
        })
    }

    /// Get the data portion of an ICMP packet (after header)
    pub fn payload(data: &[u8]) -> &[u8] {
        if data.len() > HEADER_SIZE {
            &data[HEADER_SIZE..]
        } else {
            &[]
        }
    }

    /// Verify the ICMP checksum
    pub fn verify_checksum(data: &[u8]) -> bool {
        checksum::verify_checksum(data)
    }
}

/// Build an ICMP echo reply packet
///
/// Returns the total packet length.
fn build_echo_reply(
    buffer: &mut [u8],
    identifier: u16,
    sequence: u16,
    payload: &[u8],
) -> usize {
    if buffer.len() < HEADER_SIZE + payload.len() {
        return 0;
    }

    // Type (Echo Reply)
    buffer[0] = TYPE_ECHO_REPLY;
    // Code
    buffer[1] = 0;
    // Checksum (0 for now)
    buffer[2] = 0;
    buffer[3] = 0;
    // Identifier
    buffer[4..6].copy_from_slice(&identifier.to_be_bytes());
    // Sequence
    buffer[6..8].copy_from_slice(&sequence.to_be_bytes());
    // Payload
    buffer[HEADER_SIZE..HEADER_SIZE + payload.len()].copy_from_slice(payload);

    // Calculate checksum over entire ICMP message
    let total_len = HEADER_SIZE + payload.len();
    let cksum = checksum::internet_checksum(&buffer[..total_len]);
    buffer[2..4].copy_from_slice(&cksum.to_be_bytes());

    total_len
}

/// Process a received ICMP packet
pub fn process_packet(ip_header: &ipv4::Ipv4Header, data: &[u8]) {
    let Some(icmp) = IcmpHeader::parse(data) else {
        return;
    };

    // Verify checksum
    if !IcmpHeader::verify_checksum(data) {
        println!("[icmp] Bad checksum, dropping");
        return;
    }

    match icmp.icmp_type {
        TYPE_ECHO_REQUEST => {
            println!(
                "[icmp] Echo request from {}.{}.{}.{} seq={}",
                ip_header.src_ip[0], ip_header.src_ip[1],
                ip_header.src_ip[2], ip_header.src_ip[3],
                icmp.sequence
            );

            // Send echo reply
            send_echo_reply(
                &ip_header.src_ip,
                icmp.identifier,
                icmp.sequence,
                IcmpHeader::payload(data),
            );
        }
        TYPE_ECHO_REPLY => {
            println!(
                "[icmp] Echo reply from {}.{}.{}.{} seq={}",
                ip_header.src_ip[0], ip_header.src_ip[1],
                ip_header.src_ip[2], ip_header.src_ip[3],
                icmp.sequence
            );
        }
        _ => {
            // Ignore other ICMP types for now
        }
    }
}

/// Send an ICMP echo reply
fn send_echo_reply(dst_ip: &[u8; 4], identifier: u16, sequence: u16, payload: &[u8]) {
    let mut icmp_buffer = [0u8; 1500];
    let icmp_len = build_echo_reply(&mut icmp_buffer, identifier, sequence, payload);

    if icmp_len == 0 {
        return;
    }

    if ipv4::send_packet(dst_ip, ipv4::PROTO_ICMP, &icmp_buffer[..icmp_len]) {
        println!(
            "[icmp] Sent echo reply to {}.{}.{}.{} seq={}",
            dst_ip[0], dst_ip[1], dst_ip[2], dst_ip[3], sequence
        );
    }
}
