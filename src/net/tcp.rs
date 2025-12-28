//! TCP (Transmission Control Protocol) implementation
//!
//! Implements a basic TCP state machine with:
//! - Connection establishment (3-way handshake)
//! - Data transfer with acknowledgments
//! - Connection termination
//! - Out-of-order segment handling
//! - Simple congestion control (Reno-like)

use crate::net::{checksum, ipv4};
use crate::println;
use crate::timer;

/// TCP header size (without options)
pub const HEADER_SIZE: usize = 20;

/// Maximum segment size (typical for Ethernet)
pub const MSS: u16 = 1460;

/// Maximum number of concurrent connections
const MAX_CONNECTIONS: usize = 4;

/// Receive buffer size per connection
const RX_BUFFER_SIZE: usize = 2048;

/// Send buffer size per connection
const TX_BUFFER_SIZE: usize = 2048;

/// Out-of-order segment buffer size
const OOO_BUFFER_SIZE: usize = 4;

/// Initial RTO (200ms in ticks at 100Hz)
const INITIAL_RTO: u64 = 20;

/// Minimum RTO (200ms)
const MIN_RTO: u64 = 20;

/// Maximum RTO (60 seconds)
const MAX_RTO: u64 = 6000;

/// Time-Wait timeout (30 seconds at 100Hz) - simplified from 2*MSL
const TIME_WAIT_TIMEOUT: u64 = 3000;

// TCP flags
const FLAG_FIN: u8 = 0x01;
const FLAG_SYN: u8 = 0x02;
const FLAG_RST: u8 = 0x04;
const FLAG_PSH: u8 = 0x08;
const FLAG_ACK: u8 = 0x10;

/// TCP connection state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TcpState {
    Closed,
    Listen,
    SynSent,
    SynReceived,
    Established,
    FinWait1,
    FinWait2,
    CloseWait,
    Closing,
    LastAck,
    TimeWait,
}

/// TCP header
#[derive(Debug, Clone, Copy)]
pub struct TcpHeader {
    pub src_port: u16,
    pub dst_port: u16,
    pub seq_num: u32,
    pub ack_num: u32,
    pub data_offset: u8,  // in 32-bit words
    pub flags: u8,
    pub window: u16,
    pub checksum: u16,
    pub urgent_ptr: u16,
}

impl TcpHeader {
    /// Parse a TCP header from raw bytes
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < HEADER_SIZE {
            return None;
        }

        let src_port = u16::from_be_bytes([data[0], data[1]]);
        let dst_port = u16::from_be_bytes([data[2], data[3]]);
        let seq_num = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
        let ack_num = u32::from_be_bytes([data[8], data[9], data[10], data[11]]);
        let data_offset = (data[12] >> 4) & 0x0F;
        let flags = data[13] & 0x3F;
        let window = u16::from_be_bytes([data[14], data[15]]);
        let checksum = u16::from_be_bytes([data[16], data[17]]);
        let urgent_ptr = u16::from_be_bytes([data[18], data[19]]);

        if data_offset < 5 {
            return None;
        }

        Some(TcpHeader {
            src_port,
            dst_port,
            seq_num,
            ack_num,
            data_offset,
            flags,
            window,
            checksum,
            urgent_ptr,
        })
    }

    /// Get header length in bytes
    pub fn header_length(&self) -> usize {
        (self.data_offset as usize) * 4
    }

    /// Get payload from TCP segment
    pub fn payload<'a>(&self, data: &'a [u8]) -> &'a [u8] {
        let header_len = self.header_length();
        if data.len() > header_len {
            &data[header_len..]
        } else {
            &[]
        }
    }

    /// Check if SYN flag is set
    pub fn is_syn(&self) -> bool {
        (self.flags & FLAG_SYN) != 0
    }

    /// Check if ACK flag is set
    pub fn is_ack(&self) -> bool {
        (self.flags & FLAG_ACK) != 0
    }

    /// Check if FIN flag is set
    pub fn is_fin(&self) -> bool {
        (self.flags & FLAG_FIN) != 0
    }

    /// Check if RST flag is set
    pub fn is_rst(&self) -> bool {
        (self.flags & FLAG_RST) != 0
    }
}

/// Maximum OOO segment data size
const OOO_DATA_SIZE: usize = 512;

/// Out-of-order segment
#[derive(Clone, Copy)]
struct OooSegment {
    seq: u32,
    len: u16,
    data: [u8; OOO_DATA_SIZE],
    valid: bool,
}

