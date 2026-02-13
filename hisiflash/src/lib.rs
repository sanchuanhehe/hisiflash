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

use std::sync::{Arc, OnceLock};

pub mod device;
pub mod error;
pub mod host;
pub mod image;
pub mod monitor;
pub mod port;
pub mod protocol;
pub mod target;

static INTERRUPT_CHECKER: OnceLock<Arc<dyn Fn() -> bool + Send + Sync>> = OnceLock::new();

/// Register a global interruption checker used by long-running library loops.
///
/// The checker should return `true` when the current operation should stop
/// (for example after receiving Ctrl-C in CLI applications).
pub fn set_interrupt_checker<F>(checker: F)
where
    F: Fn() -> bool + Send + Sync + 'static,
{
    let _ = INTERRUPT_CHECKER.set(Arc::new(checker));
}

/// Returns whether interruption was requested by the embedding application.
#[must_use]
pub fn is_interrupted_requested() -> bool {
    INTERRUPT_CHECKER
        .get()
        .is_some_and(|checker| checker())
}

#[cfg(test)]
pub(crate) fn test_set_interrupted(value: bool) {
    use std::sync::atomic::{AtomicBool, Ordering};

    static TEST_INTERRUPT_FLAG: OnceLock<Arc<AtomicBool>> = OnceLock::new();

    let flag = TEST_INTERRUPT_FLAG
        .get_or_init(|| {
            let shared = Arc::new(AtomicBool::new(false));
            let checker = Arc::clone(&shared);
            set_interrupt_checker(move || checker.load(Ordering::Relaxed));
            shared
        })
        .clone();

    flag.store(value, Ordering::Relaxed);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_interrupt_checker_default_false() {
        test_set_interrupted(false);
        assert!(!is_interrupted_requested());
    }

    #[test]
    fn test_interrupt_checker_toggle_true_false() {
        test_set_interrupted(true);
        assert!(is_interrupted_requested());

        test_set_interrupted(false);
        assert!(!is_interrupted_requested());
    }
}
