// Ralph OS Serial Port Driver
// Custom implementation - no external dependencies

use core::fmt;

// COM1 port address
const COM1: u16 = 0x3F8;

// UART register offsets
const DATA: u16 = 0;            // Data register (read/write)
const INT_ENABLE: u16 = 1;      // Interrupt enable
const FIFO_CTRL: u16 = 2;       // FIFO control
const LINE_CTRL: u16 = 3;       // Line control
const MODEM_CTRL: u16 = 4;      // Modem control
const LINE_STATUS: u16 = 5;     // Line status

// Line status bits
const LSR_DATA_READY: u8 = 0x01;
const LSR_TX_EMPTY: u8 = 0x20;

/// Port I/O: Read byte from port
#[inline]
unsafe fn inb(port: u16) -> u8 {
    let value: u8;
    core::arch::asm!(
        "in al, dx",
        out("al") value,
        in("dx") port,
        options(nomem, nostack, preserves_flags)
    );
    value
}

/// Port I/O: Write byte to port
#[inline]
unsafe fn outb(port: u16, value: u8) {
    core::arch::asm!(
        "out dx, al",
        in("dx") port,
        in("al") value,
        options(nomem, nostack, preserves_flags)
    );
}

/// Serial port writer
pub struct Serial {
    port: u16,
}

impl Serial {
    /// Create a new serial port instance
    pub const fn new(port: u16) -> Self {
        Serial { port }
    }

    /// Initialize the serial port
    pub fn init(&self) {
        unsafe {
            // Disable interrupts
            outb(self.port + INT_ENABLE, 0x00);

            // Enable DLAB (Divisor Latch Access Bit) to set baud rate
            outb(self.port + LINE_CTRL, 0x80);

            // Set divisor to 1 (115200 baud)
            outb(self.port + DATA, 0x01);         // Low byte
            outb(self.port + INT_ENABLE, 0x00);   // High byte

            // 8 bits, no parity, 1 stop bit (8N1)
            outb(self.port + LINE_CTRL, 0x03);

            // Enable FIFO, clear buffers, 14-byte threshold
            outb(self.port + FIFO_CTRL, 0xC7);

            // Enable IRQs, RTS/DSR set
            outb(self.port + MODEM_CTRL, 0x0B);

            // Set to normal operation mode (disable loopback)
            outb(self.port + MODEM_CTRL, 0x0F);
        }
    }

    /// Check if transmit buffer is empty
    fn is_tx_empty(&self) -> bool {
        unsafe { inb(self.port + LINE_STATUS) & LSR_TX_EMPTY != 0 }
    }

    /// Check if data is available to read
    #[allow(dead_code)]
    fn has_data(&self) -> bool {
        unsafe { inb(self.port + LINE_STATUS) & LSR_DATA_READY != 0 }
    }

    /// Write a single byte (blocking)
    pub fn write_byte(&self, byte: u8) {
        // Wait for transmit buffer to be empty
        while !self.is_tx_empty() {
            core::hint::spin_loop();
        }
        unsafe {
            outb(self.port + DATA, byte);
        }
    }

    /// Read a single byte (blocking)
    #[allow(dead_code)]
    pub fn read_byte(&self) -> u8 {
        // Wait for data to be available
        while !self.has_data() {
            core::hint::spin_loop();
        }
        unsafe { inb(self.port + DATA) }
    }

    /// Write a string
    pub fn write_str(&self, s: &str) {
        for byte in s.bytes() {
            if byte == b'\n' {
                self.write_byte(b'\r');
            }
            self.write_byte(byte);
        }
    }
}

impl fmt::Write for Serial {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        Serial::write_str(self, s);
        Ok(())
    }
}

// Global serial port instance
pub static SERIAL: Serial = Serial::new(COM1);

/// Initialize serial port (call once at startup)
pub fn init() {
    SERIAL.init();
}

/// Print to serial port (internal use)
#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    use core::fmt::Write;
    // Safety: We're single-threaded, no locking needed yet
    let mut serial = Serial::new(COM1);
    serial.write_fmt(args).unwrap();
}

/// Print to serial port
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::serial::_print(format_args!($($arg)*)));
}

/// Print to serial port with newline
#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}