impl OooSegment {
    const fn empty() -> Self {
        OooSegment {
            seq: 0,
            len: 0,
            data: [0; OOO_DATA_SIZE],
            valid: false,
        }
    }
}

/// Ring buffer for data
struct RingBuffer {
    data: [u8; RX_BUFFER_SIZE],
    head: usize,
    tail: usize,
    len: usize,
}

impl RingBuffer {
    const fn new() -> Self {
        RingBuffer {
            data: [0; RX_BUFFER_SIZE],
            head: 0,
            tail: 0,
            len: 0,
        }
    }

    fn available(&self) -> usize {
        self.len
    }

    fn free_space(&self) -> usize {
        RX_BUFFER_SIZE - self.len
    }

    fn write(&mut self, data: &[u8]) -> usize {
        let to_write = core::cmp::min(data.len(), self.free_space());
        for &byte in data.iter().take(to_write) {
            self.data[self.head] = byte;
            self.head = (self.head + 1) % RX_BUFFER_SIZE;
        }
        self.len += to_write;
        to_write
    }

    fn read(&mut self, buf: &mut [u8]) -> usize {
        let to_read = core::cmp::min(buf.len(), self.len);
        for byte in buf.iter_mut().take(to_read) {
            *byte = self.data[self.tail];
            self.tail = (self.tail + 1) % RX_BUFFER_SIZE;
        }
        self.len -= to_read;
        to_read
    }

    fn peek(&self, buf: &mut [u8]) -> usize {
        let to_read = core::cmp::min(buf.len(), self.len);
        let mut pos = self.tail;
        for byte in buf.iter_mut().take(to_read) {
            *byte = self.data[pos];
            pos = (pos + 1) % RX_BUFFER_SIZE;
        }
        to_read
    }

    fn consume(&mut self, count: usize) {
        let to_consume = core::cmp::min(count, self.len);
        self.tail = (self.tail + to_consume) % RX_BUFFER_SIZE;
        self.len -= to_consume;
    }

    fn clear(&mut self) {
        self.head = 0;
        self.tail = 0;
        self.len = 0;
    }
}

/// TCP Control Block - state for one connection
pub struct TcpControlBlock {
    /// Connection state
    pub state: TcpState,
    /// Local IP
    pub local_ip: [u8; 4],
    /// Local port
    pub local_port: u16,
    /// Remote IP
    pub remote_ip: [u8; 4],
    /// Remote port
    pub remote_port: u16,

    // Send sequence space
    /// Send unacknowledged
    pub snd_una: u32,
    /// Send next
    pub snd_nxt: u32,
    /// Send window
    pub snd_wnd: u16,
    /// Initial send sequence number
    pub iss: u32,

    // Receive sequence space
    /// Receive next
    pub rcv_nxt: u32,
    /// Receive window
    pub rcv_wnd: u16,
    /// Initial receive sequence number
    pub irs: u32,

    // Retransmission
    /// Retransmission timeout (in ticks)
    pub rto: u64,
    /// Smoothed RTT
    pub srtt: u64,
    /// RTT variance
    pub rttvar: u64,
    /// Last send time (for RTT measurement)
    pub last_send_time: u64,
    /// Retransmit timer
    pub retransmit_timer: u64,
    /// Retransmit count
    pub retransmit_count: u8,

    // Congestion control
    /// Congestion window
    pub cwnd: u32,
    /// Slow start threshold
    pub ssthresh: u32,
    /// Duplicate ACK count
    pub dup_ack_count: u8,
    /// Last ACK received
    pub last_ack: u32,

    // Out-of-order buffer
    ooo_segments: [OooSegment; OOO_BUFFER_SIZE],

    // Data buffers
    rx_buffer: RingBuffer,
    tx_buffer: RingBuffer,

    // Time-Wait timer
    time_wait_timer: u64,

    /// Is this slot in use?
    pub in_use: bool,
    /// Has this connection received data?
    pub has_data: bool,
    /// Is the connection closed by remote?
    pub remote_closed: bool,
}

