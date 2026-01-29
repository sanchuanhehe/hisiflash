//! Chip/target abstraction for supporting multiple HiSilicon chips.
//!
//! This module provides a trait-based abstraction for different chip families,
//! allowing the same codebase to support WS63, BS2X, and other HiSilicon chips.

use crate::error::{Error, Result};
use crate::image::fwpkg::Fwpkg;
use crate::port::{Port, SerialConfig};
use std::fmt;

/// Supported chip families.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ChipFamily {
    /// WS63 series (WiFi + BLE).
    #[default]
    Ws63,
    /// BS2X series (BS21, BS25, etc. - BLE only).
    Bs2x,
    /// BS25 specific.
    Bs25,
    /// WS53 series.
    Ws53,
    /// SW39 series.
    Sw39,
    /// Generic HiSilicon (unknown specific type).
    Generic,
}

impl ChipFamily {
    /// Get default baud rate for this chip family.
    #[must_use]
    pub fn default_baud(&self) -> u32 {
        // All chips currently use 115200 as default
        115200
    }

    /// Get high-speed baud rate for this chip family.
    #[must_use]
    pub fn high_speed_baud(&self) -> u32 {
        match self {
            Self::Bs2x | Self::Bs25 => 2_000_000,
            _ => 921_600,
        }
    }

    /// Get supported baud rates for this chip family.
    #[must_use]
    pub fn supported_bauds(&self) -> &'static [u32] {
        match self {
            Self::Bs2x | Self::Bs25 => &[115_200, 230_400, 460_800, 921_600, 2_000_000],
            _ => &[115_200, 230_400, 460_800, 921_600],
        }
    }

    /// Check if this chip family supports USB DFU mode.
    pub fn supports_usb_dfu(&self) -> bool {
        matches!(self, Self::Bs2x | Self::Bs25)
    }

    /// Check if this chip family supports eFuse operations.
    pub fn supports_efuse(&self) -> bool {
        true // All HiSilicon chips support eFuse
    }

    /// Check if this chip family requires signed firmware.
    pub fn requires_signed_firmware(&self) -> bool {
        // Some chips require signed firmware for security
        matches!(self, Self::Ws63 | Self::Bs2x | Self::Bs25)
    }

    /// Get the chip family from a string name.
    pub fn from_name(name: &str) -> Option<Self> {
        match name.to_lowercase().as_str() {
            "ws63" => Some(Self::Ws63),
            "bs2x" | "bs21" => Some(Self::Bs2x),
            "bs25" => Some(Self::Bs25),
            "ws53" => Some(Self::Ws53),
            "sw39" => Some(Self::Sw39),
            "generic" | "auto" => Some(Self::Generic),
            _ => None,
        }
    }
}

impl fmt::Display for ChipFamily {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ws63 => write!(f, "WS63"),
            Self::Bs2x => write!(f, "BS2X"),
            Self::Bs25 => write!(f, "BS25"),
            Self::Ws53 => write!(f, "WS53"),
            Self::Sw39 => write!(f, "SW39"),
            Self::Generic => write!(f, "Generic"),
        }
    }
}

/// Chip configuration parameters.
#[derive(Debug, Clone)]
pub struct ChipConfig {
    /// Chip family.
    pub family: ChipFamily,
    /// Initial baud rate for handshake.
    pub init_baud: u32,
    /// Target baud rate for data transfer.
    pub target_baud: u32,
    /// Use late baud rate switch (after loaderboot).
    pub late_baud_switch: bool,
    /// Handshake timeout in seconds.
    pub handshake_timeout_secs: u32,
    /// Data transfer timeout in seconds.
    pub transfer_timeout_secs: u32,
}

impl ChipConfig {
    /// Create a new chip configuration for the given family.
    pub fn new(family: ChipFamily) -> Self {
        Self {
            family,
            init_baud: family.default_baud(),
            target_baud: family.high_speed_baud(),
            late_baud_switch: false,
            handshake_timeout_secs: 30,
            transfer_timeout_secs: 60,
        }
    }

    /// Set the target baud rate.
    #[must_use]
    pub fn with_baud(mut self, baud: u32) -> Self {
        self.target_baud = baud;
        self
    }

    /// Enable late baud rate switching.
    #[must_use]
    pub fn with_late_baud(mut self, late: bool) -> Self {
        self.late_baud_switch = late;
        self
    }

    /// Set handshake timeout.
    #[must_use]
    pub fn with_handshake_timeout(mut self, secs: u32) -> Self {
        self.handshake_timeout_secs = secs;
        self
    }
}

impl Default for ChipConfig {
    fn default() -> Self {
        Self::new(ChipFamily::default())
    }
}

/// Trait for flashing operations across all chip families.
///
/// This trait provides a unified interface for flashing firmware,
/// allowing the CLI to work with any chip family through a common API.
pub trait Flasher {
    /// Connect to the device and perform handshake.
    fn connect(&mut self) -> Result<()>;

    /// Flash a complete FWPKG firmware package.
    ///
    /// # Arguments
    ///
    /// * `fwpkg` - The firmware package to flash
    /// * `filter` - Optional filter for partition names (None = flash all)
    /// * `progress` - Progress callback (partition_name, current_bytes, total_bytes)
    fn flash_fwpkg(
        &mut self,
        fwpkg: &Fwpkg,
        filter: Option<&[&str]>,
        progress: &mut dyn FnMut(&str, usize, usize),
    ) -> Result<()>;

    /// Flash raw binary files.
    fn write_bins(&mut self, loaderboot: &[u8], bins: &[(&[u8], u32)]) -> Result<()>;

