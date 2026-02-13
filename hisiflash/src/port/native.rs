//! Native serial port implementation using the `serialport` crate.
//!
//! This module provides the serial port implementation for native platforms
//! (Linux, macOS, Windows, FreeBSD, etc.).

use {
    crate::{
        error::{Error, Result},
        port::{
            DataBits, FlowControl, Parity, Port, PortEnumerator, PortInfo, SerialConfig, StopBits,
        },
    },
    log::trace,
    serialport::ClearBuffer,
    std::{
        io::{Read, Write},
        time::Duration,
    },
};

/// Native serial port implementation.
pub struct NativePort {
    port: Option<Box<dyn serialport::SerialPort>>,
    name: String,
    timeout: Duration,
    baud_rate: u32,
}

impl NativePort {
    /// Open a serial port with the given configuration.
    pub fn open(config: &SerialConfig) -> Result<Self> {
        let port = serialport::new(&config.port_name, config.baud_rate)
            .timeout(config.timeout)
            .data_bits(
                config
                    .data_bits
                    .into(),
            )
            .parity(
                config
                    .parity
                    .into(),
            )
            .stop_bits(
                config
                    .stop_bits
                    .into(),
            )
            .flow_control(
                config
                    .flow_control
                    .into(),
            )
            .open()?;

        Ok(Self {
            port: Some(port),
            name: config
                .port_name
                .clone(),
            timeout: config.timeout,
            baud_rate: config.baud_rate,
        })
    }

    /// Open a serial port with default settings.
    pub fn open_simple(port_name: &str, baud_rate: u32) -> Result<Self> {
        let config = SerialConfig::new(port_name, baud_rate);
        Self::open(&config)
    }
}

impl Port for NativePort {
    fn set_timeout(&mut self, timeout: Duration) -> Result<()> {
        if let Some(ref mut p) = self.port {
            p.set_timeout(timeout)?;
        }
        self.timeout = timeout;
        Ok(())
    }

    fn timeout(&self) -> Duration {
        self.timeout
    }

    fn set_baud_rate(&mut self, baud_rate: u32) -> Result<()> {
        if let Some(ref mut p) = self.port {
            p.set_baud_rate(baud_rate)?;
        }
        self.baud_rate = baud_rate;
        Ok(())
    }

    fn baud_rate(&self) -> u32 {
        self.baud_rate
    }

    fn clear_buffers(&mut self) -> Result<()> {
        if let Some(ref mut p) = self.port {
            p.clear(ClearBuffer::All)?;
        }
        Ok(())
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn set_dtr(&mut self, level: bool) -> Result<()> {
        trace!("Setting DTR to {level}");
        if let Some(ref mut p) = self.port {
            p.write_data_terminal_ready(level)?;
        }
        Ok(())
    }

    fn set_rts(&mut self, level: bool) -> Result<()> {
        trace!("Setting RTS to {level}");
        if let Some(ref mut p) = self.port {
            p.write_request_to_send(level)?;
        }
        Ok(())
    }

    fn read_cts(&mut self) -> Result<bool> {
        if let Some(ref mut p) = self.port {
            p.read_clear_to_send()
                .map_err(Error::Serial)
        } else {
            Err(Error::Serial(serialport::Error::new(
                serialport::ErrorKind::NoDevice,
                "Port is closed",
            )))
        }
    }

    fn read_dsr(&mut self) -> Result<bool> {
        if let Some(ref mut p) = self.port {
            p.read_data_set_ready()
                .map_err(Error::Serial)
        } else {
            Err(Error::Serial(serialport::Error::new(
                serialport::ErrorKind::NoDevice,
                "Port is closed",
            )))
        }
    }

    fn close(&mut self) -> Result<()> {
        // Take ownership of the port and let it drop (close)
        self.port
            .take();
        Ok(())
    }
}

impl Read for NativePort {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.port
            .as_mut()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotConnected, "port closed"))
            .and_then(|p| p.read(buf))
    }
}

impl Write for NativePort {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.port
            .as_mut()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotConnected, "port closed"))
            .and_then(|p| p.write(buf))
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.port
            .as_mut()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotConnected, "port closed"))
            .and_then(std::io::Write::flush)
    }
}

/// Native port enumerator.
pub struct NativePortEnumerator;

impl PortEnumerator for NativePortEnumerator {
    fn list_ports() -> Result<Vec<PortInfo>> {
        let ports = serialport::available_ports().map_err(Error::Serial)?;

        Ok(ports
            .into_iter()
            .map(|p| {
                let (vid, pid, manufacturer, product, serial_number) = match &p.port_type {
                    serialport::SerialPortType::UsbPort(info) => (
                        Some(info.vid),
                        Some(info.pid),
                        info.manufacturer
                            .clone(),
                        info.product
                            .clone(),
                        info.serial_number
                            .clone(),
                    ),
                    _ => (None, None, None, None, None),
                };

                PortInfo {
                    name: p.port_name,
                    vid,
                    pid,
                    manufacturer,
                    product,
                    serial_number,
                }
            })
            .collect())
    }
}

// Type conversions from our types to serialport types

impl From<DataBits> for serialport::DataBits {
    fn from(bits: DataBits) -> Self {
        match bits {
            DataBits::Five => Self::Five,
            DataBits::Six => Self::Six,
            DataBits::Seven => Self::Seven,
            DataBits::Eight => Self::Eight,
        }
    }
}

impl From<Parity> for serialport::Parity {
    fn from(parity: Parity) -> Self {
        match parity {
            Parity::None => Self::None,
            Parity::Odd => Self::Odd,
            Parity::Even => Self::Even,
        }
    }
}

impl From<StopBits> for serialport::StopBits {
    fn from(bits: StopBits) -> Self {
        match bits {
            StopBits::One => Self::One,
            StopBits::Two => Self::Two,
        }
    }
}

impl From<FlowControl> for serialport::FlowControl {
    fn from(flow: FlowControl) -> Self {
        match flow {
            FlowControl::None => Self::None,
            FlowControl::Hardware => Self::Hardware,
            FlowControl::Software => Self::Software,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_ports() {
        // This test just verifies that list_ports doesn't panic
        let _ = NativePortEnumerator::list_ports();
    }

    #[test]
    fn test_serial_config_default() {
        let config = SerialConfig::default();
        assert_eq!(config.baud_rate, 115200);
        assert_eq!(config.data_bits, DataBits::Eight);
        assert_eq!(config.parity, Parity::None);
        assert_eq!(config.stop_bits, StopBits::One);
        assert_eq!(config.flow_control, FlowControl::None);
    }

    #[test]
    fn test_serial_config_builder() {
        let config = SerialConfig::new("/dev/ttyUSB0", 921600).with_timeout(Duration::from_secs(5));

        assert_eq!(config.port_name, "/dev/ttyUSB0");
        assert_eq!(config.baud_rate, 921600);
        assert_eq!(config.timeout, Duration::from_secs(5));
    }
}
