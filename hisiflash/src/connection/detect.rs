//! USB device detection and serial port auto-discovery.
//!
//! This module provides automatic serial port detection based on USB VID/PID,
//! similar to how esptool and espflash work for ESP chips.
//!
//! ## Supported USB-to-UART Bridges
//!
//! Common USB-to-UART bridge chips used with HiSilicon development boards:
//!
//! | Chip | VID | PID(s) | Notes |
//! |------|-----|--------|-------|
//! | CH340/CH341 | 0x1A86 | 0x7523, 0x7522, 0x5523, 0x5512, 0x55D4 | Most common |
//! | CP210x | 0x10C4 | 0xEA60, 0xEA70, 0xEA71, 0xEA63 | Silicon Labs |
//! | FTDI FT232 | 0x0403 | 0x6001 | Single channel |
//! | FTDI FT2232 | 0x0403 | 0x6010 | Dual channel |
//! | FTDI FT4232 | 0x0403 | 0x6011 | Quad channel |
//! | FTDI FT232H | 0x0403 | 0x6014 | High speed |
//! | FTDI FT231X | 0x0403 | 0x6015 | |
//! | Prolific PL2303 | 0x067B | 0x2303, 0x23A3, 0x23C3, 0x23D3 | |
//! | HiSilicon native | 0x12D1 | various | Direct USB |
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
    /// Prolific PL2303 USB-to-Serial converter.
    Prolific,
    /// HiSilicon native USB device.
    HiSilicon,
    /// Unknown device.
    Unknown,
}

/// Known USB VID/PID pairs for common USB-to-UART bridges.
/// This list is referenced from esptool and espflash projects.
const KNOWN_USB_DEVICES: &[(u16, &[u16], UsbDevice)] = &[
    // CH340/CH341 family (WCH)
    // https://github.com/WCHSoftGroup
    (0x1A86, &[0x7523, 0x7522, 0x5523, 0x5512, 0x55D4], UsbDevice::Ch340),
    
    // Silicon Labs CP210x family
    // https://www.silabs.com/developers/usb-to-uart-bridge-vcp-drivers
    (0x10C4, &[0xEA60, 0xEA70, 0xEA71, 0xEA63], UsbDevice::Cp210x),
    
    // FTDI family
    // https://ftdichip.com/drivers/vcp-drivers/
    (0x0403, &[
        0x6001, // FT232R
        0x6010, // FT2232
        0x6011, // FT4232
        0x6014, // FT232H
        0x6015, // FT231X
    ], UsbDevice::Ftdi),
    
    // Prolific PL2303 family
    // https://www.prolific.com.tw/US/index.aspx
    (0x067B, &[0x2303, 0x23A3, 0x23C3, 0x23D3], UsbDevice::Prolific),
    
    // HiSilicon native USB
    (0x12D1, &[], UsbDevice::HiSilicon), // Empty PID list = match all PIDs
];

impl UsbDevice {
    /// Check if this VID/PID combination is a known HiSilicon-compatible device.
    #[must_use]
    pub fn from_vid_pid(vid: u16, pid: u16) -> Self {
        for (known_vid, pids, device) in KNOWN_USB_DEVICES {
            if vid == *known_vid {
                // If PID list is empty, match any PID for this VID
                if pids.is_empty() || pids.contains(&pid) {
                    return *device;
                }
            }
        }
        Self::Unknown
    }

    /// Get a human-readable name for the device.
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

    /// Check if this is a known/expected device type.
    pub fn is_known(&self) -> bool {
        !matches!(self, Self::Unknown)
    }
    
