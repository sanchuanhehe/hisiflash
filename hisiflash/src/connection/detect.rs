//! USB device detection and serial port auto-discovery.
//!
//! This module provides automatic serial port detection based on USB VID/PID,
//! similar to how esptool and espflash work for ESP chips.
//!
//! ## Supported Devices
//!
//! HiSilicon chips typically use these USB-to-UART bridges:
//! - CH340/CH341 (VID: 0x1A86, PID: 0x7523)
//! - CP210x (VID: 0x10C6, PID: 0xEA60)
//! - FTDI (VID: 0x0403, PID: 0x6001/0x6010/0x6011/0x6014/0x6015)
//! - HiSilicon native USB (VID: 0x12D1, various PIDs)
//!
//! ## Example
//!
//! ```rust,no_run
//! use hisiflash::connection::detect::{detect_ports, UsbDevice};
//!
//! let ports = detect_ports();
//! for port in ports {
//!     println!("Found: {} ({:?})", port.name, port.device);
//! }
//! ```

use crate::error::{Error, Result};
use log::{debug, info, trace};

/// Known USB VID/PID combinations for HiSilicon development boards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsbDevice {
    /// CH340/CH341 USB-to-Serial converter.
    Ch340,
    /// Silicon Labs CP210x USB-to-Serial converter.
    Cp210x,
    /// FTDI FT232/FT2232/FT4232 USB-to-Serial converter.
    Ftdi,
    /// HiSilicon native USB device.
    HiSilicon,
    /// Unknown device.
    Unknown,
}

impl UsbDevice {
    /// Check if this VID/PID combination is a known HiSilicon-compatible device.
    #[must_use]
    pub fn from_vid_pid(vid: u16, _pid: u16) -> Self {
        match vid {
            // CH340/CH341 family
            0x1A86 => Self::Ch340,
            // Silicon Labs CP210x family
            0x10C4 => Self::Cp210x,
            // FTDI family
            0x0403 => Self::Ftdi,
            // HiSilicon native
            0x12D1 => Self::HiSilicon,
            _ => Self::Unknown,
        }
    }

    /// Get a human-readable name for the device.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Ch340 => "CH340/CH341",
            Self::Cp210x => "CP210x",
            Self::Ftdi => "FTDI",
            Self::HiSilicon => "HiSilicon",
            Self::Unknown => "Unknown",
        }
    }

    /// Check if this is a known/expected device type.
    pub fn is_known(&self) -> bool {
        !matches!(self, Self::Unknown)
    }
}

/// Detected serial port information.
#[derive(Debug, Clone)]
pub struct DetectedPort {
    /// Port name/path (e.g., "/dev/ttyUSB0" or "COM3").
    pub name: String,
    /// USB device type if detected.
    pub device: UsbDevice,
    /// USB Vendor ID (if available).
    pub vid: Option<u16>,
    /// USB Product ID (if available).
    pub pid: Option<u16>,
    /// Device manufacturer string (if available).
    pub manufacturer: Option<String>,
    /// Device product string (if available).
    pub product: Option<String>,
    /// Serial number (if available).
    pub serial: Option<String>,
}

impl DetectedPort {
    /// Check if this port is likely a HiSilicon development board.
    pub fn is_likely_hisilicon(&self) -> bool {
        self.device.is_known()
    }
}

/// Detect all available serial ports with USB device information.
pub fn detect_ports() -> Vec<DetectedPort> {
    let mut result = Vec::new();

    match serialport::available_ports() {
        Ok(ports) => {
            for port_info in ports {
                let mut detected = DetectedPort {
                    name: port_info.port_name.clone(),
                    device: UsbDevice::Unknown,
                    vid: None,
                    pid: None,
                    manufacturer: None,
                    product: None,
                    serial: None,
                };

                // Extract USB info if available
                if let serialport::SerialPortType::UsbPort(usb_info) = port_info.port_type {
                    detected.vid = Some(usb_info.vid);
                    detected.pid = Some(usb_info.pid);
                    detected.manufacturer = usb_info.manufacturer;
                    detected.product = usb_info.product;
                    detected.serial = usb_info.serial_number;
                    detected.device = UsbDevice::from_vid_pid(usb_info.vid, usb_info.pid);

                    trace!(
                        "Found USB port: {} (VID: {:04X}, PID: {:04X}, Device: {:?})",
                        port_info.port_name, usb_info.vid, usb_info.pid, detected.device
                    );
                }

                result.push(detected);
            }
        },
        Err(e) => {
            debug!("Failed to enumerate serial ports: {e}");
        },
    }

    result
}