    /// Erase entire flash.
    fn erase_all(&mut self) -> Result<()>;

    /// Reset the device.
    fn reset(&mut self) -> Result<()>;

    /// Get the connection baud rate.
    fn connection_baud(&self) -> u32;

    /// Get the target transfer baud rate (if different from connection).
    fn target_baud(&self) -> Option<u32>;

    /// Close the flasher and release resources.
    ///
    /// This method ensures the serial port is properly closed.
    /// It is safe to call even if the connection is not active.
    /// After calling this method, the flasher cannot be used.
    fn close(&mut self);
}

impl ChipFamily {
    /// Create a flasher instance for this chip family (native platforms).
    ///
    /// This is the main entry point for creating chip-specific flashers.
    ///
    /// # Arguments
    ///
    /// * `port_name` - Serial port name (e.g., "/dev/ttyUSB0")
    /// * `target_baud` - Target baud rate for data transfer
    /// * `late_baud` - Use late baud rate switch (after LoaderBoot)
    /// * `verbose` - Verbose output level
    ///
    /// # Returns
    ///
    /// A boxed flasher instance implementing the `Flasher` trait
    #[cfg(feature = "native")]
    pub fn create_flasher(
        &self,
        port_name: &str,
        target_baud: u32,
        late_baud: bool,
        verbose: u8,
    ) -> Result<Box<dyn Flasher>> {
        match self {
            Self::Ws63 => {
                // Ws63Flasher struct implements the Flasher trait
                let flasher = super::ws63::flasher::Ws63Flasher::open(port_name, target_baud)?
                    .with_late_baud(late_baud)
                    .with_verbose(verbose);
                Ok(Box::new(flasher))
            },
            Self::Bs2x | Self::Bs25 => {
                Err(Error::Unsupported("BS2X series support coming soon".into()))
            },
            Self::Ws53 | Self::Sw39 => Err(Error::Unsupported(format!(
                "{self} series support coming soon"
            ))),
            Self::Generic => Err(Error::Unsupported(
                "Cannot create flasher for generic chip family".into(),
            )),
        }
    }

    /// Create a flasher with an existing port (generic, works for any Port type).
    ///
    /// This is useful for testing or custom port implementations.
    #[cfg(feature = "native")]
    pub fn create_flasher_with_port<P: Port + 'static>(
        &self,
        port: P,
        target_baud: u32,
        late_baud: bool,
        verbose: u8,
    ) -> Result<Box<dyn Flasher>> {
        match self {
            Self::Ws63 => {
                let flasher = super::ws63::flasher::Ws63Flasher::new(port, target_baud)
                    .with_late_baud(late_baud)
                    .with_verbose(verbose);
                Ok(Box::new(flasher))
            },
            _ => Err(Error::Unsupported(format!(
                "Unsupported chip family for generic port: {self}"
            ))),
        }
    }

    /// Create a flasher with full serial configuration (P0: 完整配置支持).
    ///
    /// This allows customization of all serial port parameters including
    /// baud rate, data bits, parity, stop bits, and flow control.
    ///
    /// # Arguments
    ///
    /// * `config` - Serial port configuration
    /// * `late_baud` - Use late baud rate switch (after LoaderBoot)
    /// * `verbose` - Verbose output level
    ///
    /// # Returns
    ///
    /// A boxed flasher instance implementing the `Flasher` trait
    #[cfg(feature = "native")]
    pub fn create_flasher_with_config(
        &self,
        config: SerialConfig,
        late_baud: bool,
        verbose: u8,
    ) -> Result<Box<dyn Flasher>> {
        match self {
            Self::Ws63 => {
                let flasher =
                    super::ws63::flasher::Ws63Flasher::open_with_config(config)?
                        .with_late_baud(late_baud)
                        .with_verbose(verbose);
                Ok(Box::new(flasher))
            }
            Self::Bs2x | Self::Bs25 => {
                Err(Error::Unsupported("BS2X series support coming soon".into()))
            }
            Self::Ws53 | Self::Sw39 => Err(Error::Unsupported(format!(
                "{self} series support coming soon"
            ))),
            Self::Generic => Err(Error::Unsupported(
                "Cannot create flasher for generic chip family".into(),
            )),
        }
    }
}

/// Trait for chip-specific implementations.
///
/// This trait allows different chip families to have custom behavior
/// while sharing common flashing logic.
pub trait ChipOps {
    /// Get the chip family.
    fn family(&self) -> ChipFamily;

    /// Get the chip configuration.
    fn config(&self) -> &ChipConfig;

    /// Prepare a binary for flashing (e.g., add signing header).
    fn prepare_binary(&self, data: &[u8], _addr: u32) -> Result<Vec<u8>> {
        // Default: return data unchanged
        Ok(data.to_vec())
    }

    /// Check if a binary needs signing.
    fn needs_signing(&self, _addr: u32) -> bool {
        false
    }

    /// Get the flash base address for this chip.
    fn flash_base(&self) -> u32 {
        0x00000000
    }

    /// Get the maximum flash size for this chip.
    fn flash_size(&self) -> u32 {
        0x00800000 // 8MB default
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chip_family_from_name() {
        assert_eq!(ChipFamily::from_name("ws63"), Some(ChipFamily::Ws63));
        assert_eq!(ChipFamily::from_name("BS2X"), Some(ChipFamily::Bs2x));
        assert_eq!(ChipFamily::from_name("unknown"), None);
    }

    #[test]
    fn test_chip_config_defaults() {
        let config = ChipConfig::new(ChipFamily::Ws63);
        assert_eq!(config.init_baud, 115200);
        assert_eq!(config.target_baud, 921600);
    }
}