    /// Check if this device is likely to be on a HiSilicon development board.
    /// This gives higher priority to certain device types during auto-detection.
    pub fn is_high_priority(&self) -> bool {
        matches!(self, Self::HiSilicon | Self::Ch340 | Self::Cp210x)
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

    // Then, look for high-priority USB-UART bridges (CH340, CP210x)
    if let Some(port) = ports.iter().find(|p| p.device.is_high_priority()) {
        info!(
            "Auto-detected {} USB-UART bridge: {}",
            port.device.name(),
            port.name
        );
        return Ok(port.clone());
    }
    
    // Then, look for other known USB-UART bridges
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

/// Format a list of detected ports for display.
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

/// List all serial ports in a user-friendly format.
pub fn list_ports_pretty() -> Vec<String> {
    let ports = detect_ports();
    format_port_list(&ports)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_usb_device_from_vid_pid() {
        // CH340 variants
        assert_eq!(UsbDevice::from_vid_pid(0x1A86, 0x7523), UsbDevice::Ch340);
        assert_eq!(UsbDevice::from_vid_pid(0x1A86, 0x7522), UsbDevice::Ch340);
        assert_eq!(UsbDevice::from_vid_pid(0x1A86, 0x5523), UsbDevice::Ch340);
        assert_eq!(UsbDevice::from_vid_pid(0x1A86, 0x5512), UsbDevice::Ch340);
        assert_eq!(UsbDevice::from_vid_pid(0x1A86, 0x55D4), UsbDevice::Ch340);
        
        // CP210x variants
        assert_eq!(UsbDevice::from_vid_pid(0x10C4, 0xEA60), UsbDevice::Cp210x);
        assert_eq!(UsbDevice::from_vid_pid(0x10C4, 0xEA70), UsbDevice::Cp210x);
        assert_eq!(UsbDevice::from_vid_pid(0x10C4, 0xEA71), UsbDevice::Cp210x);
        assert_eq!(UsbDevice::from_vid_pid(0x10C4, 0xEA63), UsbDevice::Cp210x);
        
        // FTDI variants
        assert_eq!(UsbDevice::from_vid_pid(0x0403, 0x6001), UsbDevice::Ftdi);
        assert_eq!(UsbDevice::from_vid_pid(0x0403, 0x6010), UsbDevice::Ftdi);
        assert_eq!(UsbDevice::from_vid_pid(0x0403, 0x6011), UsbDevice::Ftdi);
        assert_eq!(UsbDevice::from_vid_pid(0x0403, 0x6014), UsbDevice::Ftdi);
        assert_eq!(UsbDevice::from_vid_pid(0x0403, 0x6015), UsbDevice::Ftdi);
        
        // Prolific variants
        assert_eq!(UsbDevice::from_vid_pid(0x067B, 0x2303), UsbDevice::Prolific);
        assert_eq!(UsbDevice::from_vid_pid(0x067B, 0x23A3), UsbDevice::Prolific);
        assert_eq!(UsbDevice::from_vid_pid(0x067B, 0x23C3), UsbDevice::Prolific);
        assert_eq!(UsbDevice::from_vid_pid(0x067B, 0x23D3), UsbDevice::Prolific);
        
        // HiSilicon (any PID should match)
        assert_eq!(UsbDevice::from_vid_pid(0x12D1, 0x1234), UsbDevice::HiSilicon);
        assert_eq!(UsbDevice::from_vid_pid(0x12D1, 0x0000), UsbDevice::HiSilicon);
        assert_eq!(UsbDevice::from_vid_pid(0x12D1, 0xFFFF), UsbDevice::HiSilicon);
        
        // Unknown devices
        assert_eq!(UsbDevice::from_vid_pid(0x0000, 0x0000), UsbDevice::Unknown);
        assert_eq!(UsbDevice::from_vid_pid(0x1234, 0x5678), UsbDevice::Unknown);
        assert_eq!(UsbDevice::from_vid_pid(0xFFFF, 0xFFFF), UsbDevice::Unknown);
        // Unknown PID for known VID (not in PID list)
        assert_eq!(UsbDevice::from_vid_pid(0x1A86, 0x1234), UsbDevice::Unknown);
        assert_eq!(UsbDevice::from_vid_pid(0x10C4, 0x0000), UsbDevice::Unknown);
    }

    #[test]
    fn test_usb_device_is_known() {
        assert!(UsbDevice::Ch340.is_known());
        assert!(UsbDevice::Cp210x.is_known());
        assert!(UsbDevice::Ftdi.is_known());
        assert!(UsbDevice::Prolific.is_known());
        assert!(UsbDevice::HiSilicon.is_known());
        assert!(!UsbDevice::Unknown.is_known());
    }
    
    #[test]
    fn test_usb_device_is_high_priority() {
        assert!(UsbDevice::HiSilicon.is_high_priority());
        assert!(UsbDevice::Ch340.is_high_priority());
        assert!(UsbDevice::Cp210x.is_high_priority());
        assert!(!UsbDevice::Ftdi.is_high_priority());
        assert!(!UsbDevice::Prolific.is_high_priority());
        assert!(!UsbDevice::Unknown.is_high_priority());
    }
    
    #[test]
    fn test_usb_device_name() {
        assert_eq!(UsbDevice::Ch340.name(), "CH340/CH341");
        assert_eq!(UsbDevice::Cp210x.name(), "CP210x");
        assert_eq!(UsbDevice::Ftdi.name(), "FTDI");
        assert_eq!(UsbDevice::Prolific.name(), "PL2303");
        assert_eq!(UsbDevice::HiSilicon.name(), "HiSilicon");
        assert_eq!(UsbDevice::Unknown.name(), "Unknown");
    }
    
    #[test]
    fn test_detected_port_is_likely_hisilicon() {
        let port_known = DetectedPort {
            name: "/dev/ttyUSB0".to_string(),
            device: UsbDevice::Ch340,
            vid: Some(0x1A86),
            pid: Some(0x7523),
            manufacturer: None,
            product: None,
            serial: None,
        };
        assert!(port_known.is_likely_hisilicon());
        
        let port_unknown = DetectedPort {
            name: "/dev/ttyS0".to_string(),
            device: UsbDevice::Unknown,
            vid: None,
            pid: None,
            manufacturer: None,
            product: None,
            serial: None,
        };
        assert!(!port_unknown.is_likely_hisilicon());
    }

    #[test]
    fn test_detect_ports_does_not_panic() {
        // Just make sure it doesn't panic and returns a valid result
        let ports = detect_ports();
        // Verify it's a valid vector - the actual length depends on system
        let _ = ports.len();
    }
    
    #[test]
    fn test_detect_hisilicon_ports() {
        // Should not panic and return filtered results
        let ports = detect_hisilicon_ports();
        // All returned ports should be known devices
        for port in &ports {
            assert!(port.device.is_known());
        }
    }
    
    #[test]
    fn test_auto_detect_port_does_not_panic() {
        // Should not panic even if no ports found
        let _ = auto_detect_port();
    }
    
    #[test]
    fn test_format_port_list() {
        let ports = vec![
            DetectedPort {
                name: "/dev/ttyUSB0".to_string(),
                device: UsbDevice::Ch340,
                vid: Some(0x1A86),
                pid: Some(0x7523),
                manufacturer: Some("WCH".to_string()),
                product: Some("USB-Serial".to_string()),
                serial: None,
            },
            DetectedPort {
                name: "/dev/ttyUSB1".to_string(),
                device: UsbDevice::Unknown,
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