impl TcpControlBlock {
    const fn new() -> Self {
        TcpControlBlock {
            state: TcpState::Closed,
            local_ip: [0; 4],
            local_port: 0,
            remote_ip: [0; 4],
            remote_port: 0,
            snd_una: 0,
            snd_nxt: 0,
            snd_wnd: 0,
            iss: 0,
            rcv_nxt: 0,
            rcv_wnd: RX_BUFFER_SIZE as u16,
            irs: 0,
            rto: INITIAL_RTO,
            srtt: 0,
            rttvar: 0,
            last_send_time: 0,
            retransmit_timer: 0,
            retransmit_count: 0,
            cwnd: MSS as u32,
            ssthresh: 65535,
            dup_ack_count: 0,
            last_ack: 0,
            ooo_segments: [OooSegment::empty(); OOO_BUFFER_SIZE],
            rx_buffer: RingBuffer::new(),
            tx_buffer: RingBuffer::new(),
            time_wait_timer: 0,
            in_use: false,
            has_data: false,
            remote_closed: false,
        }
    }

    fn reset(&mut self) {
        *self = Self::new();
    }

    /// Get bytes available to read
    pub fn bytes_available(&self) -> usize {
        self.rx_buffer.available()
    }

    /// Read data from receive buffer
    pub fn read(&mut self, buf: &mut [u8]) -> usize {
        self.rx_buffer.read(buf)
    }

    /// Write data to send buffer
    pub fn write(&mut self, data: &[u8]) -> usize {
        self.tx_buffer.write(data)
    }

    /// Get bytes pending to send
    pub fn bytes_pending(&self) -> usize {
        self.tx_buffer.available()
    }

    /// Update receive window based on buffer space
    fn update_rcv_wnd(&mut self) {
        self.rcv_wnd = self.rx_buffer.free_space() as u16;
    }
}

/// Connection table
static mut CONNECTIONS: [TcpControlBlock; MAX_CONNECTIONS] = {
    const EMPTY: TcpControlBlock = TcpControlBlock::new();
    [EMPTY; MAX_CONNECTIONS]
};

/// Next ephemeral port
static mut NEXT_PORT: u16 = 49152;

/// Generate initial sequence number
fn generate_iss() -> u32 {
    // Simple ISN based on timer ticks (in production, use something more random)
    let ticks = timer::ticks();
    ((ticks as u32).wrapping_mul(0x41C64E6D)).wrapping_add(0x3039)
}

/// Allocate an ephemeral port
fn alloc_port() -> u16 {
    unsafe {
        let port = NEXT_PORT;
        NEXT_PORT = if NEXT_PORT >= 65535 { 49152 } else { NEXT_PORT + 1 };
        port
    }
}

/// Find a connection by local/remote address
fn find_connection(local_port: u16, remote_ip: &[u8; 4], remote_port: u16) -> Option<usize> {
    unsafe {
        for (i, conn) in CONNECTIONS.iter().enumerate() {
            if conn.in_use
                && conn.local_port == local_port
                && conn.remote_ip == *remote_ip
                && conn.remote_port == remote_port
            {
                return Some(i);
            }
        }
    }
    None
}

/// Find a listening connection on a port
fn find_listener(local_port: u16) -> Option<usize> {
    unsafe {
        for (i, conn) in CONNECTIONS.iter().enumerate() {
            if conn.in_use && conn.state == TcpState::Listen && conn.local_port == local_port {
                return Some(i);
            }
        }
    }
    None
}

/// Allocate a new connection slot
fn alloc_connection() -> Option<usize> {
    unsafe {
        for (i, conn) in CONNECTIONS.iter_mut().enumerate() {
            if !conn.in_use {
                conn.reset();
                conn.in_use = true;
                return Some(i);
            }
        }
    }
    None
}

/// Build TCP segment
fn build_segment(
    buffer: &mut [u8],
    src_port: u16,
    dst_port: u16,
    seq: u32,
    ack: u32,
    flags: u8,
    window: u16,
    payload: &[u8],
) -> usize {
    if buffer.len() < HEADER_SIZE + payload.len() {
        return 0;
    }

    // Source port
    buffer[0..2].copy_from_slice(&src_port.to_be_bytes());
    // Destination port
    buffer[2..4].copy_from_slice(&dst_port.to_be_bytes());
    // Sequence number
    buffer[4..8].copy_from_slice(&seq.to_be_bytes());
    // Acknowledgment number
    buffer[8..12].copy_from_slice(&ack.to_be_bytes());
    // Data offset (5 = 20 bytes, no options) and reserved
    buffer[12] = 0x50;
    // Flags
    buffer[13] = flags;
    // Window
    buffer[14..16].copy_from_slice(&window.to_be_bytes());
    // Checksum (0 for now)
    buffer[16..18].copy_from_slice(&[0, 0]);
    // Urgent pointer
    buffer[18..20].copy_from_slice(&[0, 0]);
    // Payload
    buffer[HEADER_SIZE..HEADER_SIZE + payload.len()].copy_from_slice(payload);

    HEADER_SIZE + payload.len()
}