/// Detect ports that are likely HiSilicon development boards.
pub fn detect_hisilicon_ports() -> Vec<DetectedPort> {
    detect_ports()
        .into_iter()
        .filter(DetectedPort::is_likely_hisilicon)
        .collect()
}

/// Auto-detect a single HiSilicon port.
///
/// Returns the first port that matches a known USB device type.
/// Prioritizes HiSilicon native USB over generic USB-UART bridges.
pub fn auto_detect_port() -> Result<DetectedPort> {
    let ports = detect_ports();

    // First, look for HiSilicon native USB
    if let Some(port) = ports.iter().find(|p| p.device == UsbDevice::HiSilicon) {
        info!("Auto-detected HiSilicon USB device: {}", port.name);
        return Ok(port.clone());
    }

    // Then, look for known USB-UART bridges
    if let Some(port) = ports.iter().find(|p| p.device.is_known()) {
        info!(
            "Auto-detected {} USB-UART bridge: {}",
            port.device.name(),
            port.name
        );
        return Ok(port.clone());
    }

    // Finally, return any available port
    if let Some(port) = ports.into_iter().next() {
        info!("Using first available port: {}", port.name);
        return Ok(port);
    }

    Err(Error::DeviceNotFound)
}

/// Find a port by name pattern.
pub fn find_port_by_pattern(pattern: &str) -> Result<DetectedPort> {
    let ports = detect_ports();

    ports
        .into_iter()
        .find(|p| p.name.contains(pattern))
        .ok_or(Error::DeviceNotFound)
}

/// List all serial ports in a user-friendly format.
pub fn list_ports_pretty() -> Vec<String> {
    let ports = detect_ports();
    let mut result = Vec::new();

    for port in ports {
        let device_info = if port.device.is_known() {
            format!(" [{}]", port.device.name())
        } else if let (Some(vid), Some(pid)) = (port.vid, port.pid) {
            format!(" [VID:{vid:04X} PID:{pid:04X}]")
        } else {
            String::new()
        };

        let product_info = port
            .product
            .as_ref()
            .map(|p| format!(" - {p}"))
            .unwrap_or_default();

        result.push(format!("{}{}{}", port.name, device_info, product_info));
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_usb_device_from_vid_pid() {
        assert_eq!(UsbDevice::from_vid_pid(0x1A86, 0x7523), UsbDevice::Ch340);
        assert_eq!(UsbDevice::from_vid_pid(0x10C4, 0xEA60), UsbDevice::Cp210x);
        assert_eq!(UsbDevice::from_vid_pid(0x0403, 0x6001), UsbDevice::Ftdi);
        assert_eq!(
            UsbDevice::from_vid_pid(0x12D1, 0x1234),
            UsbDevice::HiSilicon
        );
        assert_eq!(UsbDevice::from_vid_pid(0x0000, 0x0000), UsbDevice::Unknown);
    }

    #[test]
    fn test_usb_device_is_known() {
        assert!(UsbDevice::Ch340.is_known());
        assert!(UsbDevice::Cp210x.is_known());
        assert!(UsbDevice::Ftdi.is_known());
        assert!(UsbDevice::HiSilicon.is_known());
        assert!(!UsbDevice::Unknown.is_known());
    }

    #[test]
    fn test_detect_ports_does_not_panic() {
        // Just make sure it doesn't panic
        let _ = detect_ports();
    }
}
