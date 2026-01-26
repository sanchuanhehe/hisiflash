//! WS63 boot protocol implementation.
//!
//! This module implements the HiSilicon boot protocol used by the WS63 chip.
//!
//! ## Frame Format
//!
//! ```text
//! +------------+--------+-----+------+---------------+--------+
//! |   Magic    | Length | CMD | SCMD |     Data      | CRC16  |
//! +------------+--------+-----+------+---------------+--------+
//! |   4 bytes  | 2 bytes| 1   | 1    |   variable    | 2 bytes|
//! +------------+--------+-----+------+---------------+--------+
//! | 0xDEADBEEF |  total | cmd | ~cmd |   payload     | CRC    |
//! +------------+--------+-----+------+---------------+--------+
//! ```

use crate::protocol::crc::crc16_xmodem;
use byteorder::{LittleEndian, WriteBytesExt};

/// Frame magic number.
pub const FRAME_MAGIC: u32 = 0xDEADBEEF;

/// Default initial baud rate for handshake.
pub const DEFAULT_BAUD: u32 = 115200;

/// High-speed baud rate after handshake.
pub const HIGH_BAUD: u32 = 921600;

/// Handshake ACK magic (first 10 bytes of successful handshake response).
pub const HANDSHAKE_ACK: [u8; 10] = [
    0xEF, 0xBE, 0xAD, 0xDE, // Magic (little-endian)
    0x0C, 0x00, // Length = 12
    0xE1, 0x1E, // CMD = 0xE1, SCMD = 0x1E (swapped 0x0F)
    0x5A, 0x00, // ACK = 0x5A (success)
];

/// WS63 command types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Command {
    /// Handshake command (establish connection).
    Handshake = 0xF0,
    /// Set baud rate command.
    SetBaudRate = 0x5A,
    /// Download/erase command.
    Download = 0xD2,
    /// Reset command.
    Reset = 0x87,
}

impl Command {
    /// Get the swapped command byte (SCMD).
    /// SCMD = (CMD << 4) | (CMD >> 4)
    pub fn swapped(self) -> u8 {
        let cmd = self as u8;
        cmd.rotate_right(4)
    }
}

/// Command frame builder.
#[derive(Debug)]
pub struct CommandFrame {
    cmd: Command,
    data: Vec<u8>,
}

impl CommandFrame {
    /// Create a new command frame.
    pub fn new(cmd: Command) -> Self {
        Self {
            cmd,
            data: Vec::new(),
        }
    }

    /// Create a handshake command frame.
    ///
    /// # Arguments
    ///
    /// * `baud` - The baud rate to use for communication.
    #[allow(clippy::unwrap_used)] // Writing to Vec<u8> cannot fail
    pub fn handshake(baud: u32) -> Self {
        let mut frame = Self::new(Command::Handshake);
        frame.data.write_u32::<LittleEndian>(baud).unwrap();
        frame.data.write_u32::<LittleEndian>(0x0108).unwrap(); // Magic constant
        frame
    }

    /// Create a set baud rate command frame.
    #[allow(clippy::unwrap_used)] // Writing to Vec<u8> cannot fail
    pub fn set_baud_rate(baud: u32) -> Self {
        let mut frame = Self::new(Command::SetBaudRate);
        frame.data.write_u32::<LittleEndian>(baud).unwrap();
        frame.data.write_u32::<LittleEndian>(0x0108).unwrap();
        frame
    }

    /// Create a download command frame.
    ///
    /// # Arguments
    ///
    /// * `addr` - Flash address to write to.
    /// * `len` - Data length.
    /// * `erase_size` - Size to erase (0xFFFFFFFF for full erase).
    #[allow(clippy::unwrap_used)] // Writing to Vec<u8> cannot fail
    pub fn download(addr: u32, len: u32, erase_size: u32) -> Self {
        let mut frame = Self::new(Command::Download);
        frame.data.write_u32::<LittleEndian>(addr).unwrap();
        frame.data.write_u32::<LittleEndian>(len).unwrap();
        frame.data.write_u32::<LittleEndian>(erase_size).unwrap();
        frame.data.extend_from_slice(&[0x00, 0xFF]); // Constant bytes
        frame
    }

    /// Create an erase-all command frame.
    pub fn erase_all() -> Self {
        Self::download(0, 0, 0xFFFFFFFF)
    }

    /// Create a reset command frame.
    pub fn reset() -> Self {
        let mut frame = Self::new(Command::Reset);
        frame.data.extend_from_slice(&[0x00, 0x00]);
        frame
    }