/// Send TCP segment
fn send_segment(
    conn: &TcpControlBlock,
    flags: u8,
    payload: &[u8],
) -> bool {
    let mut segment = [0u8; 1500];
    let seg_len = build_segment(
        &mut segment,
        conn.local_port,
        conn.remote_port,
        conn.snd_nxt,
        conn.rcv_nxt,
        flags,
        conn.rcv_wnd,
        payload,
    );

    if seg_len == 0 {
        return false;
    }

    // Calculate TCP checksum using pseudo-header
    let cksum = checksum::tcp_udp_checksum(
        conn.local_ip,
        conn.remote_ip,
        ipv4::PROTO_TCP,
        &segment[..seg_len],
    );
    segment[16..18].copy_from_slice(&cksum.to_be_bytes());

    ipv4::send_packet(&conn.remote_ip, ipv4::PROTO_TCP, &segment[..seg_len])
}

/// Send RST segment
fn send_rst(src_ip: &[u8; 4], dst_ip: &[u8; 4], header: &TcpHeader) {
    let mut segment = [0u8; 20];
    let seq = if header.is_ack() { header.ack_num } else { 0 };
    let ack = header.seq_num.wrapping_add(1);
    let flags = FLAG_RST | if header.is_ack() { 0 } else { FLAG_ACK };

    let seg_len = build_segment(
        &mut segment,
        header.dst_port,
        header.src_port,
        seq,
        ack,
        flags,
        0,
        &[],
    );

    // Calculate checksum
    let cksum = checksum::tcp_udp_checksum(*src_ip, *dst_ip, ipv4::PROTO_TCP, &segment[..seg_len]);
    segment[16..18].copy_from_slice(&cksum.to_be_bytes());

    ipv4::send_packet(dst_ip, ipv4::PROTO_TCP, &segment[..seg_len]);
}

/// Process incoming TCP segment
pub fn process_packet(ip_header: &ipv4::Ipv4Header, data: &[u8]) {
    let Some(tcp) = TcpHeader::parse(data) else {
        return;
    };

    // Verify checksum
    let cksum = checksum::tcp_udp_checksum(
        ip_header.src_ip,
        ip_header.dst_ip,
        ipv4::PROTO_TCP,
        data,
    );
    if cksum != 0 {
        println!("[tcp] Bad checksum, dropping");
        return;
    }

    // Find existing connection
    if let Some(idx) = find_connection(tcp.dst_port, &ip_header.src_ip, tcp.src_port) {
        unsafe {
            process_segment(&mut CONNECTIONS[idx], &tcp, data, ip_header);
        }
        return;
    }

    // Check for listener (SYN to listening port)
    if tcp.is_syn() && !tcp.is_ack() {
        if let Some(_listener_idx) = find_listener(tcp.dst_port) {
            // Create new connection for incoming SYN
            if let Some(idx) = alloc_connection() {
                unsafe {
                    let conn = &mut CONNECTIONS[idx];
                    conn.local_ip = ip_header.dst_ip;
                    conn.local_port = tcp.dst_port;
                    conn.remote_ip = ip_header.src_ip;
                    conn.remote_port = tcp.src_port;
                    conn.state = TcpState::Listen;

                    process_segment(conn, &tcp, data, ip_header);
                }
                return;
            }
        }
    }

    // No connection found, send RST
    if !tcp.is_rst() {
        send_rst(&ip_header.dst_ip, &ip_header.src_ip, &tcp);
    }
}

