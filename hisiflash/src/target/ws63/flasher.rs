//! WS63 flasher implementation.
//!
//! This module provides the main flasher interface for the WS63 chip.

use crate::connection::ConnectionPort;
use crate::connection::serial::SerialPort;
use crate::error::{Error, Result};
use crate::image::fwpkg::Fwpkg;
use crate::protocol::ymodem::{YmodemConfig, YmodemTransfer};
use crate::target::ws63::protocol::{CommandFrame, DEFAULT_BAUD, contains_handshake_ack};
use log::{debug, info, trace, warn};
use std::io::{Read, Write};
use std::thread;
use std::time::{Duration, Instant};

/// Timeout for waiting for handshake.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);

/// Delay after sending a command before reading response.
const COMMAND_DELAY: Duration = Duration::from_millis(50);

/// Delay after changing baud rate.
const BAUD_CHANGE_DELAY: Duration = Duration::from_millis(100);

/// Maximum number of connection attempts.
const MAX_CONNECT_ATTEMPTS: usize = 7;

/// Maximum number of attempts to open serial port.
const MAX_OPEN_PORT_ATTEMPTS: usize = 3;

/// Delay between connection retry attempts.
const CONNECT_RETRY_DELAY: Duration = Duration::from_millis(500);

/// Maximum number of download retry attempts.
const MAX_DOWNLOAD_RETRIES: usize = 3;

/// WS63 flasher.
pub struct Ws63Flasher {
    port: SerialPort,
    target_baud: u32,
    late_baud: bool,
    verbose: u8,
}

impl Ws63Flasher {
    /// Create a new WS63 flasher.
    ///
    /// # Arguments
    ///
    /// * `port_name` - Serial port name (e.g., "/dev/ttyUSB0" or "COM3")
    /// * `baud` - Target baud rate for data transfer
    pub fn new(port_name: &str, baud: u32) -> Result<Self> {
        // Open port at default baud rate for handshake, with retry
        let port = Self::open_port_with_retry(port_name, DEFAULT_BAUD)?;

        Ok(Self {
            port,
            target_baud: baud,
            late_baud: false,
            verbose: 0,
        })
    }

    /// Open serial port with retry mechanism.
    fn open_port_with_retry(port_name: &str, baud: u32) -> Result<SerialPort> {
        let mut last_error = None;

        for attempt in 1..=MAX_OPEN_PORT_ATTEMPTS {
            match SerialPort::open(port_name, baud) {
                Ok(port) => {
                    if attempt > 1 {
                        debug!("Port opened on attempt {attempt}");
                    }
                    return Ok(port);
                },
                Err(e) => {
                    warn!(
                        "Failed to open port {port_name} (attempt {attempt}/{MAX_OPEN_PORT_ATTEMPTS}): {e}"
                    );
                    last_error = Some(e);

                    if attempt < MAX_OPEN_PORT_ATTEMPTS {
                        thread::sleep(CONNECT_RETRY_DELAY);
                    }
                },
            }
        }

        Err(last_error.unwrap_or(Error::DeviceNotFound))
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
                        self.port.clear()?;
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
        self.port.clear()?;

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
        self.port.clear()?;

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
                        let _ = self.port.clear();
                        thread::sleep(CONNECT_RETRY_DELAY);
                    } else {
                        return Err(e);
                    }
                },
            }
        }

        Err(last_error.unwrap_or(Error::Protocol("Download failed".into())))
    }

    /// Single attempt to download a binary.
    #[allow(clippy::cast_possible_truncation)]
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
        // Safe cast: firmware images are always < 4GB
        let len = data.len() as u32;
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

#[cfg(test)]
mod tests {
    // Integration tests would require actual hardware
    // Unit tests for internal functions can be added here
}
