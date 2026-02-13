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
//! use hisiflash::{ChipFamily, Fwpkg};
//!
//! fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Parse firmware package
//!     let fwpkg = Fwpkg::from_file("firmware.fwpkg")?;
//!
//!     // Create flasher and connect (native only)
//!     #[cfg(feature = "native")]
//!     {
//!         let chip = ChipFamily::Ws63;
//!         let mut flasher = chip.create_flasher("/dev/ttyUSB0", 921600, false, 0)?;
//!         flasher.connect()?;
//!
//!         // Flash the firmware
//!         flasher.flash_fwpkg(&fwpkg, None, &mut |name, current, total| {
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

use std::sync::{Mutex, OnceLock};

pub mod device;
pub mod error;
pub mod host;
pub mod image;
pub mod monitor;
pub mod port;
pub mod protocol;
pub mod target;

type InterruptChecker = fn() -> bool;

static INTERRUPT_CHECKER: OnceLock<Mutex<Option<InterruptChecker>>> = OnceLock::new();

/// Register a process-level interruption checker (e.g. Ctrl-C flag reader).
///
/// The library polls this callback in long-running loops so operations can be
/// cancelled promptly instead of waiting for timeouts.
pub fn set_interrupt_checker(checker: InterruptChecker) {
    let slot = INTERRUPT_CHECKER.get_or_init(|| Mutex::new(None));
    if let Ok(mut guard) = slot.lock() {
        *guard = Some(checker);
    }
}

/// Returns whether an external interruption has been requested.
pub fn is_interrupted_requested() -> bool {
    let Some(slot) = INTERRUPT_CHECKER.get() else {
        return false;
    };

    match slot.lock() {
        Ok(guard) => match *guard {
            Some(checker) => checker(),
            None => false,
        },
        Err(_) => false,
    }
}

// Re-exports for convenience
// Native-specific re-exports
#[cfg(feature = "native")]
pub use port::{NativePort, NativePortEnumerator};
// Ws63Flasher 不直接导出，只通过 Flasher trait 访问
pub use target::{ChipConfig, ChipFamily, ChipOps, Flasher};
pub use {
    device::{DetectedPort, DeviceKind, TransportKind, UsbDevice},
    error::{Error, Result},
    host::{auto_detect_port, discover_hisilicon_ports, discover_ports},
    image::fwpkg::{Fwpkg, FwpkgBinInfo, FwpkgHeader, FwpkgVersion, PartitionType},
    monitor::{
        MonitorSession, clean_monitor_text, drain_utf8_lossy, format_monitor_output, split_utf8,
    },
    port::{Port, PortEnumerator, PortInfo, SerialConfig},
    protocol::seboot::{CommandType, ImageType, SebootAck, SebootFrame, contains_handshake_ack},
};
