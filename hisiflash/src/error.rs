//! Error types for hisiflash.

use std::io;
use thiserror::Error;

/// Result type for hisiflash operations.
pub type Result<T> = std::result::Result<T, Error>;

/// Error type for hisiflash operations.
#[derive(Debug, Error)]
pub enum Error {
    /// I/O error (serial port, file operations).
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    /// Serial port error.
    #[error("Serial port error: {0}")]
    Serial(#[from] serialport::Error),

    /// Invalid firmware package format.
    #[error("Invalid FWPKG: {0}")]
    InvalidFwpkg(String),

    /// CRC checksum mismatch.
    #[error("CRC mismatch: expected {expected:#06x}, got {actual:#06x}")]
    CrcMismatch {
        /// Expected CRC value.
        expected: u16,
        /// Actual CRC value.
        actual: u16,
    },

    /// Communication timeout.
    #[error("Timeout: {0}")]
    Timeout(String),

    /// Device not responding or not in boot mode.
    #[error("Device not found or not in boot mode")]
    DeviceNotFound,

    /// Handshake failed.
    #[error("Handshake failed: {0}")]
    HandshakeFailed(String),

    /// Protocol error.
    #[error("Protocol error: {0}")]
    Protocol(String),

    /// YMODEM transfer error.
    #[error("YMODEM error: {0}")]
    Ymodem(String),

    /// Unsupported chip or operation.
    #[error("Unsupported: {0}")]
    Unsupported(String),

    /// Configuration error.
    #[error("Configuration error: {0}")]
    Config(String),
}