/// Process segment for a connection
fn process_segment(
    conn: &mut TcpControlBlock,
    tcp: &TcpHeader,
    data: &[u8],
    ip_header: &ipv4::Ipv4Header,
) {
    // Handle RST
    if tcp.is_rst() {
        println!(
            "[tcp] RST received, closing {}:{}",
            conn.remote_ip[0], conn.remote_port
        );
        conn.reset();
        return;
    }

    match conn.state {
        TcpState::Closed => {
            // Should not happen
        }

        TcpState::Listen => {
            if tcp.is_syn() && !tcp.is_ack() {
                // Received SYN, send SYN-ACK
                conn.irs = tcp.seq_num;
                conn.rcv_nxt = tcp.seq_num.wrapping_add(1);
                conn.iss = generate_iss();
                conn.snd_nxt = conn.iss;
                conn.snd_una = conn.iss;
                conn.snd_wnd = tcp.window;
                conn.remote_ip = ip_header.src_ip;
                conn.remote_port = tcp.src_port;

                if send_segment(conn, FLAG_SYN | FLAG_ACK, &[]) {
                    conn.snd_nxt = conn.snd_nxt.wrapping_add(1);
                    conn.state = TcpState::SynReceived;
                    conn.last_send_time = timer::ticks();
                    conn.retransmit_timer = timer::ticks() + conn.rto;
                    println!(
                        "[tcp] SYN-ACK sent to {}.{}.{}.{}:{}",
                        conn.remote_ip[0], conn.remote_ip[1],
                        conn.remote_ip[2], conn.remote_ip[3],
                        conn.remote_port
                    );
                }
            }
        }

        TcpState::SynSent => {
            if tcp.is_syn() && tcp.is_ack() {
                // Received SYN-ACK
                if tcp.ack_num == conn.snd_nxt {
                    conn.irs = tcp.seq_num;
                    conn.rcv_nxt = tcp.seq_num.wrapping_add(1);
                    conn.snd_una = tcp.ack_num;
                    conn.snd_wnd = tcp.window;

                    // Send ACK
                    if send_segment(conn, FLAG_ACK, &[]) {
                        conn.state = TcpState::Established;
                        update_rtt(conn);
                        println!(
                            "[tcp] Connected to {}.{}.{}.{}:{}",
                            conn.remote_ip[0], conn.remote_ip[1],
                            conn.remote_ip[2], conn.remote_ip[3],
                            conn.remote_port
                        );
                    }
                }
            } else if tcp.is_syn() {
                // Simultaneous open
                conn.irs = tcp.seq_num;
                conn.rcv_nxt = tcp.seq_num.wrapping_add(1);

                if send_segment(conn, FLAG_SYN | FLAG_ACK, &[]) {
                    conn.state = TcpState::SynReceived;
                }
            }
        }

        TcpState::SynReceived => {
            if tcp.is_ack() && tcp.ack_num == conn.snd_nxt {
                conn.snd_una = tcp.ack_num;
                conn.snd_wnd = tcp.window;
                conn.state = TcpState::Established;
                update_rtt(conn);
                println!(
                    "[tcp] Established from {}.{}.{}.{}:{}",
                    conn.remote_ip[0], conn.remote_ip[1],
                    conn.remote_ip[2], conn.remote_ip[3],
                    conn.remote_port
                );

                // Process any data in this segment
                process_data(conn, tcp, data);
            }
        }

        TcpState::Established => {
            // Process ACK
            if tcp.is_ack() {
                process_ack(conn, tcp.ack_num);
            }

            // Process data
            process_data(conn, tcp, data);

            // Handle FIN
            if tcp.is_fin() {
                conn.rcv_nxt = conn.rcv_nxt.wrapping_add(1);
                conn.remote_closed = true;
                send_segment(conn, FLAG_ACK, &[]);
                conn.state = TcpState::CloseWait;
                println!("[tcp] Received FIN, entering CloseWait");
            }
        }

        TcpState::FinWait1 => {
            if tcp.is_ack() && tcp.ack_num == conn.snd_nxt {
                conn.snd_una = tcp.ack_num;
                if tcp.is_fin() {
                    conn.rcv_nxt = conn.rcv_nxt.wrapping_add(1);
                    send_segment(conn, FLAG_ACK, &[]);
                    conn.state = TcpState::TimeWait;
                    conn.time_wait_timer = timer::ticks() + TIME_WAIT_TIMEOUT;
                } else {
                    conn.state = TcpState::FinWait2;
                }
            } else if tcp.is_fin() {
                conn.rcv_nxt = conn.rcv_nxt.wrapping_add(1);
                send_segment(conn, FLAG_ACK, &[]);
                conn.state = TcpState::Closing;
            }
        }

        TcpState::FinWait2 => {
            if tcp.is_fin() {
                conn.rcv_nxt = conn.rcv_nxt.wrapping_add(1);
                send_segment(conn, FLAG_ACK, &[]);
                conn.state = TcpState::TimeWait;
                conn.time_wait_timer = timer::ticks() + TIME_WAIT_TIMEOUT;
            }
        }

        TcpState::CloseWait => {
            // Waiting for application to close
            if tcp.is_ack() {
                process_ack(conn, tcp.ack_num);
            }
        }

        TcpState::Closing => {
            if tcp.is_ack() && tcp.ack_num == conn.snd_nxt {
                conn.state = TcpState::TimeWait;
                conn.time_wait_timer = timer::ticks() + TIME_WAIT_TIMEOUT;
            }
        }

        TcpState::LastAck => {
            if tcp.is_ack() && tcp.ack_num == conn.snd_nxt {
                conn.reset();
                println!("[tcp] Connection closed");
            }
        }

        TcpState::TimeWait => {
            // Should timeout and close
        }
    }
}

