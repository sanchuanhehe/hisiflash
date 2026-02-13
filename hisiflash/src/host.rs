//! Host-side utilities for serial port discovery.

use crate::device::DetectedPort;

/// Discover all available serial ports.
#[must_use]
pub fn discover_ports() -> Vec<DetectedPort> {
    crate::device::detect_ports()
}

/// Discover serial ports that are likely HiSilicon devices.
#[must_use]
pub fn discover_hisilicon_ports() -> Vec<DetectedPort> {
    crate::device::detect_hisilicon_ports()
}

/// Auto-detect a single best serial port candidate.
pub fn auto_detect_port() -> crate::Result<DetectedPort> {
    crate::device::auto_detect_port()
}