    /// Build the complete frame data.
    #[allow(clippy::cast_possible_truncation)]
    #[allow(clippy::unwrap_used)] // Writing to Vec<u8> cannot fail
    pub fn build(&self) -> Vec<u8> {
        // Total length = Magic(4) + Len(2) + CMD(1) + SCMD(1) + Data + CRC(2)
        let total_len = 10 + self.data.len();
        let mut buf = Vec::with_capacity(total_len);

        // Magic (little-endian)
        buf.write_u32::<LittleEndian>(FRAME_MAGIC).unwrap();

        // Length - safe cast, frame size < 64KB
        buf.write_u16::<LittleEndian>(total_len as u16).unwrap();

        // CMD + SCMD
        buf.push(self.cmd as u8);
        buf.push(self.cmd.swapped());

        // Data
        buf.extend_from_slice(&self.data);

        // CRC16 (calculated over everything before CRC)
        let crc = crc16_xmodem(&buf);
        buf.write_u16::<LittleEndian>(crc).unwrap();

        buf
    }

    /// Get the command type.
    pub fn command(&self) -> Command {
        self.cmd
    }
}

/// Response frame parser.
#[derive(Debug)]
pub struct ResponseFrame {
    /// Command byte from response.
    pub cmd: u8,
    /// Sub-command byte from response.
    pub scmd: u8,
    /// Response data.
    pub data: Vec<u8>,
}

impl ResponseFrame {
    /// Parse a response frame from raw data.
    ///
    /// Returns `None` if the data is not a valid frame.
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 10 {
            return None;
        }

        // Find magic
        let magic_pos = data
            .windows(4)
            .position(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]) == FRAME_MAGIC)?;

        let frame = &data[magic_pos..];
        if frame.len() < 10 {
            return None;
        }

        let len = u16::from_le_bytes([frame[4], frame[5]]) as usize;
        if frame.len() < len {
            return None;
        }

        let cmd = frame[6];
        let scmd = frame[7];
        let data = frame[8..len - 2].to_vec();

        Some(Self { cmd, scmd, data })
    }

    /// Check if this is a successful handshake ACK.
    pub fn is_handshake_ack(&self) -> bool {
        // CMD = 0xE1 (response to 0x0F), first data byte = 0x5A (ACK)
        self.cmd == 0xE1 && !self.data.is_empty() && self.data[0] == 0x5A
    }

    /// Check if this is a successful ACK response.
    pub fn is_ack(&self) -> bool {
        !self.data.is_empty() && self.data[0] == 0x5A
    }
}

/// Check if data contains the handshake ACK pattern.
pub fn contains_handshake_ack(data: &[u8]) -> bool {
    data.windows(HANDSHAKE_ACK.len())
        .any(|w| w == HANDSHAKE_ACK)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_swapped() {
        assert_eq!(Command::Handshake.swapped(), 0x0F);
        assert_eq!(Command::SetBaudRate.swapped(), 0xA5);
        assert_eq!(Command::Download.swapped(), 0x2D);
        assert_eq!(Command::Reset.swapped(), 0x78);
    }

    #[test]
    fn test_handshake_frame() {
        let frame = CommandFrame::handshake(115200);
        let data = frame.build();

        // Check magic
        assert_eq!(&data[0..4], &[0xEF, 0xBE, 0xAD, 0xDE]);

        // Check CMD and SCMD
        assert_eq!(data[6], 0xF0);
        assert_eq!(data[7], 0x0F);
    }

    #[test]
    fn test_download_frame() {
        let frame = CommandFrame::download(0x00800000, 0x1000, 0x1000);
        let data = frame.build();

        assert_eq!(&data[0..4], &[0xEF, 0xBE, 0xAD, 0xDE]);
        assert_eq!(data[6], 0xD2);
        assert_eq!(data[7], 0x2D);
    }

    #[test]
    fn test_erase_all_frame() {
        let frame = CommandFrame::erase_all();
        let data = frame.build();

        // Check that erase_size is 0xFFFFFFFF
        // Data layout: addr(4) + len(4) + erase_size(4) + const(2)
        // erase_size is at offset 8 + 8 = 16
        assert_eq!(&data[16..20], &[0xFF, 0xFF, 0xFF, 0xFF]);
    }

    #[test]
    fn test_contains_handshake_ack() {
        // Should find ACK in exact match
        assert!(contains_handshake_ack(&HANDSHAKE_ACK));

        // Should find ACK with garbage before/after
        let mut data = vec![0x00, 0x00];
        data.extend_from_slice(&HANDSHAKE_ACK);
        data.extend_from_slice(&[0x00, 0x00]);
        assert!(contains_handshake_ack(&data));

        // Should not find ACK in random data
        assert!(!contains_handshake_ack(&[0x00; 20]));
    }
}