/// Process incoming data
fn process_data(conn: &mut TcpControlBlock, tcp: &TcpHeader, data: &[u8]) {
    let payload = tcp.payload(data);
    if payload.is_empty() {
        return;
    }

    let seg_seq = tcp.seq_num;
    let seg_len = payload.len() as u32;

    // Check if segment is in order
    if seg_seq == conn.rcv_nxt {
        // In-order segment
        let written = conn.rx_buffer.write(payload);
        conn.rcv_nxt = conn.rcv_nxt.wrapping_add(written as u32);
        conn.has_data = true;
        conn.update_rcv_wnd();

        // Check for buffered out-of-order segments that are now in order
        deliver_ooo_segments(conn);

        // Send ACK
        send_segment(conn, FLAG_ACK, &[]);
    } else if seq_after(seg_seq, conn.rcv_nxt) {
        // Out-of-order segment, buffer it
        buffer_ooo_segment(conn, seg_seq, payload);

        // Send duplicate ACK
        send_segment(conn, FLAG_ACK, &[]);
    }
    // else: old segment, ignore
}

/// Buffer out-of-order segment
fn buffer_ooo_segment(conn: &mut TcpControlBlock, seq: u32, data: &[u8]) {
    // Find empty slot or oldest segment
    let mut slot = None;
    for (i, seg) in conn.ooo_segments.iter().enumerate() {
        if !seg.valid {
            slot = Some(i);
            break;
        }
    }

    if let Some(i) = slot {
        let len = core::cmp::min(data.len(), OOO_DATA_SIZE);
        conn.ooo_segments[i].seq = seq;
        conn.ooo_segments[i].len = len as u16;
        conn.ooo_segments[i].data[..len].copy_from_slice(&data[..len]);
        conn.ooo_segments[i].valid = true;
    }
}

/// Deliver buffered out-of-order segments that are now in order
fn deliver_ooo_segments(conn: &mut TcpControlBlock) {
    loop {
        let mut delivered = false;
        for seg in conn.ooo_segments.iter_mut() {
            if seg.valid && seg.seq == conn.rcv_nxt {
                let written = conn.rx_buffer.write(&seg.data[..seg.len as usize]);
                conn.rcv_nxt = conn.rcv_nxt.wrapping_add(written as u32);
                seg.valid = false;
                delivered = true;
                break;
            }
        }
        if !delivered {
            break;
        }
    }
}

/// Process ACK
fn process_ack(conn: &mut TcpControlBlock, ack: u32) {
    if seq_after(ack, conn.snd_una) && !seq_after(ack, conn.snd_nxt) {
        let bytes_acked = ack.wrapping_sub(conn.snd_una) as usize;
        conn.snd_una = ack;

        // Remove acked data from TX buffer
        conn.tx_buffer.consume(bytes_acked);

        // Update RTT
        update_rtt(conn);

        // Congestion control: update cwnd
        if conn.cwnd < conn.ssthresh {
            // Slow start
            conn.cwnd = conn.cwnd.saturating_add(bytes_acked as u32);
        } else {
            // Congestion avoidance
            conn.cwnd = conn.cwnd.saturating_add(
                (MSS as u32 * MSS as u32) / conn.cwnd
            );
        }

        // Reset duplicate ACK counter
        conn.dup_ack_count = 0;
        conn.last_ack = ack;

        // Reset retransmit timer
        if conn.snd_una != conn.snd_nxt {
            conn.retransmit_timer = timer::ticks() + conn.rto;
        }
    } else if ack == conn.last_ack {
        // Duplicate ACK
        conn.dup_ack_count += 1;
        if conn.dup_ack_count == 3 {
            // Fast retransmit
            conn.ssthresh = core::cmp::max(conn.cwnd / 2, 2 * MSS as u32);
            conn.cwnd = conn.ssthresh + 3 * MSS as u32;
            retransmit(conn);
        } else if conn.dup_ack_count > 3 {
            // Fast recovery
            conn.cwnd = conn.cwnd.saturating_add(MSS as u32);
        }
    }
}

