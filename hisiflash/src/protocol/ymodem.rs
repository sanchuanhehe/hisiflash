//! YMODEM-1K file transfer protocol implementation.
//!
//! This module implements the YMODEM-1K protocol for file transfer,
//! which is used by HiSilicon boot loaders for firmware transfer.
//!
//! ## Protocol Overview
//!
//! YMODEM-1K uses 1024-byte data blocks with CRC16 error detection:
//!
//! ```text
//! Block format:
//! +-----+-----+------+---------------+--------+
//! | STX | SEQ | ~SEQ |   DATA (1024) | CRC16  |
//! +-----+-----+------+---------------+--------+
//! | 1   | 1   | 1    |     1024      | 2      |
//! +-----+-----+------+---------------+--------+
//! ```

use crate::error::{Error, Result};
use crate::protocol::crc::crc16_xmodem;
use log::{debug, trace};
use std::io::{Read, Write};
use std::time::Duration;

/// YMODEM control characters.
pub mod control {
    /// Start of Header (128-byte block).
    pub const SOH: u8 = 0x01;
    /// Start of Text (1024-byte block).
    pub const STX: u8 = 0x02;
    /// End of Transmission.
    pub const EOT: u8 = 0x04;
    /// Acknowledge.
    pub const ACK: u8 = 0x06;
    /// Not Acknowledge.
    pub const NAK: u8 = 0x15;
    /// Cancel.
    pub const CAN: u8 = 0x18;
    /// CRC mode request character.
    pub const C: u8 = b'C';
}

/// Block size for SOH packets.
pub const SOH_BLOCK_SIZE: usize = 128;

/// Block size for STX packets (YMODEM-1K).
pub const STX_BLOCK_SIZE: usize = 1024;

/// YMODEM configuration options.
#[derive(Debug, Clone)]
pub struct YmodemConfig {
    /// Timeout for waiting for a character.
    pub char_timeout: Duration,
    /// Timeout for waiting for 'C' character.
    pub c_timeout: Duration,
    /// Maximum retries for sending a block.
    pub max_retries: u32,
    /// Verbose output level.
    pub verbose: u8,
}

impl Default for YmodemConfig {
    fn default() -> Self {
        Self {
            char_timeout: Duration::from_millis(1000),
            c_timeout: Duration::from_secs(60),
            max_retries: 10,
            verbose: 0,
        }
    }
}

/// YMODEM transfer handler.
pub struct YmodemTransfer<'a, P: Read + Write> {
    port: &'a mut P,
    config: YmodemConfig,
}

impl<'a, P: Read + Write> YmodemTransfer<'a, P> {
    /// Create a new YMODEM transfer handler.
    pub fn new(port: &'a mut P) -> Self {
        Self {
            port,
            config: YmodemConfig::default(),
        }
    }

    /// Create a new YMODEM transfer handler with custom configuration.
    pub fn with_config(port: &'a mut P, config: YmodemConfig) -> Self {
        Self { port, config }
    }

    /// Read a single byte with timeout.
    fn read_byte(&mut self, _timeout: Duration) -> Result<u8> {
        let mut buf = [0u8; 1];
        // Note: Actual timeout handling depends on the port implementation.
        // serialport crate handles this internally.
        match self.port.read(&mut buf) {
            Ok(1) => Ok(buf[0]),
            Ok(_) => Err(Error::Timeout("read_byte: no data".into())),
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {
                Err(Error::Timeout("read_byte: timeout".into()))
            },
            Err(e) => Err(Error::Io(e)),
        }
    }

    /// Wait for the receiver to send 'C' (CRC mode request).
    pub fn wait_for_c(&mut self) -> Result<()> {
        debug!("Waiting for 'C' from receiver...");
        let start = std::time::Instant::now();

        while start.elapsed() < self.config.c_timeout {
            match self.read_byte(self.config.char_timeout) {
                Ok(control::C) => {
                    debug!("Received 'C', starting transfer");
                    return Ok(());
                },
                Ok(c) => {
                    trace!("Received unexpected char: 0x{c:02X}");
                },
                Err(Error::Timeout(_)) => {},
                Err(e) => return Err(e),
            }
        }

        Err(Error::Timeout("Timeout waiting for 'C'".into()))
    }

    /// Build a YMODEM block.
    fn build_block(seq: u8, data: &[u8], use_stx: bool) -> Vec<u8> {
        let block_size = if use_stx {
            STX_BLOCK_SIZE
        } else {
            SOH_BLOCK_SIZE
        };
        let header = if use_stx { control::STX } else { control::SOH };

        let mut block = Vec::with_capacity(3 + block_size + 2);

        // Header
        block.push(header);
        block.push(seq);
        block.push(!seq);

        // Data (padded with 0x00 if necessary)
        if data.len() >= block_size {
            block.extend_from_slice(&data[..block_size]);
        } else {
            block.extend_from_slice(data);
            block.resize(3 + block_size, 0x00);
        }

        // CRC16
        let crc = crc16_xmodem(&block[3..3 + block_size]);
        block.push((crc >> 8) as u8);
        block.push((crc & 0xFF) as u8);

        block
    }

