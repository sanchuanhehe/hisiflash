//! WS63 flasher implementation.
//!
//! This module provides the main flasher interface for the WS63 chip.
//!
//! ## Generic Port Support
//!
//! The flasher uses a generic `Port` trait, allowing it to work with different
//! serial port implementations:
//!
//! - **Native platforms**: Uses the `serialport` crate via `NativePort`
//! - **WASM/Web**: Can use Web Serial API via `WebSerialPort` (experimental)
//!
//! ## Example
//!
//! ```rust,no_run
//! use hisiflash::{ChipFamily, Fwpkg};
//!
//! fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Create flasher using chip abstraction
//!     let mut flasher = ChipFamily::Ws63.create_flasher("/dev/ttyUSB0", 921600, false, 0)?;
//!
//!     // Connect to device
//!     flasher.connect()?;
//!
//!     // Flash firmware
//!     let fwpkg = Fwpkg::from_file("firmware.fwpkg")?;
//!     flasher.flash_fwpkg(&fwpkg, None, &mut |name, current, total| {
//!         println!("Flashing {}: {}/{}", name, current, total);
//!     })?;
//!
//!     Ok(())
//! }
//! ```

use crate::error::{Error, Result};
use crate::image::fwpkg::Fwpkg;
use crate::port::Port;
use crate::protocol::ymodem::{YmodemConfig, YmodemTransfer};
use crate::target::ws63::protocol::{CommandFrame, DEFAULT_BAUD, contains_handshake_ack};
use log::{debug, info, trace, warn};
use std::thread;
use std::time::{Duration, Instant};

/// Timeout for waiting for handshake.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);

/// Delay after sending a command before reading response.
const COMMAND_DELAY: Duration = Duration::from_millis(50);

/// Delay after changing baud rate.
const BAUD_CHANGE_DELAY: Duration = Duration::from_millis(100);

/// Delay between connection retry attempts.
const CONNECT_RETRY_DELAY: Duration = Duration::from_millis(500);

/// Maximum number of connection attempts.
const MAX_CONNECT_ATTEMPTS: usize = 7;

/// Maximum number of download retry attempts.
const MAX_DOWNLOAD_RETRIES: usize = 3;

/// WS63 flasher.
///
/// Generic over the port type `P`, which must implement the `Port` trait.
/// This allows the flasher to work with different serial port implementations.
pub struct Ws63Flasher<P: Port> {
    port: P,
    target_baud: u32,
    late_baud: bool,
    verbose: u8,
}

// Implementation for any Port type
impl<P: Port> Ws63Flasher<P> {
    /// Create a new WS63 flasher with an existing port.
    ///
    /// # Arguments
    ///
    /// * `port` - An opened serial port implementing the `Port` trait
    /// * `target_baud` - Target baud rate for data transfer
    pub fn new(port: P, target_baud: u32) -> Self {
        Self {
            port,
            target_baud,
            late_baud: false,
            verbose: 0,
        }
    }

    /// Set late baud rate change mode.
    ///
    /// In late baud mode, the baud rate is changed after LoaderBoot is loaded,
    /// which may be necessary for some firmware configurations.
    #[must_use]
    pub fn with_late_baud(mut self, late_baud: bool) -> Self {
        self.late_baud = late_baud;
        self
    }

    /// Set verbose output level.
    #[must_use]
    pub fn with_verbose(mut self, verbose: u8) -> Self {
        self.verbose = verbose;
        self
    }