/// Update RTT estimates
fn update_rtt(conn: &mut TcpControlBlock) {
    let now = timer::ticks();
    let measured = now.saturating_sub(conn.last_send_time);

    if conn.srtt == 0 {
        // First measurement
        conn.srtt = measured;
        conn.rttvar = measured / 2;
    } else {
        // RFC 6298 formulas
        let diff = if measured > conn.srtt {
            measured - conn.srtt
        } else {
            conn.srtt - measured
        };
        conn.rttvar = (3 * conn.rttvar + diff) / 4;
        conn.srtt = (7 * conn.srtt + measured) / 8;
    }

    conn.rto = conn.srtt + 4 * conn.rttvar;
    conn.rto = core::cmp::max(conn.rto, MIN_RTO);
    conn.rto = core::cmp::min(conn.rto, MAX_RTO);
}

/// Retransmit unacknowledged data
fn retransmit(conn: &mut TcpControlBlock) {
    let pending = conn.tx_buffer.available();
    if pending == 0 {
        return;
    }

    let to_send = core::cmp::min(pending, MSS as usize);
    let mut data = [0u8; MSS as usize];
    conn.tx_buffer.peek(&mut data[..to_send]);

    send_segment(conn, FLAG_ACK | FLAG_PSH, &data[..to_send]);
    conn.retransmit_timer = timer::ticks() + conn.rto;
    conn.retransmit_count += 1;
}

/// Sequence number comparison (handles wrap-around)
fn seq_after(a: u32, b: u32) -> bool {
    (a.wrapping_sub(b) as i32) > 0
}

/// Process TCP timers (called from network_task)
pub fn process_timers() {
    let now = timer::ticks();

    unsafe {
        for conn in CONNECTIONS.iter_mut() {
            if !conn.in_use {
                continue;
            }

            // Time-Wait timeout
            if conn.state == TcpState::TimeWait {
                if now >= conn.time_wait_timer {
                    conn.reset();
                    println!("[tcp] Time-Wait expired");
                }
                continue;
            }

            // Retransmission timeout
            if conn.retransmit_timer > 0 && now >= conn.retransmit_timer {
                if conn.retransmit_count >= 5 {
                    // Too many retries, abort
                    println!("[tcp] Connection timed out");
                    conn.reset();
                } else {
                    // Exponential backoff
                    conn.rto = core::cmp::min(conn.rto * 2, MAX_RTO);
                    conn.ssthresh = core::cmp::max(conn.cwnd / 2, 2 * MSS as u32);
                    conn.cwnd = MSS as u32;
                    retransmit(conn);
                }
            }

            // Send pending data
            if conn.state == TcpState::Established {
                send_pending_data(conn);
            }
        }
    }
}

/// Send pending data from TX buffer
fn send_pending_data(conn: &mut TcpControlBlock) {
    let pending = conn.tx_buffer.available();
    if pending == 0 {
        return;
    }

    // Calculate how much we can send
    let flight_size = conn.snd_nxt.wrapping_sub(conn.snd_una) as usize;
    let window = core::cmp::min(conn.snd_wnd as usize, conn.cwnd as usize);
    let can_send = window.saturating_sub(flight_size);

    if can_send == 0 {
        return;
    }

    let to_send = core::cmp::min(core::cmp::min(pending, can_send), MSS as usize);
    let mut data = [0u8; MSS as usize];
    conn.tx_buffer.peek(&mut data[..to_send]);

    if send_segment(conn, FLAG_ACK | FLAG_PSH, &data[..to_send]) {
        conn.snd_nxt = conn.snd_nxt.wrapping_add(to_send as u32);
        conn.last_send_time = timer::ticks();
        if conn.retransmit_timer == 0 {
            conn.retransmit_timer = timer::ticks() + conn.rto;
        }
    }
}

// ============================================================================
// Public API
// ============================================================================

/// Create a new socket
pub fn socket() -> Option<usize> {
    alloc_connection()
}

/// Start a connection (active open)
pub fn connect(sock: usize, remote_ip: &[u8; 4], remote_port: u16) -> bool {
    unsafe {
        if sock >= MAX_CONNECTIONS || !CONNECTIONS[sock].in_use {
            return false;
        }

        let conn = &mut CONNECTIONS[sock];
        if conn.state != TcpState::Closed {
            return false;
        }

        conn.local_ip = crate::net::CONFIG.ip;
        conn.local_port = alloc_port();
        conn.remote_ip = *remote_ip;
        conn.remote_port = remote_port;
        conn.iss = generate_iss();
        conn.snd_nxt = conn.iss;
        conn.snd_una = conn.iss;

        // Send SYN
        if send_segment(conn, FLAG_SYN, &[]) {
            conn.snd_nxt = conn.snd_nxt.wrapping_add(1);
            conn.state = TcpState::SynSent;
            conn.last_send_time = timer::ticks();
            conn.retransmit_timer = timer::ticks() + conn.rto;
            println!(
                "[tcp] Connecting to {}.{}.{}.{}:{}",
                remote_ip[0], remote_ip[1], remote_ip[2], remote_ip[3],
                remote_port
            );
            true
        } else {
            false
        }
    }
}