    /// Send a block and wait for ACK.
    fn send_block(&mut self, block: &[u8]) -> Result<()> {
        for retry in 0..self.config.max_retries {
            trace!("Sending block (attempt {})", retry + 1);

            self.port.write_all(block)?;
            self.port.flush()?;

            match self.read_byte(self.config.char_timeout) {
                Ok(control::ACK) => {
                    trace!("Block ACKed");
                    return Ok(());
                },
                Ok(control::NAK) => {
                    debug!("Block NAKed, retrying...");
                },
                Ok(control::CAN) => {
                    return Err(Error::Ymodem("Transfer cancelled by receiver".into()));
                },
                Ok(c) => {
                    debug!("Unexpected response: 0x{c:02X}, retrying...");
                },
                Err(Error::Timeout(_)) => {
                    debug!("Timeout waiting for ACK, retrying...");
                },
                Err(e) => return Err(e),
            }
        }

        Err(Error::Ymodem(format!(
            "Block transfer failed after {} retries",
            self.config.max_retries
        )))
    }

    /// Send file information block (block 0).
    ///
    /// Format: `filename\0filesize\0`
    pub fn send_file_info(&mut self, filename: &str, filesize: usize) -> Result<()> {
        debug!("Sending file info: {filename} ({filesize} bytes)");

        // Build block 0 data
        let mut data = Vec::with_capacity(SOH_BLOCK_SIZE);
        data.extend_from_slice(filename.as_bytes());
        data.push(0x00);
        data.extend_from_slice(filesize.to_string().as_bytes());
        data.push(0x00);

        let block = Self::build_block(0, &data, false);
        self.send_block(&block)
    }

    /// Send EOT (End of Transmission).
    pub fn send_eot(&mut self) -> Result<()> {
        debug!("Sending EOT");

        for _retry in 0..self.config.max_retries {
            self.port.write_all(&[control::EOT])?;
            self.port.flush()?;

            match self.read_byte(self.config.char_timeout) {
                Ok(control::ACK) => {
                    debug!("EOT ACKed");
                    return Ok(());
                },
                Ok(control::C) => {
                    // Receiver is ready for next file, we're done
                    return Ok(());
                },
                // NAK, timeout, or unexpected response - retry
                Ok(_) | Err(Error::Timeout(_)) => {},
                Err(e) => return Err(e),
            }
        }

        // Consider EOT sent even without ACK
        Ok(())
    }

    /// Send finish block (empty block 0 to end session).
    pub fn send_finish(&mut self) -> Result<()> {
        debug!("Sending finish block");
        let block = Self::build_block(0, &[], false);
        self.send_block(&block)
    }

    /// Transfer file data.
    ///
    /// # Arguments
    ///
    /// * `filename` - Name of the file being transferred
    /// * `data` - File data to transfer
    /// * `progress` - Optional progress callback (current, total)
    pub fn transfer<F>(&mut self, filename: &str, data: &[u8], mut progress: F) -> Result<()>
    where
        F: FnMut(usize, usize),
    {
        debug!(
            "Starting YMODEM transfer: {} ({} bytes)",
            filename,
            data.len()
        );

        // Wait for receiver to request transfer
        self.wait_for_c()?;

        // Send file info (block 0)
        self.send_file_info(filename, data.len())?;

        // Wait for 'C' again before data
        self.wait_for_c()?;

        // Send data blocks
        let mut seq: u8 = 1;
        let mut offset = 0;
        let total = data.len();

        while offset < total {
            let chunk_end = (offset + STX_BLOCK_SIZE).min(total);
            let chunk = &data[offset..chunk_end];

            let block = Self::build_block(seq, chunk, true);
            self.send_block(&block)?;

            offset = chunk_end;
            seq = seq.wrapping_add(1);

            progress(offset, total);
        }

        // Send EOT
        self.send_eot()?;

        // Wait for 'C' and send finish block
        if let Ok(()) = self.wait_for_c() {
            let _ = self.send_finish();
        }

        debug!("YMODEM transfer complete");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_block_soh() {
        let data = [0x01, 0x02, 0x03];

        let block = YmodemTransfer::<std::io::Cursor<Vec<u8>>>::build_block(1, &data, false);

        assert_eq!(block[0], control::SOH);
        assert_eq!(block[1], 1);
        assert_eq!(block[2], 0xFE);
        assert_eq!(block.len(), 3 + SOH_BLOCK_SIZE + 2);
    }

    #[test]
    fn test_build_block_stx() {
        let data = vec![0xAA; STX_BLOCK_SIZE];

        let block = YmodemTransfer::<std::io::Cursor<Vec<u8>>>::build_block(5, &data, true);

        assert_eq!(block[0], control::STX);
        assert_eq!(block[1], 5);
        assert_eq!(block[2], 0xFA);
        assert_eq!(block.len(), 3 + STX_BLOCK_SIZE + 2);
    }
}