    /// Connect to the device.
    ///
    /// This waits for the device to boot into download mode and performs
    /// the initial handshake with retry mechanism.
    pub fn connect(&mut self) -> Result<()> {
        info!("Waiting for device on {}...", self.port.name());
        info!("Please reset the device to enter download mode.");

        for attempt in 1..=MAX_CONNECT_ATTEMPTS {
            if attempt > 1 {
                info!("Connection attempt {attempt}/{MAX_CONNECT_ATTEMPTS}");
            }

            match self.try_connect() {
                Ok(()) => {
                    return Ok(());
                },
                Err(e) => {
                    if attempt < MAX_CONNECT_ATTEMPTS {
                        warn!("Connection failed (attempt {attempt}/{MAX_CONNECT_ATTEMPTS}): {e}");
                        thread::sleep(CONNECT_RETRY_DELAY);
                        self.port.clear_buffers()?;
                    } else {
                        return Err(e);
                    }
                },
            }
        }

        Err(Error::Timeout(format!(
            "Connection failed after {MAX_CONNECT_ATTEMPTS} attempts"
        )))
    }

    /// Single connection attempt.
    fn try_connect(&mut self) -> Result<()> {
        self.port.clear_buffers()?;

        let start = Instant::now();
        let handshake_frame = CommandFrame::handshake(self.target_baud);
        let handshake_data = handshake_frame.build();

        // Send handshake frames repeatedly until we get a response
        while start.elapsed() < HANDSHAKE_TIMEOUT {
            // Send handshake
            if let Err(e) = self.port.write_all(&handshake_data) {
                trace!("Write error (ignoring): {e}");
            }
            let _ = self.port.flush();

            // Small delay
            thread::sleep(Duration::from_millis(10));

            // Check for response
            let mut buf = [0u8; 256];
            match self.port.read(&mut buf) {
                Ok(n) if n > 0 => {
                    trace!("Received {n} bytes");
                    if contains_handshake_ack(&buf[..n]) {
                        info!("Handshake successful!");

                        // Change baud rate if not in late mode
                        if !self.late_baud && self.target_baud != DEFAULT_BAUD {
                            self.change_baud_rate(self.target_baud)?;
                        }

                        return Ok(());
                    }
                },
                Ok(_) => {},
                Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {},
                Err(e) => {
                    trace!("Read error (ignoring): {e}");
                },
            }
        }

        Err(Error::Timeout(format!(
            "No response after {} seconds",
            HANDSHAKE_TIMEOUT.as_secs()
        )))
    }

    /// Change the baud rate.
    fn change_baud_rate(&mut self, baud: u32) -> Result<()> {
        info!("Changing baud rate to {baud}");

        // Send baud rate change command
        let frame = CommandFrame::set_baud_rate(baud);
        self.send_command(&frame)?;

        // Wait for command to be processed
        thread::sleep(BAUD_CHANGE_DELAY);

        // Change local baud rate
        self.port.set_baud_rate(baud)?;

        // Clear buffers
        thread::sleep(BAUD_CHANGE_DELAY);
        self.port.clear_buffers()?;

        debug!("Baud rate changed to {baud}");
        Ok(())
    }

    /// Send a command frame.
    fn send_command(&mut self, frame: &CommandFrame) -> Result<()> {
        let data = frame.build();
        trace!(
            "Sending command {:?}: {} bytes",
            frame.command(),
            data.len()
        );

        self.port.write_all(&data)?;
        self.port.flush()?;

        Ok(())
    }

    /// Wait for YMODEM 'C' character.
    fn wait_for_c(&mut self, timeout: Duration) -> Result<()> {
        let start = Instant::now();
        let mut buf = [0u8; 1];

        while start.elapsed() < timeout {
            match self.port.read(&mut buf) {
                Ok(1) if buf[0] == b'C' => {
                    debug!("Received 'C', ready for YMODEM transfer");
                    return Ok(());
                },
                Ok(1) => {
                    trace!("Received: 0x{:02X}", buf[0]);
                },
                Ok(_) => {},
                Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {},
                Err(e) => return Err(Error::Io(e)),
            }
        }

        Err(Error::Timeout("Timeout waiting for 'C'".into()))
    }

