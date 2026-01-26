//! WASM serial port implementation using Web Serial API.
//!
//! This module provides a serial port implementation for WASM targets
//! using the Web Serial API available in modern browsers.
//!
//! ## Requirements
//!
//! - Browser with Web Serial API support (Chrome, Edge, Opera)
//! - HTTPS or localhost (required for Web Serial API)
//! - User gesture to request port access
//!
//! ## Note
//!
//! The Web Serial API is inherently asynchronous, while our `Port` trait
//! is synchronous. This implementation uses blocking semantics where possible,
//! but full async support may be needed for optimal web performance.
//!
//! ## Example (JavaScript interop)
//!
//! ```javascript
//! // Request port from user
//! const port = await navigator.serial.requestPort();
//! await port.open({ baudRate: 115200 });
//!
//! // Pass to WASM
//! wasm_module.set_serial_port(port);
//! ```

use crate::error::{Error, Result};
use crate::port::{Port, PortEnumerator, PortInfo, SerialConfig};
use std::io::{Read, Write};
use std::time::Duration;

/// Web Serial port implementation.
///
/// This is a placeholder for future Web Serial API support.
/// The actual implementation will use `web-sys` bindings to the
/// Web Serial API.
pub struct WebSerialPort {
    name: String,
    baud_rate: u32,
    timeout: Duration,
    // TODO: Add web-sys Serial port handle
    // port: web_sys::SerialPort,
    // reader: web_sys::ReadableStreamDefaultReader,
    // writer: web_sys::WritableStreamDefaultWriter,
}

impl WebSerialPort {
    /// Create a new Web Serial port.
    ///
    /// Note: In WASM, port opening must be initiated by a user gesture
    /// and is asynchronous. This constructor expects the port to already
    /// be opened from JavaScript.
    pub fn new(_config: &SerialConfig) -> Result<Self> {
        Err(Error::Unsupported(
            "Web Serial API support is not yet implemented. \
             Please use the native version of hisiflash."
                .to_string(),
        ))
    }

    /// Create from an existing JavaScript SerialPort object.
    ///
    /// This is the primary way to create a WebSerialPort in WASM,
    /// as port selection must be done via JavaScript user interaction.
    ///
    /// Note: This function is only available when targeting WASM and
    /// when the Web Serial API becomes stable in web-sys.
    #[cfg(all(feature = "wasm", target_arch = "wasm32"))]
    pub fn from_js_port(
        _js_port: js_sys::Object, // Use generic Object for now until web-sys stabilizes SerialPort
        name: String,
        baud_rate: u32,
    ) -> Result<Self> {
        // TODO: Implement when web-sys Web Serial API support is stable
        // The Web Serial API types (Serial, SerialPort, etc.) are not yet
        // available in stable web-sys. When they become available, this
        // function will accept web_sys::SerialPort directly.
        let _ = (_js_port, &name, baud_rate);
        Err(Error::Unsupported(
            "Web Serial API support is not yet implemented.".to_string(),
        ))
    }
}

impl Port for WebSerialPort {
    fn set_timeout(&mut self, timeout: Duration) -> Result<()> {
        self.timeout = timeout;
        Ok(())
    }

    fn timeout(&self) -> Duration {
        self.timeout
    }

    fn set_baud_rate(&mut self, baud_rate: u32) -> Result<()> {
        // Web Serial API requires closing and reopening to change baud rate
        self.baud_rate = baud_rate;
        Err(Error::Unsupported(
            "Changing baud rate on Web Serial requires reopening the port.".to_string(),
        ))
    }

    fn baud_rate(&self) -> u32 {
        self.baud_rate
    }

    fn clear_buffers(&mut self) -> Result<()> {
        // Web Serial API doesn't have a direct buffer clear
        // We would need to read and discard any pending data
        Ok(())
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn set_dtr(&mut self, _level: bool) -> Result<()> {
        // Web Serial API supports setSignals for DTR
        Err(Error::Unsupported(
            "DTR control not yet implemented for Web Serial.".to_string(),
        ))
    }

    fn set_rts(&mut self, _level: bool) -> Result<()> {
        // Web Serial API supports setSignals for RTS
        Err(Error::Unsupported(
            "RTS control not yet implemented for Web Serial.".to_string(),
        ))
    }

    fn read_cts(&self) -> Result<bool> {
        // Web Serial API supports getSignals for CTS
        Err(Error::Unsupported(
            "CTS reading not yet implemented for Web Serial.".to_string(),
        ))
    }

    fn read_dsr(&self) -> Result<bool> {
        // Web Serial API supports getSignals for DSR
        Err(Error::Unsupported(
            "DSR reading not yet implemented for Web Serial.".to_string(),
        ))
    }
}

impl Read for WebSerialPort {
    fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
        // TODO: Implement using ReadableStreamDefaultReader
        // This will need async-to-sync bridging via wasm-bindgen-futures
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "Web Serial read not yet implemented",
        ))
    }
}

impl Write for WebSerialPort {
    fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
        // TODO: Implement using WritableStreamDefaultWriter
        // This will need async-to-sync bridging via wasm-bindgen-futures
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "Web Serial write not yet implemented",
        ))
    }

    fn flush(&mut self) -> std::io::Result<()> {
        // Web Serial writes are buffered by the browser
        Ok(())
    }
}

/// Web Serial port enumerator.
pub struct WebSerialPortEnumerator;

impl PortEnumerator for WebSerialPortEnumerator {
    fn list_ports() -> Result<Vec<PortInfo>> {
        // Web Serial API doesn't allow enumeration without user gesture
        // getPorts() only returns previously granted ports
        Err(Error::Unsupported(
            "Web Serial cannot enumerate ports without user interaction. \
             Use navigator.serial.requestPort() from JavaScript instead."
                .to_string(),
        ))
    }
}

// Future async API for better WASM integration
// These would be the primary interface for web applications

/// Async port trait for WASM environments.
///
/// This trait provides an async interface more suitable for the
/// inherently async Web Serial API.
///
/// Note: This trait is intended for internal use within this crate.
/// The `async fn` in traits warning is suppressed as we control all implementations.
#[cfg(feature = "wasm")]
#[allow(async_fn_in_trait)]
pub trait AsyncPort {
    /// Read bytes asynchronously.
    async fn read_async(&mut self, buf: &mut [u8]) -> Result<usize>;

    /// Write bytes asynchronously.
    async fn write_async(&mut self, buf: &[u8]) -> Result<usize>;

    /// Write all bytes asynchronously.
    async fn write_all_async(&mut self, buf: &[u8]) -> Result<()>;

    /// Flush the write buffer asynchronously.
    async fn flush_async(&mut self) -> Result<()>;
}
