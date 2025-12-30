use core::fmt;

/// Result of polling for a single input byte.
pub enum ReadStatus {
    /// A byte is available.
    Byte(u8),
    /// No data is available yet.
    NoData,
    /// The underlying connection/stream is closed (EOF).
    Eof,
}

/// A terminal for the BASIC REPL: non-blocking input + formatted output.
pub trait Terminal: fmt::Write {
    fn poll_byte(&mut self) -> ReadStatus;
}

/// Serial-backed terminal (COM1).
pub struct SerialTerminal;

impl fmt::Write for SerialTerminal {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        crate::serial::SERIAL.write_str(s);
        Ok(())
    }
}

impl Terminal for SerialTerminal {
    fn poll_byte(&mut self) -> ReadStatus {
        if crate::serial::has_data() {
            ReadStatus::Byte(crate::serial::read_byte())
        } else {
            ReadStatus::NoData
        }
    }
}

