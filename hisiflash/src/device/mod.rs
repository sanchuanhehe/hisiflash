//! Device discovery and classification utilities.
//!
//! This module provides transport-agnostic device discovery primitives.
//! Currently, native discovery is serial-port based, but the data model is
//! designed to support future transports (TCP, BLE, USB-HID, etc.).

use crate::error::{Error, Result};

#[cfg(feature = "native")]
use log::{debug, info, trace};

/// Transport type for discovered endpoints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportKind {
    /// Serial transport (UART/USB CDC).
    Serial,
    /// Unknown or unclassified transport.
    Unknown,
}

/// Known USB bridge/device kinds commonly used with HiSilicon boards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceKind {
    /// CH340/CH341 USB-to-Serial converter.
    Ch340,
    /// Silicon Labs CP210x USB-to-Serial converter.
    Cp210x,
    /// FTDI FT232/FT2232/FT4232 USB-to-Serial converter.
    Ftdi,
    /// Prolific PL2303 USB-to-Serial converter.
    Prolific,
    /// HiSilicon native USB device.
    HiSilicon,
    /// Unknown device.
    Unknown,
}

/// Legacy type alias kept for compatibility inside this release line.
pub type UsbDevice = DeviceKind;

/// Known USB VID/PID pairs for common USB-to-UART bridges.
const KNOWN_USB_DEVICES: &[(u16, &[u16], DeviceKind)] = &[
    (
        0x1A86,
        &[0x7523, 0x7522, 0x5523, 0x5512, 0x55D4],
        DeviceKind::Ch340,
    ),
    (0x10C4, &[0xEA60, 0xEA70, 0xEA71, 0xEA63], DeviceKind::Cp210x),
    (
        0x0403,
        &[0x6001, 0x6010, 0x6011, 0x6014, 0x6015],
        DeviceKind::Ftdi,
    ),
    (0x067B, &[0x2303, 0x23A3, 0x23C3, 0x23D3], DeviceKind::Prolific),
    (0x12D1, &[], DeviceKind::HiSilicon),
];

impl DeviceKind {
    /// Check if this VID/PID combination is a known HiSilicon-compatible device.
    #[must_use]
    pub fn from_vid_pid(vid: u16, pid: u16) -> Self {
        for (known_vid, pids, device) in KNOWN_USB_DEVICES {
            if vid == *known_vid && (pids.is_empty() || pids.contains(&pid)) {
                return *device;
            }
        }
        Self::Unknown
    }

    /// Get a human-readable name for the device kind.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Ch340 => "CH340/CH341",
            Self::Cp210x => "CP210x",
            Self::Ftdi => "FTDI",
            Self::Prolific => "PL2303",
            Self::HiSilicon => "HiSilicon",
            Self::Unknown => "Unknown",
        }
    }

    /// Check if this is a known/expected device kind.
    pub fn is_known(&self) -> bool {
        !matches!(self, Self::Unknown)
    }

    /// Check if this device kind should be preferred during auto-selection.
    pub fn is_high_priority(&self) -> bool {
        matches!(self, Self::HiSilicon | Self::Ch340 | Self::Cp210x)
    }
}

/// Discovered device endpoint information.
#[derive(Debug, Clone)]
pub struct DetectedPort {
    /// Endpoint name/path (e.g., "/dev/ttyUSB0" or "COM3").
    pub name: String,
    /// Transport type.
    pub transport: TransportKind,
    /// Classified device kind.
    pub device: DeviceKind,
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
    /// Check if this endpoint is likely a HiSilicon development board.
    pub fn is_likely_hisilicon(&self) -> bool {
        self.device.is_known()
    }
}

