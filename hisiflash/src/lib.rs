//! # hisiflash
//!
//! A library for flashing HiSilicon chips.
//!
//! This crate provides the core functionality for communicating with HiSilicon
//! chips via serial port, including:
//!
//! - FWPKG firmware package parsing
//! - WS63 protocol implementation
//! - YMODEM file transfer
//! - CRC16-XMODEM checksum calculation
//!
//! ## Supported Chips
//!
//! - WS63 (primary support)
//! - More chips coming in future releases
//!
//! ## Supported Platforms
//!
//! - **Native** (default): Linux, macOS, Windows via the `serialport` crate
//! - **WASM** (experimental): Web browsers via the Web Serial API
//!
//! ## Features
//!
//! - `native` (default): Native serial port support
//! - `wasm`: WASM/Web Serial API support (experimental)
//! - `serde`: Serialization support for data types
//!
//! ## Example
//!
//! ```rust,no_run
//! use hisiflash::{Ws63Flasher, Fwpkg};
//!
//! fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Parse firmware package
//!     let fwpkg = Fwpkg::from_file("firmware.fwpkg")?;
//!     
//!     // Create flasher and connect (native only)
//!     #[cfg(feature = "native")]
//!     {
//!         let mut flasher = Ws63Flasher::open("/dev/ttyUSB0", 921600)?;
//!         flasher.connect()?;
//!         
//!         // Flash the firmware
//!         flasher.flash_fwpkg(&fwpkg, None, |name, current, total| {
//!             println!("Flashing {}: {}/{}", name, current, total);
//!         })?;
//!         
//!         // Reset the device
//!         flasher.reset()?;
//!     }
//!     
//!     Ok(())
//! }
//! ```

#![warn(missing_docs)]
#![warn(clippy::all)]

pub mod connection;
pub mod error;
pub mod image;
pub mod port;
pub mod protocol;
pub mod target;

// Re-exports for convenience
pub use error::{Error, Result};
pub use image::fwpkg::{Fwpkg, FwpkgBinInfo, FwpkgHeader, FwpkgVersion, PartitionType};
pub use port::{Port, PortEnumerator, PortInfo, SerialConfig};
pub use protocol::seboot::{
    CommandType, ImageType, SebootAck, SebootFrame, contains_handshake_ack,
};
pub use target::ws63::flasher::Ws63Flasher;
pub use target::{ChipConfig, ChipFamily, ChipOps};

// Native-specific re-exports
#[cfg(feature = "native")]
pub use port::{NativePort, NativePortEnumerator};

// Legacy re-exports for backward compatibility
pub use connection::ConnectionPort;
pub use connection::detect::{DetectedPort, UsbDevice};
#[cfg(feature = "native")]
pub use connection::serial::SerialPort;
