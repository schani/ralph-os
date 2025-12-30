use core::fmt;

use crate::basic::terminal::{ReadStatus, Terminal};
use crate::net::tcp;
use crate::scheduler;

const TELNET_PORT: u16 = 23;

const IAC: u8 = 255;
const DONT: u8 = 254;
const DO: u8 = 253;
const WONT: u8 = 252;
const WILL: u8 = 251;
const SB: u8 = 250;
const SE: u8 = 240;

const OPT_ECHO: u8 = 1;
const OPT_SUPPRESS_GO_AHEAD: u8 = 3;
const OPT_LINEMODE: u8 = 34;

#[derive(Clone, Copy, Debug)]
enum RxState {
    Data,
    Iac,
    IacCommand(u8),
    Subnegotiation,
    SubnegotiationIac,
}

pub struct TelnetTerminal {
    sock: usize,
    rx_buf: [u8; 128],
    rx_pos: usize,
    rx_len: usize,
    rx_state: RxState,
    swallow_lf: bool,
    closed: bool,
}

impl TelnetTerminal {
    pub fn new(sock: usize) -> Self {
        TelnetTerminal {
            sock,
            rx_buf: [0; 128],
            rx_pos: 0,
            rx_len: 0,
            rx_state: RxState::Data,
            swallow_lf: false,
            closed: false,
        }
    }

    pub fn negotiate(&mut self) {
        // Ask the client to let the server echo and suppress go-ahead.
        let _ = self.send_bytes(&[IAC, WILL, OPT_ECHO]);
        let _ = self.send_bytes(&[IAC, WILL, OPT_SUPPRESS_GO_AHEAD]);
        let _ = self.send_bytes(&[IAC, DO, OPT_SUPPRESS_GO_AHEAD]);
        let _ = self.send_bytes(&[IAC, WONT, OPT_LINEMODE]);
    }

    fn send_bytes(&mut self, mut bytes: &[u8]) -> Result<(), fmt::Error> {
        while !bytes.is_empty() {
            let n = tcp::send(self.sock, bytes);
            if n < 0 {
                self.closed = true;
                return Err(fmt::Error);
            }
            let n = n as usize;
            if n == 0 {
                scheduler::yield_now();
                continue;
            }
            bytes = &bytes[n..];
        }
        Ok(())
    }

    fn reply_to_command(&mut self, cmd: u8, opt: u8) {
        // Minimal, mostly-refuse negotiation with a couple of safe opts.
        let (resp_cmd, resp_opt) = match (cmd, opt) {
            (DO, OPT_ECHO) => (WILL, OPT_ECHO),
            (DO, OPT_SUPPRESS_GO_AHEAD) => (WILL, OPT_SUPPRESS_GO_AHEAD),
            (WILL, OPT_SUPPRESS_GO_AHEAD) => (DO, OPT_SUPPRESS_GO_AHEAD),
            // Refuse everything else.
            (DO, _) => (WONT, opt),
            (DONT, _) => (WONT, opt),
            (WILL, _) => (DONT, opt),
            (WONT, _) => (DONT, opt),
            _ => return,
        };

        let _ = self.send_bytes(&[IAC, resp_cmd, resp_opt]);
    }
}

impl fmt::Write for TelnetTerminal {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        if self.closed {
            return Err(fmt::Error);
        }

        // Telnet requires:
        // - Newlines as CRLF.
        // - A literal 0xFF sent as 0xFF 0xFF.
        // - Bare CR sent as CR NUL (to avoid CRLF ambiguity).
        let mut out = [0u8; 256];
        let mut out_len = 0usize;

