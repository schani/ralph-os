//! Ethernet frame handling
//!
//! Parses and builds Ethernet II frames.

use crate::net::ne2000;

/// Ethernet header size in bytes
pub const HEADER_SIZE: usize = 14;

/// Minimum Ethernet frame size (excluding FCS)
pub const MIN_FRAME_SIZE: usize = 60;

/// Maximum Ethernet frame size (excluding FCS)
pub const MAX_FRAME_SIZE: usize = 1514;

/// EtherType values
pub const ETHERTYPE_IPV4: u16 = 0x0800;
pub const ETHERTYPE_ARP: u16 = 0x0806;

/// Broadcast MAC address
pub const BROADCAST_MAC: [u8; 6] = [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF];

/// Parsed Ethernet frame header
#[derive(Debug, Clone, Copy)]
pub struct EthernetHeader {
    /// Destination MAC address
    pub dst_mac: [u8; 6],
    /// Source MAC address
    pub src_mac: [u8; 6],
    /// EtherType (protocol identifier)
    pub ethertype: u16,
}

impl EthernetHeader {
    /// Parse an Ethernet header from raw bytes
    ///
    /// Returns None if the buffer is too short.
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < HEADER_SIZE {
            return None;
        }

        let mut dst_mac = [0u8; 6];
        let mut src_mac = [0u8; 6];

        dst_mac.copy_from_slice(&data[0..6]);
        src_mac.copy_from_slice(&data[6..12]);

        let ethertype = u16::from_be_bytes([data[12], data[13]]);

        Some(EthernetHeader {
            dst_mac,
            src_mac,
            ethertype,
        })
    }

    /// Get the payload portion of an Ethernet frame
    pub fn payload(data: &[u8]) -> &[u8] {
        if data.len() > HEADER_SIZE {
            &data[HEADER_SIZE..]
        } else {
            &[]
        }
    }

    /// Check if this frame is addressed to us or broadcast
    pub fn is_for_us(&self) -> bool {
        let our_mac = ne2000::mac_address();
        self.dst_mac == our_mac || self.dst_mac == BROADCAST_MAC
    }

    /// Check if this is a broadcast frame
    pub fn is_broadcast(&self) -> bool {
        self.dst_mac == BROADCAST_MAC
    }
}

/// Build an Ethernet frame
///
/// Writes the Ethernet header to the buffer and returns the header size.
/// The caller should write the payload after this.
pub fn build_frame(
    buffer: &mut [u8],
    dst_mac: &[u8; 6],
    ethertype: u16,
) -> usize {
    if buffer.len() < HEADER_SIZE {
        return 0;
    }

    let src_mac = ne2000::mac_address();

    // Destination MAC
    buffer[0..6].copy_from_slice(dst_mac);

    // Source MAC
    buffer[6..12].copy_from_slice(&src_mac);

    // EtherType (big endian)
    buffer[12..14].copy_from_slice(&ethertype.to_be_bytes());

    HEADER_SIZE
}

/// Send an Ethernet frame
///
/// Builds the frame header and sends the complete frame.
/// Returns true if the frame was sent successfully.
pub fn send_frame(dst_mac: &[u8; 6], ethertype: u16, payload: &[u8]) -> bool {
    let frame_len = HEADER_SIZE + payload.len();

    if frame_len > MAX_FRAME_SIZE {
        return false;
    }

    // Get a TX buffer from the packet pool
    if let Some(buffer) = super::packet::get_tx_buffer() {
        // Build header
        let header_len = build_frame(buffer, dst_mac, ethertype);

        // Copy payload
        if header_len + payload.len() <= buffer.len() {
            buffer[header_len..header_len + payload.len()].copy_from_slice(payload);

            // Calculate frame length (pad to minimum if needed)
            let send_len = core::cmp::max(frame_len, MIN_FRAME_SIZE);

            // Pad with zeros if needed
            if frame_len < MIN_FRAME_SIZE {
                for byte in buffer[frame_len..MIN_FRAME_SIZE].iter_mut() {
                    *byte = 0;
                }
            }

            // Mark buffer ready and send
            let _index = super::packet::tx_buffer_ready(send_len);

            // Actually transmit via NE2000
            return ne2000::send(&buffer[..send_len]);
        }
    }

    false
}