    /// Flash a FWPKG firmware package.
    ///
    /// # Arguments
    ///
    /// * `fwpkg` - The firmware package to flash
    /// * `filter` - Optional filter for partition names (None = flash all)
    /// * `progress` - Progress callback (partition_name, current_bytes, total_bytes)
    pub fn flash_fwpkg<F>(
        &mut self,
        fwpkg: &Fwpkg,
        filter: Option<&[&str]>,
        mut progress: F,
    ) -> Result<()>
    where
        F: FnMut(&str, usize, usize),
    {
        // Get LoaderBoot
        let loaderboot = fwpkg
            .loaderboot()
            .ok_or_else(|| Error::InvalidFwpkg("No LoaderBoot partition found".into()))?;

        info!("Flashing LoaderBoot: {}", loaderboot.name);

        // Send download command for LoaderBoot
        let lb_data = fwpkg.bin_data(loaderboot)?;
        self.download_binary(
            &loaderboot.name,
            lb_data,
            loaderboot.burn_addr,
            &mut progress,
        )?;

        // Change baud rate if in late mode
        if self.late_baud && self.target_baud != DEFAULT_BAUD {
            self.change_baud_rate(self.target_baud)?;
        }

        // Flash remaining partitions
        for bin in fwpkg.normal_bins() {
            // Apply filter if provided
            if let Some(names) = filter {
                if !names.iter().any(|n| bin.name.contains(n)) {
                    debug!("Skipping partition: {}", bin.name);
                    continue;
                }
            }

            info!(
                "Flashing partition: {} -> 0x{:08X}",
                bin.name, bin.burn_addr
            );

            let bin_data = fwpkg.bin_data(bin)?;
            self.download_binary(&bin.name, bin_data, bin.burn_addr, &mut progress)?;
        }

        info!("Flashing complete!");
        Ok(())
    }

    /// Download a single binary to flash with retry mechanism.
    #[allow(clippy::cast_possible_truncation)]
    fn download_binary<F>(
        &mut self,
        name: &str,
        data: &[u8],
        addr: u32,
        progress: &mut F,
    ) -> Result<()>
    where
        F: FnMut(&str, usize, usize),
    {
        let mut last_error = None;

        for attempt in 1..=MAX_DOWNLOAD_RETRIES {
            match self.try_download_binary(name, data, addr, progress) {
                Ok(()) => {
                    return Ok(());
                },
                Err(e) => {
                    if attempt < MAX_DOWNLOAD_RETRIES {
                        warn!(
                            "Download failed for {name} (attempt {attempt}/{MAX_DOWNLOAD_RETRIES}): {e}"
                        );
                        warn!("Retrying...");
                        last_error = Some(e);

                        // Clear buffers and wait before retry
                        let _ = self.port.clear_buffers();
                        thread::sleep(CONNECT_RETRY_DELAY);
                    } else {
                        return Err(e);
                    }
                },
            }
        }

        // Use unwrap_or_else to ensure we never lose error information
        Err(last_error.unwrap_or_else(|| {
            Error::Protocol("Download failed after all retries (no error captured)".into())
        }))
    }

    /// Single attempt to download a binary.
    fn try_download_binary<F>(
        &mut self,
        name: &str,
        data: &[u8],
        addr: u32,
        progress: &mut F,
    ) -> Result<()>
    where
        F: FnMut(&str, usize, usize),
    {
        // Check for oversized data that would truncate
        let len = u32::try_from(data.len())
            .map_err(|_| Error::Protocol(format!("Firmware too large ({} bytes > 4GB)", data.len())))?;

        debug!(
            "Downloading {} ({} bytes) to 0x{:08X}",
            name,
            data.len(),
            addr
        );

        // Send download command
        let frame = CommandFrame::download(addr, len, len);
        self.send_command(&frame)?;

        // Wait for 'C'
        thread::sleep(COMMAND_DELAY);
        self.wait_for_c(Duration::from_secs(10))?;

        // Transfer using YMODEM
        let config = YmodemConfig {
            char_timeout: Duration::from_millis(1000),
            c_timeout: Duration::from_secs(30),
            max_retries: 10,
            verbose: self.verbose,
        };

        let mut ymodem = YmodemTransfer::with_config(&mut self.port, config);
        ymodem.transfer(name, data, |current, total| {
            progress(name, current, total);
        })?;

        debug!("{name} transfer complete");
        Ok(())
    }

