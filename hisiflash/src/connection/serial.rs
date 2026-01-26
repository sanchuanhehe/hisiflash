//! Serial port implementation.

use crate::connection::ConnectionPort;
use crate::error::{Error, Result};
use serialport::{ClearBuffer, DataBits, FlowControl, Parity, StopBits};
use std::io::{Read, Write};
use std::time::Duration;

/// Serial port connection.
pub struct SerialPort {
    port: Box<dyn serialport::SerialPort>,
    name: String,
    timeout: Duration,
    baud_rate: u32,
}

impl SerialPort {
    /// Default timeout for serial operations.
    pub const DEFAULT_TIMEOUT: Duration = Duration::from_millis(1000);

    /// Open a serial port with the given parameters.
    pub fn open(port_name: &str, baud_rate: u32) -> Result<Self> {
        Self::open_with_timeout(port_name, baud_rate, Self::DEFAULT_TIMEOUT)
    }

    /// Open a serial port with custom timeout.
    pub fn open_with_timeout(port_name: &str, baud_rate: u32, timeout: Duration) -> Result<Self> {
        let port = serialport::new(port_name, baud_rate)
            .timeout(timeout)
            .data_bits(DataBits::Eight)
            .parity(Parity::None)
            .stop_bits(StopBits::One)
            .flow_control(FlowControl::None)
            .open()?;

        Ok(Self {
            port,
            name: port_name.to_string(),
            timeout,
            baud_rate,
        })
    }

    /// List available serial ports.
    pub fn list_ports() -> Result<Vec<serialport::SerialPortInfo>> {
        serialport::available_ports().map_err(Error::Serial)
    }

    /// Get the underlying serial port.
    pub fn inner(&self) -> &dyn serialport::SerialPort {
        self.port.as_ref()
    }

    /// Get mutable access to the underlying serial port.
    pub fn inner_mut(&mut self) -> &mut dyn serialport::SerialPort {
        self.port.as_mut()
    }

    /// Set DTR (Data Terminal Ready) pin state.
    pub fn set_dtr(&mut self, level: bool) -> Result<()> {
        self.port.write_data_terminal_ready(level)?;
        Ok(())
    }

    /// Set RTS (Request To Send) pin state.
    pub fn set_rts(&mut self, level: bool) -> Result<()> {
        self.port.write_request_to_send(level)?;
        Ok(())
    }

    /// Read DTR pin state.
    pub fn dtr(&self) -> Result<bool> {
        // Note: serialport doesn't provide a direct way to read DTR
        // This is typically not needed for HiSilicon chips
        Ok(false)
    }

    /// Read CTS (Clear To Send) pin state.
    pub fn cts(&mut self) -> Result<bool> {
        Ok(self.port.read_clear_to_send()?)
    }

    /// Read DSR (Data Set Ready) pin state.
    pub fn dsr(&mut self) -> Result<bool> {
        Ok(self.port.read_data_set_ready()?)
    }
}

impl ConnectionPort for SerialPort {
    fn set_timeout(&mut self, timeout: Duration) -> Result<()> {
        self.port.set_timeout(timeout)?;
        self.timeout = timeout;
        Ok(())
    }

    fn timeout(&self) -> Duration {
        self.timeout
    }

    fn set_baud_rate(&mut self, baud_rate: u32) -> Result<()> {
        self.port.set_baud_rate(baud_rate)?;
        self.baud_rate = baud_rate;
        Ok(())
    }

    fn baud_rate(&self) -> u32 {
        self.baud_rate
    }

    fn clear(&mut self) -> Result<()> {
        self.port.clear(ClearBuffer::All)?;
        Ok(())
    }

    fn name(&self) -> &str {
        &self.name
    }
}

impl Read for SerialPort {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.port.read(buf)
    }
}

impl Write for SerialPort {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.port.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.port.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_ports() {
        // This test just verifies that list_ports doesn't panic
        let _ = SerialPort::list_ports();
    }
}
