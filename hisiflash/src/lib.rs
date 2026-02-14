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
//! ## Cancellation Model
//!
//! Long-running operations (flashing, erasing, etc.) can be cancelled via the
//! [`CancelContext`] mechanism. This allows the embedding application (e.g., CLI)
//! to signal interruption (e.g., Ctrl-C) and have the operation stop gracefully.
//!
//! ### Quick Start
//!
//! ```ignore
//! use hisiflash::{CancelContext, cancel_context_from_global};
//!
//! // Option 1: Use global interrupt flag (set by CLI when Ctrl-C is pressed)
//! let cancel = cancel_context_from_global();
//!
//! // Option 2: Create a custom cancel context
//! use std::sync::atomic::{AtomicBool, Ordering};
//! let flag = AtomicBool::new(false);
//! let cancel = CancelContext::new(move || flag.load(Ordering::SeqCst));
//!
//! // Option 3: No cancellation (always returns "not cancelled")
//! let cancel = CancelContext::none();
//! ```
//!
//! ### Integration with Flasher
//!
//! ```ignore
//! use hisiflash::{CancelContext, Ws63Flasher, cancel_context_from_global};
//!
//! // Create flasher with global cancellation support
//! let cancel = cancel_context_from_global();
//! let flasher = Ws63Flasher::with_cancel(port, 921600, cancel);
//!
//! // Or use non-cancellable flasher
//! let flasher = Ws63Flasher::new(port, 921600);
//! ```
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

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub mod device;
pub mod error;
pub mod host;
pub mod image;
pub mod monitor;
pub mod port;
pub mod protocol;
pub mod target;

/// Global interrupt flag for CLI-to-library communication.
///
/// This is set by CLI when Ctrl-C is received, and checked by
/// `cancel_context_from_global()` during long-running operations.
static INTERRUPT_FLAG: AtomicBool = AtomicBool::new(false);

/// Explicit cancellation context for long-running library operations.
///
/// Unlike the global interrupt checker, this is explicitly passed through
/// the call chain, making it testable and composable.
#[derive(Clone, Default)]
pub struct CancelContext {
    checker: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
}

impl CancelContext {
    /// Create a new cancel context with the given checker function.
    #[must_use]
    pub fn new<F>(checker: F) -> Self
    where
        F: Fn() -> bool + Send + Sync + 'static,
    {
        Self {
            checker: Some(Arc::new(checker)),
        }
    }

    /// Create a no-op cancel context (always returns "not cancelled").
    #[must_use]
    pub fn none() -> Self {
        Self { checker: None }
    }

    /// Returns true if cancellation has been requested.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.checker
            .as_ref()
            .is_some_and(|c| c())
    }

    /// Check and return an Interrupted error if cancelled.
    pub fn check(&self) -> crate::Result<()> {
        if self.is_cancelled() {
            return Err(crate::Error::Io(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "operation cancelled",
            )));
        }
        Ok(())
    }
}

/// Bridge from global interrupt checker to CancelContext for backward compatibility.
impl From<fn() -> bool> for CancelContext {
    fn from(checker: fn() -> bool) -> Self {
        Self::new(checker)
    }
}

/// Create a CancelContext that bridges to the global interrupt flag.
///
/// This is used internally by native implementations to check for Ctrl-C.
#[must_use]
pub fn cancel_context_from_global() -> CancelContext {
    CancelContext::new(|| INTERRUPT_FLAG.load(Ordering::SeqCst))
}

/// Set the global interrupt flag (for CLI to call when Ctrl-C is received).
pub fn set_interrupt_flag() {
    INTERRUPT_FLAG.store(true, Ordering::SeqCst);
}

/// Clear the global interrupt flag.
pub fn clear_interrupt_flag() {
    INTERRUPT_FLAG.store(false, Ordering::SeqCst);
}

/// Returns whether interruption was requested.
#[must_use]
pub fn is_interrupted_requested() -> bool {
    INTERRUPT_FLAG.load(Ordering::SeqCst)
}

#[cfg(test)]
pub(crate) fn test_set_interrupted(value: bool) {
    INTERRUPT_FLAG.store(value, Ordering::SeqCst);
}

// Re-exports for convenience
// Native-specific re-exports
#[cfg(feature = "native")]
pub use port::{NativePort, NativePortEnumerator};
// Ws63Flasher 不直接导出，只通过 Flasher trait 访问
pub use target::{ChipConfig, ChipFamily, ChipOps, Flasher};
// CancelContext is already defined in this module, no need to re-export
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