    /// Write raw binary data to flash.
    ///
    /// # Arguments
    ///
    /// * `loaderboot` - LoaderBoot binary data (required for first-stage boot)
    /// * `bins` - List of (data, address) pairs to flash
    pub fn write_bins(&mut self, loaderboot: &[u8], bins: &[(&[u8], u32)]) -> Result<()> {
        info!("Writing LoaderBoot ({} bytes)", loaderboot.len());

        // Download LoaderBoot first
        self.download_binary("loaderboot", loaderboot, 0, &mut |_, _, _| {})?;

        // Change baud rate if in late mode
        if self.late_baud && self.target_baud != DEFAULT_BAUD {
            self.change_baud_rate(self.target_baud)?;
        }

        // Download remaining binaries
        for (i, (data, addr)) in bins.iter().enumerate() {
            let name = format!("binary_{i}");
            info!("Writing {} ({} bytes) to 0x{:08X}", name, data.len(), addr);
            self.download_binary(&name, data, *addr, &mut |_, _, _| {})?;
        }

        Ok(())
    }

    /// Erase entire flash.
    pub fn erase_all(&mut self) -> Result<()> {
        info!("Erasing entire flash...");

        let frame = CommandFrame::erase_all();
        self.send_command(&frame)?;

        // Wait for erase to complete
        thread::sleep(Duration::from_secs(5));

        info!("Flash erased");
        Ok(())
    }

    /// Reset the device.
    pub fn reset(&mut self) -> Result<()> {
        info!("Resetting device...");

        let frame = CommandFrame::reset();
        self.send_command(&frame)?;

        Ok(())
    }
}

// Native-specific convenience functions
#[cfg(feature = "native")]
mod native_impl {
    use super::{DEFAULT_BAUD, Duration, Error, Result, Ws63Flasher, debug, thread, warn};
    use crate::port::NativePort;

    impl Ws63Flasher<NativePort> {
        /// Create a new WS63 flasher by opening a serial port.
        ///
        /// This is a convenience function for native platforms that opens
        /// the port with default settings.
        ///
        /// # Arguments
        ///
        /// * `port_name` - Serial port name (e.g., "/dev/ttyUSB0" or "COM3")
        /// * `target_baud` - Target baud rate for data transfer
        pub fn open(port_name: &str, target_baud: u32) -> Result<Self> {
            Self::open_with_retry(port_name, target_baud)
        }

        /// Open a serial port with full configuration (P0: 完整配置支持).
        ///
        /// This allows customization of all serial port parameters.
        ///
        /// # Arguments
        ///
        /// * `config` - Serial port configuration
        pub fn open_with_config(config: crate::port::SerialConfig) -> Result<Self> {
            Self::open_with_config_retry(config)
        }

        /// Open serial port with full config and retry mechanism.
        #[allow(clippy::needless_pass_by_value)]
        fn open_with_config_retry(config: crate::port::SerialConfig) -> Result<Self> {
            const MAX_OPEN_PORT_ATTEMPTS: usize = 3;
            const OPEN_RETRY_DELAY: Duration = Duration::from_millis(500);

            let mut last_error = None;

            for attempt in 1..=MAX_OPEN_PORT_ATTEMPTS {
                match NativePort::open(&config) {
                    Ok(port) => {
                        if attempt > 1 {
                            debug!("Port opened on attempt {attempt}");
                        }
                        return Ok(Self::new(port, config.baud_rate));
                    },
                    Err(e) => {
                        warn!(
                            "Failed to open port {} (attempt {}/{}): {e}",
                            config.port_name, attempt, MAX_OPEN_PORT_ATTEMPTS
                        );
                        last_error = Some(e);

                        if attempt < MAX_OPEN_PORT_ATTEMPTS {
                            thread::sleep(OPEN_RETRY_DELAY);
                        }
                    },
                }
            }

            Err(last_error.unwrap_or_else(||
                Error::Config(format!("Failed to open port after {} attempts", MAX_OPEN_PORT_ATTEMPTS))
            ))
        }

