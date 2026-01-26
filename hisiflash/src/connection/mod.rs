//! Serial port connection abstraction.

pub mod detect;
pub mod serial;

// Re-export for convenience
pub use detect::{DetectedPort, UsbDevice};
pub use serial::SerialPort;

use crate::error::Result;
use std::io::{Read, Write};
use std::time::Duration;

/// Trait for connection ports (serial, TCP, etc.)
pub trait ConnectionPort: Read + Write + Send {
    /// Set the read/write timeout.
    fn set_timeout(&mut self, timeout: Duration) -> Result<()>;

    /// Get the current timeout.
    fn timeout(&self) -> Duration;

    /// Set the baud rate.
    fn set_baud_rate(&mut self, baud_rate: u32) -> Result<()>;

    /// Get the current baud rate.
    fn baud_rate(&self) -> u32;

    /// Clear input/output buffers.
    fn clear(&mut self) -> Result<()>;

    /// Get the port name/path.
    fn name(&self) -> &str;
}

/// List available serial ports.
pub fn list_ports() -> Result<Vec<String>> {
    Ok(serial::SerialPort::list_ports()?
        .into_iter()
        .map(|p| p.port_name)
        .collect())
}

/// Find the first available serial port matching a pattern.
pub fn find_port(pattern: Option<&str>) -> Result<String> {
    let ports = list_ports()?;

    match pattern {
        Some(p) => ports
            .into_iter()
            .find(|port| port.contains(p))
            .ok_or(crate::error::Error::DeviceNotFound),
        None => ports
            .into_iter()
            .next()
            .ok_or(crate::error::Error::DeviceNotFound),
    }
}