        for b in s.as_bytes().iter().copied() {
            let emit = |buf: &mut [u8; 256], len: &mut usize, bytes: &[u8], this: &mut TelnetTerminal| -> fmt::Result {
                if *len + bytes.len() > buf.len() {
                    this.send_bytes(&buf[..*len])?;
                    *len = 0;
                }
                buf[*len..*len + bytes.len()].copy_from_slice(bytes);
                *len += bytes.len();
                Ok(())
            };

            match b {
                b'\n' => {
                    emit(&mut out, &mut out_len, &[b'\r', b'\n'], self)?;
                }
                b'\r' => {
                    emit(&mut out, &mut out_len, &[b'\r', 0], self)?;
                }
                IAC => {
                    emit(&mut out, &mut out_len, &[IAC, IAC], self)?;
                }
                _ => {
                    emit(&mut out, &mut out_len, &[b], self)?;
                }
            }
        }

        if out_len > 0 {
            self.send_bytes(&out[..out_len])?;
        }

        Ok(())
    }
}

impl Terminal for TelnetTerminal {
    fn poll_byte(&mut self) -> ReadStatus {
        if self.closed {
            return ReadStatus::Eof;
        }

        loop {
            if self.rx_pos >= self.rx_len {
                self.rx_pos = 0;
                self.rx_len = 0;

                let n = tcp::recv(self.sock, &mut self.rx_buf);
                if n < 0 {
                    self.closed = true;
                    return ReadStatus::Eof;
                }
                let n = n as usize;
                if n == 0 {
                    return ReadStatus::NoData;
                }
                self.rx_len = n;
            }

            let b = self.rx_buf[self.rx_pos];
            self.rx_pos += 1;

            if self.swallow_lf {
                if b == b'\n' {
                    self.swallow_lf = false;
                    continue;
                }
                self.swallow_lf = false;
            }

            match self.rx_state {
                RxState::Data => {
                    if b == IAC {
                        self.rx_state = RxState::Iac;
                        continue;
                    }
                    if b == b'\r' {
                        self.swallow_lf = true;
                        return ReadStatus::Byte(b'\n');
                    }
                    return ReadStatus::Byte(b);
                }
                RxState::Iac => {
                    match b {
                        IAC => {
                            self.rx_state = RxState::Data;
                            return ReadStatus::Byte(IAC);
                        }
                        DO | DONT | WILL | WONT => {
                            self.rx_state = RxState::IacCommand(b);
                            continue;
                        }
                        SB => {
                            self.rx_state = RxState::Subnegotiation;
                            continue;
                        }
                        SE => {
                            self.rx_state = RxState::Data;
                            continue;
                        }
                        _ => {
                            self.rx_state = RxState::Data;
                            continue;
                        }
                    }
                }
                RxState::IacCommand(cmd) => {
                    self.reply_to_command(cmd, b);
                    self.rx_state = RxState::Data;
                    continue;
                }
                RxState::Subnegotiation => {
                    if b == IAC {
                        self.rx_state = RxState::SubnegotiationIac;
                    }
                    continue;
                }
                RxState::SubnegotiationIac => {
                    if b == SE {
                        self.rx_state = RxState::Data;
                    } else if b != IAC {
                        self.rx_state = RxState::Subnegotiation;
                    }
                    continue;
                }
            }
        }
    }
}

pub fn telnetd_task() {
    crate::println!("[telnet] telnetd started");

    let Some(listener) = tcp::socket() else {
        crate::println!("[telnet] Failed to allocate listener socket");
        return;
    };
    if !tcp::listen(listener, TELNET_PORT) {
        crate::println!("[telnet] Failed to listen on {}", TELNET_PORT);
        return;
    }

    loop {
        if let Some(sock) = tcp::accept(listener) {
            if scheduler::spawn_with_arg("telnet", telnet_session_task, sock).is_none() {
                crate::println!("[telnet] Failed to spawn session task");
                tcp::close(sock);
            }
        } else {
            scheduler::sleep_ms(25);
        }
    }
}

fn telnet_session_task(sock: usize) {
    crate::println!("[telnet] Session started (sock={})", sock);

    let mut term = TelnetTerminal::new(sock);
    term.negotiate();

    crate::basic::run_repl_on_terminal(&mut term);

    tcp::close(sock);
    crate::println!("[telnet] Session ended (sock={})", sock);
}