        /// Open serial port with retry mechanism.
        fn open_with_retry(port_name: &str, target_baud: u32) -> Result<Self> {
            const MAX_OPEN_PORT_ATTEMPTS: usize = 3;
            const OPEN_RETRY_DELAY: Duration = Duration::from_millis(500);

            let mut last_error = None;

            for attempt in 1..=MAX_OPEN_PORT_ATTEMPTS {
                let config = crate::port::SerialConfig::new(port_name, DEFAULT_BAUD);
                match NativePort::open(&config) {
                    Ok(port) => {
                        if attempt > 1 {
                            debug!("Port opened on attempt {attempt}");
                        }
                        return Ok(Self::new(port, target_baud));
                    },
                    Err(e) => {
                        warn!(
                            "Failed to open port {port_name} (attempt {attempt}/{MAX_OPEN_PORT_ATTEMPTS}): {e}"
                        );
                        last_error = Some(e);

                        if attempt < MAX_OPEN_PORT_ATTEMPTS {
                            thread::sleep(OPEN_RETRY_DELAY);
                        }
                    },
                }
            }

            Err(last_error.unwrap_or_else(||
                Error::Config(format!("Failed to open port {} after {} attempts", port_name, MAX_OPEN_PORT_ATTEMPTS))
            ))
        }
    }
}

impl<P: Port> crate::target::Flasher for Ws63Flasher<P> {
    fn connect(&mut self) -> Result<()> {
        self.connect()
    }

    fn flash_fwpkg(
        &mut self,
        fwpkg: &Fwpkg,
        filter: Option<&[&str]>,
        progress: &mut dyn FnMut(&str, usize, usize),
    ) -> Result<()> {
        self.flash_fwpkg(fwpkg, filter, |name, current, total| {
            progress(name, current, total);
        })
    }

    fn write_bins(&mut self, loaderboot: &[u8], bins: &[(&[u8], u32)]) -> Result<()> {
        self.write_bins(loaderboot, bins)
    }

    fn erase_all(&mut self) -> Result<()> {
        self.erase_all()
    }

    fn reset(&mut self) -> Result<()> {
        self.reset()
    }

    fn connection_baud(&self) -> u32 {
        DEFAULT_BAUD
    }

    fn target_baud(&self) -> Option<u32> {
        Some(self.target_baud)
    }