/// Listen on a port (passive open)
pub fn listen(sock: usize, port: u16) -> bool {
    unsafe {
        if sock >= MAX_CONNECTIONS || !CONNECTIONS[sock].in_use {
            return false;
        }

        let conn = &mut CONNECTIONS[sock];
        if conn.state != TcpState::Closed {
            return false;
        }

        conn.local_ip = crate::net::CONFIG.ip;
        conn.local_port = port;
        conn.state = TcpState::Listen;
        println!("[tcp] Listening on port {}", port);
        true
    }
}

/// Get socket state
pub fn get_state(sock: usize) -> TcpState {
    unsafe {
        if sock >= MAX_CONNECTIONS || !CONNECTIONS[sock].in_use {
            return TcpState::Closed;
        }
        CONNECTIONS[sock].state
    }
}

/// Check if connected
pub fn is_connected(sock: usize) -> bool {
    get_state(sock) == TcpState::Established
}

/// Get bytes available to read
pub fn available(sock: usize) -> usize {
    unsafe {
        if sock >= MAX_CONNECTIONS || !CONNECTIONS[sock].in_use {
            return 0;
        }
        CONNECTIONS[sock].bytes_available()
    }
}

/// Read data from socket (non-blocking)
pub fn recv(sock: usize, buf: &mut [u8]) -> isize {
    unsafe {
        if sock >= MAX_CONNECTIONS || !CONNECTIONS[sock].in_use {
            return -1;
        }

        let conn = &mut CONNECTIONS[sock];

        // Check for closed connection
        if conn.state == TcpState::Closed {
            return -1;
        }

        let n = conn.read(buf);
        conn.update_rcv_wnd();

        if n > 0 {
            n as isize
        } else if conn.remote_closed {
            -1  // EOF
        } else {
            0  // No data yet
        }
    }
}

/// Write data to socket (non-blocking)
pub fn send(sock: usize, data: &[u8]) -> isize {
    unsafe {
        if sock >= MAX_CONNECTIONS || !CONNECTIONS[sock].in_use {
            return -1;
        }

        let conn = &mut CONNECTIONS[sock];

        if conn.state != TcpState::Established && conn.state != TcpState::CloseWait {
            return -1;
        }

        let n = conn.write(data);
        n as isize
    }
}

/// Close socket (initiate graceful close)
pub fn close(sock: usize) {
    unsafe {
        if sock >= MAX_CONNECTIONS || !CONNECTIONS[sock].in_use {
            return;
        }

        let conn = &mut CONNECTIONS[sock];

        match conn.state {
            TcpState::Established => {
                if send_segment(conn, FLAG_FIN | FLAG_ACK, &[]) {
                    conn.snd_nxt = conn.snd_nxt.wrapping_add(1);
                    conn.state = TcpState::FinWait1;
                    conn.retransmit_timer = timer::ticks() + conn.rto;
                    println!("[tcp] Closing connection");
                }
            }
            TcpState::CloseWait => {
                if send_segment(conn, FLAG_FIN | FLAG_ACK, &[]) {
                    conn.snd_nxt = conn.snd_nxt.wrapping_add(1);
                    conn.state = TcpState::LastAck;
                    conn.retransmit_timer = timer::ticks() + conn.rto;
                }
            }
            TcpState::SynSent | TcpState::Listen => {
                conn.reset();
            }
            _ => {}
        }
    }
}

/// Accept a new connection on a listening socket
pub fn accept(sock: usize) -> Option<usize> {
    unsafe {
        if sock >= MAX_CONNECTIONS || !CONNECTIONS[sock].in_use {
            return None;
        }

        let listener = &CONNECTIONS[sock];
        if listener.state != TcpState::Listen {
            return None;
        }

        // Find an established connection on this port
        for (i, conn) in CONNECTIONS.iter().enumerate() {
            if i != sock
                && conn.in_use
                && conn.local_port == listener.local_port
                && conn.state == TcpState::Established
            {
                return Some(i);
            }
        }

        None
    }
}
