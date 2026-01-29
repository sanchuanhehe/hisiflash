//! Port abstraction for cross-platform serial communication.
//!
//! This module provides a unified `Port` trait that abstracts over different
//! serial port implementations:
//!
//! - **Native platforms** (Linux, macOS, Windows): Uses the `serialport` crate
//! - **WASM/Web**: Uses Web Serial API via `web-sys` (feature-gated)
//!
//! ## Architecture
//!
//! The design separates I/O from protocol logic, allowing the protocol layer
//! to be I/O-agnostic and portable across platforms.
//!
//! ```text
//! +------------------+     +------------------+
//! |   Protocol Layer |     |   Protocol Layer |
//! |  (seboot, ymodem)|     |  (seboot, ymodem)|
//! +--------+---------+     +--------+---------+
//!          |                        |
//!          v                        v
//! +--------+---------+     +--------+---------+
//! |   Port Trait     |     |   Port Trait     |
//! +--------+---------+     +--------+---------+
//!          |                        |
//!          v                        v
//! +--------+---------+     +--------+---------+
//! | Native SerialPort|     | WebSerial Port   |
//! |   (serialport)   |     |    (web-sys)     |
//! +------------------+     +------------------+
//!       Desktop              Browser/WASM
//! ```
//!
//! ## Example
//!
//! ```rust,no_run
//! use hisiflash::port::{Port, SerialConfig};
//!
//! fn example<P: Port>(port: &mut P) -> std::io::Result<()> {
//!     port.write_all(b"Hello")?;
//!     
//!     let mut buf = [0u8; 32];
//!     let n = port.read(&mut buf)?;
//!     println!("Received: {:?}", &buf[..n]);
//!     
//!     Ok(())
//! }
//! ```

#[cfg(feature = "native")]
pub mod native;

#[cfg(feature = "wasm")]
pub mod wasm;

use std::io::{Read, Write};
use std::time::Duration;

use crate::error::Result;

/// Serial port configuration.
#[derive(Debug, Clone)]
pub struct SerialConfig {
    /// Port name/path (e.g., "/dev/ttyUSB0", "COM3").
    pub port_name: String,
    /// Baud rate.
    pub baud_rate: u32,
    /// Read/write timeout.
    pub timeout: Duration,
    /// Data bits (typically 8).
    pub data_bits: DataBits,
    /// Parity (typically None).
    pub parity: Parity,
    /// Stop bits (typically One).
    pub stop_bits: StopBits,
    /// Flow control (typically None).
    pub flow_control: FlowControl,
}

impl Default for SerialConfig {
    fn default() -> Self {
        Self {
            port_name: String::new(),
            baud_rate: 115200,
            timeout: Duration::from_millis(1000),
            data_bits: DataBits::Eight,
            parity: Parity::None,
            stop_bits: StopBits::One,
            flow_control: FlowControl::None,
        }
    }
}

impl SerialConfig {
    /// Create a new configuration with port name and baud rate.
    pub fn new(port_name: impl Into<String>, baud_rate: u32) -> Self {
        Self {
            port_name: port_name.into(),
            baud_rate,
            ..Default::default()
        }
    }

    /// Set the timeout.
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

/// Number of data bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DataBits {
    /// 5 data bits.
    Five,
    /// 6 data bits.
    Six,
    /// 7 data bits.
    Seven,
    /// 8 data bits.
    #[default]
    Eight,
}

/// Parity checking mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Parity {
    /// No parity.
    #[default]
    None,
    /// Odd parity.
    Odd,
    /// Even parity.
    Even,
}

/// Number of stop bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StopBits {
    /// 1 stop bit.
    #[default]
    One,
    /// 2 stop bits.
    Two,
}

/// Flow control mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FlowControl {
    /// No flow control.
    #[default]
    None,
    /// Hardware flow control (RTS/CTS).
    Hardware,
    /// Software flow control (XON/XOFF).
    Software,
}

/// Serial port information.
#[derive(Debug, Clone)]
pub struct PortInfo {
    /// Port name/path.
    pub name: String,
    /// USB vendor ID (if available).
    pub vid: Option<u16>,
    /// USB product ID (if available).
    pub pid: Option<u16>,
    /// Manufacturer string (if available).
    pub manufacturer: Option<String>,
    /// Product string (if available).
    pub product: Option<String>,
    /// Serial number (if available).
    pub serial_number: Option<String>,
}

/// Unified port trait for serial communication.
///
/// This trait provides a platform-agnostic interface for serial port operations.
/// Implementations exist for:
///
/// - Native platforms via the `serialport` crate
/// - WASM/Web via the Web Serial API
pub trait Port: Read + Write + Send {
    /// Set the read/write timeout.
    fn set_timeout(&mut self, timeout: Duration) -> Result<()>;

    /// Get the current timeout.
    fn timeout(&self) -> Duration;

    /// Set the baud rate.
    fn set_baud_rate(&mut self, baud_rate: u32) -> Result<()>;

    /// Get the current baud rate.
    fn baud_rate(&self) -> u32;

    /// Clear input/output buffers.
    fn clear_buffers(&mut self) -> Result<()>;

    /// Get the port name/path.
    fn name(&self) -> &str;

    /// Set DTR (Data Terminal Ready) pin state.
    fn set_dtr(&mut self, level: bool) -> Result<()>;

    /// Set RTS (Request To Send) pin state.
    fn set_rts(&mut self, level: bool) -> Result<()>;

    /// Read CTS (Clear To Send) pin state.
    fn read_cts(&self) -> Result<bool>;

    /// Read DSR (Data Set Ready) pin state.
    fn read_dsr(&self) -> Result<bool>;

    /// Close the port and release resources.
    ///
    /// After calling this method, the port cannot be used for further I/O.
    fn close(&mut self) -> Result<()>;

    /// Write all bytes, blocking until complete.
    fn write_all_bytes(&mut self, buf: &[u8]) -> Result<()> {
        std::io::Write::write_all(self, buf)?;
        std::io::Write::flush(self)?;
        Ok(())
    }
}

/// Trait for listing available serial ports.
///
/// This is separated from `Port` because it's a static operation that
/// doesn't require an open port instance.
pub trait PortEnumerator {
    /// List all available serial ports.
    fn list_ports() -> Result<Vec<PortInfo>>;

    /// Find ports matching the given VID/PID.
    fn find_by_vid_pid(vid: u16, pid: u16) -> Result<Vec<PortInfo>> {
        let ports = Self::list_ports()?;
        Ok(ports
            .into_iter()
            .filter(|p| p.vid == Some(vid) && p.pid == Some(pid))
            .collect())
    }
}

// Re-export the appropriate implementation based on features
#[cfg(feature = "native")]
pub use native::{NativePort, NativePortEnumerator};

#[cfg(feature = "wasm")]
pub use wasm::{WebSerialPort, WebSerialPortEnumerator};