    fn close(&mut self) {
        // Close the underlying port to release resources
        // This is important for proper cleanup after reset
        let _ = self.port.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::port::Port;
    use std::io::{Read, Write};
    use std::sync::{Arc, Mutex};

    /// Mock port implementation for testing without real hardware.
    ///
    /// This implementation uses an internal buffer to simulate serial port
    /// behavior, allowing unit tests to run without actual hardware.
    #[derive(Clone)]
    struct MockPort {
        name: String,
        baud_rate: u32,
        timeout: Duration,
        read_buffer: Arc<Mutex<Vec<u8>>>,
        write_buffer: Arc<Mutex<Vec<u8>>>,
        dtr: bool,
        rts: bool,
    }

    impl MockPort {
        fn new(name: &str) -> Self {
            Self {
                name: name.to_string(),
                baud_rate: 115200,
                timeout: Duration::from_millis(1000),
                read_buffer: Arc::new(Mutex::new(Vec::new())),
                write_buffer: Arc::new(Mutex::new(Vec::new())),
                dtr: false,
                rts: false,
            }
        }

        /// Add data to the read buffer (simulates receiving data from device).
        fn add_read_data(&self, data: &[u8]) {
            let mut buf = self.read_buffer.lock().unwrap();
            buf.extend_from_slice(data);
        }

        /// Get data written to the port (simulates sending data to device).
        fn get_written_data(&self) -> Vec<u8> {
            let buf = self.write_buffer.lock().unwrap();
            buf.clone()
        }

        /// Clear all buffers.
        fn clear(&self) {
            let mut read_buf = self.read_buffer.lock().unwrap();
            let mut write_buf = self.write_buffer.lock().unwrap();
            read_buf.clear();
            write_buf.clear();
        }
    }

    impl Port for MockPort {
        fn set_timeout(&mut self, timeout: Duration) -> Result<()> {
            self.timeout = timeout;
            Ok(())
        }

        fn timeout(&self) -> Duration {
            self.timeout
        }

        fn set_baud_rate(&mut self, baud_rate: u32) -> Result<()> {
            self.baud_rate = baud_rate;
            Ok(())
        }

        fn baud_rate(&self) -> u32 {
            self.baud_rate
        }

        fn clear_buffers(&mut self) -> Result<()> {
            self.clear();
            Ok(())
        }

        fn name(&self) -> &str {
            &self.name
        }

        fn set_dtr(&mut self, level: bool) -> Result<()> {
            self.dtr = level;
            Ok(())
        }

        fn set_rts(&mut self, level: bool) -> Result<()> {
            self.rts = level;
            Ok(())
        }

        fn read_cts(&self) -> Result<bool> {
            Ok(true) // Assume CTS is asserted
        }

        fn read_dsr(&self) -> Result<bool> {
            Ok(true) // Assume DSR is asserted
        }

        fn close(&mut self) -> Result<()> {
            // Clear all buffers to simulate port closure
            self.clear();
            Ok(())
        }
    }

    impl Read for MockPort {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let mut read_buf = self.read_buffer.lock().map_err(|e| {
                std::io::Error::other(format!("mutex poisoned: {e}"))
            })?;

            if read_buf.is_empty() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "no data available",
                ));
            }

            let to_read = std::cmp::min(buf.len(), read_buf.len());
            buf[..to_read].copy_from_slice(&read_buf[..to_read]);
            read_buf.drain(..to_read);
            Ok(to_read)
        }
    }

    impl Write for MockPort {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            let mut write_buf = self.write_buffer.lock().map_err(|e| {
                std::io::Error::other(format!("mutex poisoned: {e}"))
            })?;
            write_buf.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// Test creating a Ws63Flasher with a mock port.
    #[test]
    fn test_flasher_new_with_mock_port() {
        let port = MockPort::new("/dev/ttyUSB0");
        let flasher = Ws63Flasher::new(port, 921600);

        assert_eq!(flasher.target_baud, 921600);
        assert!(!flasher.late_baud);
        assert_eq!(flasher.verbose, 0);
    }

    /// Test builder methods on Ws63Flasher.
    #[test]
    fn test_flasher_builder_methods() {
        let port = MockPort::new("/dev/ttyUSB0");
        let flasher = Ws63Flasher::new(port, 921600)
            .with_late_baud(true)
            .with_verbose(2);

        assert!(flasher.late_baud);
        assert_eq!(flasher.verbose, 2);
    }

    /// Test MockPort read/write operations.
    #[test]
    fn test_mock_port_read_write() {
        let mut port = MockPort::new("/dev/ttyUSB0");

        // Add some data to read buffer
        port.add_read_data(&[0xDE, 0xAD, 0xBE, 0xEF]);

        // Write some data
        port.write_all(b"test").unwrap();
        port.flush().unwrap();

        // Verify written data
        let written = port.get_written_data();
        assert_eq!(written, b"test");

        // Read data - use read_exact to handle partial reads properly
        let mut buf = [0u8; 4];
        std::io::Read::read_exact(&mut port, &mut buf).unwrap();
        assert_eq!(&buf, &[0xDE, 0xAD, 0xBE, 0xEF]);
    }

    /// Test MockPort buffer operations.
    #[test]
    fn test_mock_port_buffers() {
        let mut port = MockPort::new("/dev/ttyUSB0");

        // Clear buffers
        port.clear();
        assert!(port.get_written_data().is_empty());

        // Write and add read data
        port.write_all(b"hello").unwrap();
        port.add_read_data(&[1, 2, 3]);

        // Verify
        assert_eq!(port.get_written_data(), b"hello");

        let mut buf = [0u8; 3];
        std::io::Read::read_exact(&mut port, &mut buf).unwrap();
        assert_eq!(&buf, &[1, 2, 3]);

        // Clear and verify
        port.clear();
        assert!(port.get_written_data().is_empty());
    }

    /// Test MockPort pin control.
    #[test]
    fn test_mock_port_pin_control() {
        let mut port = MockPort::new("/dev/ttyUSB0");

        assert!(!port.dtr);
        assert!(!port.rts);

        port.set_dtr(true).unwrap();
        port.set_rts(true).unwrap();

        assert!(port.dtr);
        assert!(port.rts);
    }

    /// Test MockPort baud rate and timeout.
    #[test]
    fn test_mock_port_baud_timeout() {
        let mut port = MockPort::new("/dev/ttyUSB0");

        assert_eq!(port.baud_rate(), 115200);
        assert_eq!(port.timeout(), Duration::from_millis(1000));

        port.set_baud_rate(921600).unwrap();
        port.set_timeout(Duration::from_millis(500)).unwrap();

        assert_eq!(port.baud_rate(), 921600);
        assert_eq!(port.timeout(), Duration::from_millis(500));
    }

    /// Test MockPort name.
    #[test]
    fn test_mock_port_name() {
        let port = MockPort::new("/dev/ttyUSB1");
        assert_eq!(port.name(), "/dev/ttyUSB1");

        let port2 = MockPort::new("COM3");
        assert_eq!(port2.name(), "COM3");
    }

    /// Test creating flasher with mock port through ChipFamily::create_flasher_with_port.
    #[test]
    fn test_create_flasher_with_mock_port() {
        use crate::target::ChipFamily;

        let port = MockPort::new("/dev/ttyUSB0");
        let flasher = ChipFamily::Ws63.create_flasher_with_port(port, 921600, false, 0);

        assert!(flasher.is_ok());
        let flasher = flasher.unwrap();

        // Flasher should be usable (even though connect will fail without mock response data)
        assert_eq!(flasher.connection_baud(), 115200); // DEFAULT_BAUD for handshake
        assert_eq!(flasher.target_baud(), Some(921600));
    }

    /// Test that Flasher trait object works correctly.
    #[test]
    fn test_flasher_trait_object() {
        use crate::target::Flasher;

        let port = MockPort::new("/dev/ttyUSB0");
        let flasher: Box<dyn Flasher> = Box::new(Ws63Flasher::new(port, 921600));

        assert_eq!(flasher.connection_baud(), 115200);
        assert_eq!(flasher.target_baud(), Some(921600));
    }

    /// Test multiple flasher instances with same mock port clone.
    #[test]
    fn test_multiple_flashers_same_port() {
        use crate::target::ChipFamily;

        let port = MockPort::new("/dev/ttyUSB0");
        let port_clone = port.clone();

        let flasher1 = ChipFamily::Ws63.create_flasher_with_port(port, 921600, false, 0);
        let flasher2 = ChipFamily::Ws63.create_flasher_with_port(port_clone, 115200, true, 1);

        assert!(flasher1.is_ok());
        assert!(flasher2.is_ok());

        let flasher1 = flasher1.unwrap();
        let flasher2 = flasher2.unwrap();

        assert_eq!(flasher1.target_baud(), Some(921600));
        assert_eq!(flasher2.target_baud(), Some(115200));
    }

    /// Test unsupported chip family returns error for create_flasher_with_port.
    #[test]
    fn test_create_flasher_with_port_unsupported_chip() {
        use crate::target::ChipFamily;

        let port = MockPort::new("/dev/ttyUSB0");
        let result = ChipFamily::Bs2x.create_flasher_with_port(port, 115200, false, 0);

        assert!(result.is_err());
        // Verify error is the Unsupported variant
        assert!(matches!(result, Err(crate::error::Error::Unsupported(_))));
    }
}
