//! Internet checksum calculation (RFC 1071)
//!
//! The Internet checksum is used by IP, ICMP, TCP, and UDP headers.
//! It's a 16-bit one's complement sum of the data.

/// Calculate the Internet checksum for a buffer
///
/// This implements the standard Internet checksum algorithm as defined
/// in RFC 1071. The checksum is the 16-bit one's complement of the
/// one's complement sum of all 16-bit words in the buffer.
///
/// # Arguments
/// * `data` - The data to checksum
///
/// # Returns
/// The 16-bit checksum value (in host byte order)
pub fn internet_checksum(data: &[u8]) -> u16 {
    let mut sum: u32 = 0;

    // Sum all 16-bit words
    let mut i = 0;
    while i + 1 < data.len() {
        let word = u16::from_be_bytes([data[i], data[i + 1]]);
        sum += word as u32;
        i += 2;
    }

    // Handle odd byte at the end
    if i < data.len() {
        sum += (data[i] as u32) << 8;
    }

    // Fold 32-bit sum to 16 bits
    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }

    // Return one's complement
    !(sum as u16)
}

/// Verify an Internet checksum
///
/// When calculated over data that includes a valid checksum field,
/// the one's complement sum should equal 0xFFFF (all ones).
/// After the final complement in fold_checksum, this becomes 0x0000.
///
/// # Arguments
/// * `data` - The data including checksum field
///
/// # Returns
/// `true` if the checksum is valid
pub fn verify_checksum(data: &[u8]) -> bool {
    let sum = checksum_accumulate(data);
    let folded = fold_checksum(sum);
    // For valid data with checksum, folded complement should be 0
    folded == 0x0000
}

/// Accumulate checksum over a buffer (partial checksum)
///
/// Use this when you need to combine checksums from multiple buffers,
/// such as when computing TCP/UDP checksum with pseudo-header.
///
/// # Arguments
/// * `data` - The data to sum
///
/// # Returns
/// The 32-bit accumulator (not yet folded or complemented)
pub fn checksum_accumulate(data: &[u8]) -> u32 {
    let mut sum: u32 = 0;

    let mut i = 0;
    while i + 1 < data.len() {
        let word = u16::from_be_bytes([data[i], data[i + 1]]);
        sum += word as u32;
        i += 2;
    }

    if i < data.len() {
        sum += (data[i] as u32) << 8;
    }

    sum
}

/// Add a 16-bit value to the checksum accumulator
pub fn checksum_add_u16(sum: u32, value: u16) -> u32 {
    sum + value as u32
}

/// Add a 32-bit value to the checksum accumulator (as two 16-bit words)
pub fn checksum_add_u32(sum: u32, value: u32) -> u32 {
    sum + ((value >> 16) & 0xFFFF) + (value & 0xFFFF)
}

/// Fold and complement the accumulator to get final checksum
pub fn fold_checksum(mut sum: u32) -> u16 {
    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16)
}

/// Calculate TCP/UDP checksum with pseudo-header
///
/// The pseudo-header includes: src IP, dst IP, zero, protocol, length
///
/// # Arguments
/// * `src_ip` - Source IP address
/// * `dst_ip` - Destination IP address
/// * `protocol` - IP protocol number (6 for TCP, 17 for UDP)
/// * `data` - The TCP/UDP header + payload
///
/// # Returns
/// The 16-bit checksum
pub fn tcp_udp_checksum(
    src_ip: [u8; 4],
    dst_ip: [u8; 4],
    protocol: u8,
    data: &[u8],
) -> u16 {
    let mut sum: u32 = 0;

    // Pseudo-header: source IP (4 bytes)
    sum += u16::from_be_bytes([src_ip[0], src_ip[1]]) as u32;
    sum += u16::from_be_bytes([src_ip[2], src_ip[3]]) as u32;

    // Pseudo-header: destination IP (4 bytes)
    sum += u16::from_be_bytes([dst_ip[0], dst_ip[1]]) as u32;
    sum += u16::from_be_bytes([dst_ip[2], dst_ip[3]]) as u32;

    // Pseudo-header: zero + protocol (2 bytes)
    sum += protocol as u32;

    // Pseudo-header: TCP/UDP length (2 bytes)
    sum += data.len() as u32;

    // Add the actual data
    sum += checksum_accumulate(data);

    fold_checksum(sum)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_checksum() {
        // Example from RFC 1071
        let data = [0x00, 0x01, 0xf2, 0x03, 0xf4, 0xf5, 0xf6, 0xf7];
        let checksum = internet_checksum(&data);
        // The checksum of this data should verify to 0
        let mut with_checksum = data.to_vec();
        with_checksum.extend_from_slice(&checksum.to_be_bytes());
        assert!(verify_checksum(&with_checksum));
    }

    #[test]
    fn test_odd_length() {
        let data = [0x45, 0x00, 0x00, 0x73, 0x00];
        let _ = internet_checksum(&data); // Should not panic
    }
}