/// Detect all available endpoints with metadata.
#[cfg(feature = "native")]
pub fn detect_ports() -> Vec<DetectedPort> {
    let mut result = Vec::new();

    match serialport::available_ports() {
        Ok(ports) => {
            for port_info in ports {
                let mut detected = DetectedPort {
                    name: port_info.port_name.clone(),
                    transport: TransportKind::Serial,
                    device: DeviceKind::Unknown,
                    vid: None,
                    pid: None,
                    manufacturer: None,
                    product: None,
                    serial: None,
                };

                if let serialport::SerialPortType::UsbPort(usb_info) = port_info.port_type {
                    detected.vid = Some(usb_info.vid);
                    detected.pid = Some(usb_info.pid);
                    detected.manufacturer = usb_info.manufacturer;
                    detected.product = usb_info.product;
                    detected.serial = usb_info.serial_number;
                    detected.device = DeviceKind::from_vid_pid(usb_info.vid, usb_info.pid);

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

/// Detect all available endpoints (WASM stub - always returns empty).
#[cfg(not(feature = "native"))]
pub fn detect_ports() -> Vec<DetectedPort> {
    Vec::new()
}

/// Detect endpoints that are likely HiSilicon development boards.
pub fn detect_hisilicon_ports() -> Vec<DetectedPort> {
    detect_ports()
        .into_iter()
        .filter(DetectedPort::is_likely_hisilicon)
        .collect()
}

/// Auto-detect a single HiSilicon endpoint.
#[cfg(feature = "native")]
pub fn auto_detect_port() -> Result<DetectedPort> {
    let ports = detect_ports();

    if let Some(port) = ports.iter().find(|p| p.device == DeviceKind::HiSilicon) {
        info!("Auto-detected HiSilicon USB device: {}", port.name);
        return Ok(port.clone());
    }

    if let Some(port) = ports.iter().find(|p| p.device.is_high_priority()) {
        info!(
            "Auto-detected {} USB-UART bridge: {}",
            port.device.name(),
            port.name
        );
        return Ok(port.clone());
    }

    if let Some(port) = ports.iter().find(|p| p.device.is_known()) {
        info!(
            "Auto-detected {} USB-UART bridge: {}",
            port.device.name(),
            port.name
        );
        return Ok(port.clone());
    }

    if let Some(port) = ports.into_iter().next() {
        info!("Using first available port: {}", port.name);
        return Ok(port);
    }

    Err(Error::DeviceNotFound)
}

/// Auto-detect a single HiSilicon endpoint (WASM stub - not supported).
#[cfg(not(feature = "native"))]
pub fn auto_detect_port() -> Result<DetectedPort> {
    Err(Error::Unsupported(
        "Auto-detection is not available in WASM. Use the Web Serial API to request a port."
            .to_string(),
    ))
}

/// Find an endpoint by name pattern.
#[cfg(feature = "native")]
pub fn find_port_by_pattern(pattern: &str) -> Result<DetectedPort> {
    let ports = detect_ports();

    ports
        .into_iter()
        .find(|p| p.name.contains(pattern))
        .ok_or(Error::DeviceNotFound)
}

/// Find an endpoint by name pattern (WASM stub - not supported).
#[cfg(not(feature = "native"))]
pub fn find_port_by_pattern(_pattern: &str) -> Result<DetectedPort> {
    Err(Error::Unsupported(
        "Port enumeration is not available in WASM. Use the Web Serial API to request a port."
            .to_string(),
    ))
}

/// Format a list of detected endpoints for display.
pub fn format_port_list(ports: &[DetectedPort]) -> Vec<String> {
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

/// List all endpoints in a user-friendly format.
pub fn list_ports_pretty() -> Vec<String> {
    let ports = detect_ports();
    format_port_list(&ports)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_kind_from_vid_pid() {
        assert_eq!(DeviceKind::from_vid_pid(0x1A86, 0x7523), DeviceKind::Ch340);
        assert_eq!(DeviceKind::from_vid_pid(0x10C4, 0xEA60), DeviceKind::Cp210x);
        assert_eq!(DeviceKind::from_vid_pid(0x0403, 0x6001), DeviceKind::Ftdi);
        assert_eq!(DeviceKind::from_vid_pid(0x067B, 0x2303), DeviceKind::Prolific);
        assert_eq!(DeviceKind::from_vid_pid(0x12D1, 0x1234), DeviceKind::HiSilicon);
        assert_eq!(DeviceKind::from_vid_pid(0x1234, 0x5678), DeviceKind::Unknown);
    }

    #[test]
    fn test_device_kind_is_known() {
        assert!(DeviceKind::Ch340.is_known());
        assert!(!DeviceKind::Unknown.is_known());
    }

    #[test]
    fn test_detected_port_is_likely_hisilicon() {
        let known = DetectedPort {
            name: "/dev/ttyUSB0".to_string(),
            transport: TransportKind::Serial,
            device: DeviceKind::Ch340,
            vid: Some(0x1A86),
            pid: Some(0x7523),
            manufacturer: None,
            product: None,
            serial: None,
        };
        assert!(known.is_likely_hisilicon());

        let unknown = DetectedPort {
            name: "/dev/ttyS0".to_string(),
            transport: TransportKind::Serial,
            device: DeviceKind::Unknown,
            vid: None,
            pid: None,
            manufacturer: None,
            product: None,
            serial: None,
        };
        assert!(!unknown.is_likely_hisilicon());
    }

    #[test]
    fn test_format_port_list() {
        let ports = vec![
            DetectedPort {
                name: "/dev/ttyUSB0".to_string(),
                transport: TransportKind::Serial,
                device: DeviceKind::Ch340,
                vid: Some(0x1A86),
                pid: Some(0x7523),
                manufacturer: Some("WCH".to_string()),
                product: Some("USB-Serial".to_string()),
                serial: None,
            },
            DetectedPort {
                name: "/dev/ttyUSB1".to_string(),
                transport: TransportKind::Serial,
                device: DeviceKind::Unknown,
                vid: None,
                pid: None,
                manufacturer: None,
                product: None,
                serial: None,
            },
        ];

        let formatted = format_port_list(&ports);
        assert_eq!(formatted.len(), 2);
        assert!(formatted[0].contains("/dev/ttyUSB0"));
        assert!(formatted[0].contains("CH340/CH341"));
        assert!(formatted[1].contains("/dev/ttyUSB1"));
    }
}