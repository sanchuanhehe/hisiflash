//! HiSilicon SEBOOT protocol commands and structures.
//!
//! This module implements the official HiSilicon SEBOOT protocol
//! based on the fbb_burntool source code. It supports:
//!
//! - WS63 series
//! - BS2X series (BS21, BS25, etc.)
//! - Other HiSilicon WiFi/BT chips
//!
//! ## Frame Format
//!
//! All SEBOOT commands use the same frame format:
//!
//! ```text
//! +------------+--------+------+-------+---------------+--------+
//! |   Magic    | Length | Type | ~Type |     Data      | CRC16  |
//! +------------+--------+------+-------+---------------+--------+
//! |   4 bytes  | 2 bytes| 1    | 1     |   variable    | 2 bytes|
//! +------------+--------+------+-------+---------------+--------+
//! | 0xDEADBEEF |  total | cmd  | ~cmd  |   payload     | CRC    |
//! +------------+--------+------+-------+---------------+--------+
//! ```

use crate::protocol::crc::crc16_xmodem;
use byteorder::{LittleEndian, WriteBytesExt};

/// Frame magic number (0xDEADBEEF stored as little-endian).
pub const FRAME_MAGIC: u32 = 0xDEADBEEF;

/// Frame magic for FWPKG header (different byte order).
pub const FWPKG_MAGIC: u32 = 0xEFBEADDF;

/// ACK result code for success.
pub const ACK_SUCCESS: u8 = 0x5A;

/// ACK result code for failure.
pub const ACK_FAILURE: u8 = 0x00;

/// SEBOOT command types (frame type field).
///
/// These are the official command codes from HiSilicon's BurnTool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CommandType {
    /// Handshake/connection establishment (0xF0).
    Handshake = 0xF0,

    /// ACK frame type (response from device, 0xE1).
    Ack = 0xE1,

    /// Download flash image (0xD2).
    DownloadFlashImage = 0xD2,

    /// Download OTP/eFuse (0xC3).
    DownloadOtpEfuse = 0xC3,

    /// Upload data from flash (0xB4).
    UploadData = 0xB4,

    /// Read OTP/eFuse (0xA5).
    ReadOtpEfuse = 0xA5,

    /// Flash lock (0x96).
    FlashLock = 0x96,

    /// Reset device (0x87).
    Reset = 0x87,

    /// Download factory bin (0x78).
    DownloadFactoryBin = 0x78,

    /// Download version info (0x69).
    DownloadVersion = 0x69,

    /// Set baud rate (0x5A).
    SetBaudRate = 0x5A,

    /// Download NV data (0x4B).
    DownloadNv = 0x4B,

    /// Switch to DFU mode (0x1E).
    SwitchDfu = 0x1E,
}

impl CommandType {
    /// Get the reversed/complement frame type byte (~cmd).
    pub fn reversed(self) -> u8 {
        !(self as u8)
    }

    /// Get the swapped nibble version (for some commands).
    /// SCMD = (CMD << 4) | (CMD >> 4)
    pub fn swapped(self) -> u8 {
        let cmd = self as u8;
        cmd.rotate_right(4)
    }
}

/// Image/partition types supported by HiSilicon chips.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum ImageType {
    /// Boot loader (first stage).
    Loader = 0,
    /// Normal firmware partition.
    Normal = 1,
    /// Key-Value NV storage.
    KvNv = 2,
    /// eFuse data.
    Efuse = 3,
    /// OTP data.
    Otp = 4,
    /// Flash boot (second stage loader).
    FlashBoot = 5,
    /// Factory calibration data.
    Factory = 6,
    /// Version information.
    Version = 7,
    /// Security partition A.
    SecurityA = 8,
    /// Security partition B.
    SecurityB = 9,
    /// Security partition C.
    SecurityC = 10,
    /// Protocol partition A.
    ProtocolA = 11,
    /// Application partition A.
    AppsA = 12,
    /// Radio configuration.
    RadioConfig = 13,
    /// ROM image.
    Rom = 14,
    /// eMMC image.
    Emmc = 15,
    /// Database.
    Database = 16,
    /// FlashBoot 3892.
    FlashBoot3892 = 17,
}

impl From<u32> for ImageType {
    fn from(value: u32) -> Self {
        match value {
            0 => Self::Loader,
            2 => Self::KvNv,
            3 => Self::Efuse,
            4 => Self::Otp,
            5 => Self::FlashBoot,
            6 => Self::Factory,
            7 => Self::Version,
            // Default to Normal for unknown or value 1
            _ => Self::Normal,
        }
    }
}

/// SEBOOT command frame builder.
///
/// Builds frames according to the official HiSilicon SEBOOT protocol.
#[derive(Debug)]
pub struct SebootFrame {
    frame_type: CommandType,
    data: Vec<u8>,
}

impl SebootFrame {
    /// Create a new frame with the given command type.
    pub fn new(frame_type: CommandType) -> Self {
        Self {
            frame_type,
            data: Vec::new(),
        }
    }

    /// Build handshake frame.
    ///
    /// Frame structure (18 bytes total):
    /// - Magic: 4 bytes (0xDEADBEEF)
    /// - Length: 2 bytes (0x0012 = 18)
    /// - Type: 1 byte (0xF0)
    /// - ~Type: 1 byte (0x0F)
    /// - BaudRate: 4 bytes
    /// - DataBits: 1 byte
    /// - StopBits: 1 byte
    /// - Parity: 1 byte
    /// - FlowCtrl: 1 byte
    /// - CRC16: 2 bytes
    #[allow(clippy::unwrap_used)] // Writing to Vec<u8> cannot fail
    pub fn handshake(baud_rate: u32) -> Self {
        let mut frame = Self::new(CommandType::Handshake);
        frame.data.write_u32::<LittleEndian>(baud_rate).unwrap();
        frame.data.push(8); // DataBits = 8
        frame.data.push(1); // StopBits = 1
        frame.data.push(0); // Parity = None
        frame.data.push(0); // FlowCtrl = None
        frame
    }

    /// Build set baud rate frame.
    #[allow(clippy::unwrap_used)] // Writing to Vec<u8> cannot fail
    pub fn set_baud_rate(baud_rate: u32) -> Self {
        let mut frame = Self::new(CommandType::SetBaudRate);
        frame.data.write_u32::<LittleEndian>(baud_rate).unwrap();
        frame.data.write_u32::<LittleEndian>(0x0108).unwrap(); // Magic constant
        frame
    }

    /// Build download flash image frame.
    ///
    /// Frame structure (24 bytes total):
    /// - Magic: 4 bytes
    /// - Length: 2 bytes (0x0018 = 24)
    /// - Type: 1 byte (0xD2)
    /// - ~Type: 1 byte (0x2D)
    /// - FileAddr: 4 bytes (flash address)
    /// - FileLen: 4 bytes (data length)
    /// - EraseSize: 4 bytes (size to erase, 0xFFFFFFFF for full)
    /// - Formal: 1 byte (0x00 for normal)
    /// - ~Formal: 1 byte (0xFF)
    /// - CRC16: 2 bytes
    #[allow(clippy::unwrap_used)] // Writing to Vec<u8> cannot fail
    pub fn download_flash_image(addr: u32, len: u32, erase_size: u32, is_rom: bool) -> Self {
        let mut frame = Self::new(CommandType::DownloadFlashImage);
        frame.data.write_u32::<LittleEndian>(addr).unwrap();
        frame.data.write_u32::<LittleEndian>(len).unwrap();
        frame.data.write_u32::<LittleEndian>(erase_size).unwrap();
        let formal = u8::from(is_rom);
        frame.data.push(formal);
        frame.data.push(!formal);
        frame
    }

    /// Build download factory bin frame.
    #[allow(clippy::unwrap_used)] // Writing to Vec<u8> cannot fail
    pub fn download_factory_bin(addr: u32, len: u32, erase_size: u32) -> Self {
        let mut frame = Self::new(CommandType::DownloadFactoryBin);
        frame.data.write_u32::<LittleEndian>(addr).unwrap();
        frame.data.write_u32::<LittleEndian>(len).unwrap();
        frame.data.write_u32::<LittleEndian>(erase_size).unwrap();
        frame.data.push(0x00); // formal
        frame.data.push(0xFF); // ~formal
        frame
    }

    /// Build download NV frame.
    #[allow(clippy::unwrap_used)] // Writing to Vec<u8> cannot fail
    pub fn download_nv(addr: u32, len: u32, erase_size: u32, erase_all: bool) -> Self {
        let mut frame = Self::new(CommandType::DownloadNv);
        frame.data.write_u32::<LittleEndian>(addr).unwrap();
        frame.data.write_u32::<LittleEndian>(len).unwrap();
        frame.data.write_u32::<LittleEndian>(erase_size).unwrap();
        frame.data.write_u16::<LittleEndian>(0).unwrap(); // encItemCnt
        frame
            .data
            .write_u16::<LittleEndian>(u16::from(erase_all))
            .unwrap(); // flag
        frame
    }

    /// Build download OTP/eFuse frame.
    #[allow(clippy::unwrap_used)] // Writing to Vec<u8> cannot fail
    pub fn download_otp_efuse(len: u32) -> Self {
        let mut frame = Self::new(CommandType::DownloadOtpEfuse);
        frame.data.write_u32::<LittleEndian>(len).unwrap();
        frame
    }

    /// Build download version frame.
    #[allow(clippy::unwrap_used)] // Writing to Vec<u8> cannot fail
    pub fn download_version(len: u32) -> Self {
        let mut frame = Self::new(CommandType::DownloadVersion);
        frame.data.write_u32::<LittleEndian>(len).unwrap();
        frame
    }

    /// Build upload data frame.
    #[allow(clippy::unwrap_used)] // Writing to Vec<u8> cannot fail
    pub fn upload_data(addr: u32, len: u32) -> Self {
        let mut frame = Self::new(CommandType::UploadData);
        frame.data.write_u32::<LittleEndian>(len).unwrap();
        frame.data.write_u32::<LittleEndian>(addr).unwrap();
        frame
    }

    /// Build read OTP/eFuse frame.
    #[allow(clippy::unwrap_used)] // Writing to Vec<u8> cannot fail
    pub fn read_otp_efuse(start_bit: u16, bit_width: u16) -> Self {
        let mut frame = Self::new(CommandType::ReadOtpEfuse);
        frame.data.write_u16::<LittleEndian>(start_bit).unwrap();
        frame.data.write_u16::<LittleEndian>(bit_width).unwrap();
        frame
    }

    /// Build flash lock frame.
    #[allow(clippy::unwrap_used)] // Writing to Vec<u8> cannot fail
    pub fn flash_lock(param: u16) -> Self {
        let mut frame = Self::new(CommandType::FlashLock);
        frame.data.write_u16::<LittleEndian>(param).unwrap();
        frame
    }

    /// Build reset frame.
    #[allow(clippy::unwrap_used)] // Writing to Vec<u8> cannot fail
    pub fn reset() -> Self {
        let mut frame = Self::new(CommandType::Reset);
        frame.data.write_u16::<LittleEndian>(0).unwrap();
        frame
    }

    /// Build switch to DFU mode frame.
    #[allow(clippy::unwrap_used)] // Writing to Vec<u8> cannot fail
    pub fn switch_dfu() -> Self {
        let mut frame = Self::new(CommandType::SwitchDfu);
        frame.data.write_u16::<LittleEndian>(0).unwrap();
        frame
    }

    /// Build erase all flash frame.
    pub fn erase_all() -> Self {
        Self::download_flash_image(0, 0, 0xFFFFFFFF, false)
    }

    /// Build the complete frame data.
    #[allow(clippy::cast_possible_truncation)]
    #[allow(clippy::unwrap_used)] // Writing to Vec<u8> cannot fail
    pub fn build(&self) -> Vec<u8> {
        // Total length = Magic(4) + Len(2) + Type(1) + ~Type(1) + Data + CRC(2)
        let total_len = 10 + self.data.len();
        let mut buf = Vec::with_capacity(total_len);

        // Magic (little-endian)
        buf.write_u32::<LittleEndian>(FRAME_MAGIC).unwrap();

        // Length (includes everything) - safe cast, frame size < 64KB
        buf.write_u16::<LittleEndian>(total_len as u16).unwrap();

        // Frame type and its complement
        buf.push(self.frame_type as u8);
        buf.push(self.frame_type.reversed());

        // Data payload
        buf.extend_from_slice(&self.data);

        // CRC16 (calculated over everything before CRC)
        let crc = crc16_xmodem(&buf);
        buf.write_u16::<LittleEndian>(crc).unwrap();

        buf
    }

    /// Get the command type.
    pub fn command_type(&self) -> CommandType {
        self.frame_type
    }
}

/// SEBOOT ACK frame parser.
#[derive(Debug)]
pub struct SebootAck {
    /// Frame type from response.
    pub frame_type: u8,
    /// Result code (0x5A = success).
    pub result: u8,
    /// Error code (if result != 0x5A).
    pub error_code: u8,
}

impl SebootAck {
    /// Minimum ACK frame length.
    pub const MIN_LEN: usize = 12;

    /// Expected ACK frame for handshake success.
    pub const HANDSHAKE_ACK: [u8; 12] = [
        0xEF, 0xBE, 0xAD, 0xDE, // Magic (little-endian)
        0x0C, 0x00, // Length = 12
        0xE1, 0x1E, // Type = 0xE1 (ACK), ~Type = 0x1E
        0x5A, 0x00, // Result = 0x5A (success), ErrorCode = 0
        0x00, 0x00, // CRC16 (placeholder)
    ];

    /// Parse an ACK frame from raw data.
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < Self::MIN_LEN {
            return None;
        }

        // Find magic
        let magic_pos = data
            .windows(4)
            .position(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]) == FRAME_MAGIC)?;

        let frame = &data[magic_pos..];
        if frame.len() < Self::MIN_LEN {
            return None;
        }

        let frame_type = frame[6];
        let result = frame[8];
        let error_code = frame[9];

        Some(Self {
            frame_type,
            result,
            error_code,
        })
    }

    /// Check if this is a successful response.
    pub fn is_success(&self) -> bool {
        self.result == ACK_SUCCESS
    }

    /// Check if this is a handshake ACK.
    pub fn is_handshake_ack(&self) -> bool {
        self.frame_type == CommandType::Ack as u8 && self.is_success()
    }
}

/// Check if data contains a valid handshake ACK pattern.
pub fn contains_handshake_ack(data: &[u8]) -> bool {
    // Look for the pattern: Magic + Length(12) + Type(E1) + ~Type(1E) + Result(5A)
    data.windows(10).any(|w| {
        w[0] == 0xEF
            && w[1] == 0xBE
            && w[2] == 0xAD
            && w[3] == 0xDE
            && w[4] == 0x0C
            && w[5] == 0x00
            && w[6] == 0xE1
            && w[7] == 0x1E
            && w[8] == 0x5A
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_type_reversed() {
        assert_eq!(CommandType::Handshake.reversed(), 0x0F);
        assert_eq!(CommandType::DownloadFlashImage.reversed(), 0x2D);
        assert_eq!(CommandType::Reset.reversed(), 0x78);
    }

    #[test]
    fn test_handshake_frame_length() {
        let frame = SebootFrame::handshake(115200);
        let data = frame.build();
        // Handshake frame should be 18 bytes
        assert_eq!(data.len(), 18);
        // Check magic
        assert_eq!(&data[0..4], &[0xEF, 0xBE, 0xAD, 0xDE]);
        // Check length field
        assert_eq!(&data[4..6], &[0x12, 0x00]); // 18 in little-endian
        // Check frame type
        assert_eq!(data[6], 0xF0);
        assert_eq!(data[7], 0x0F);
    }

    #[test]
    fn test_download_flash_image_frame() {
        let frame = SebootFrame::download_flash_image(0x00800000, 0x1000, 0x1000, false);
        let data = frame.build();
        // Download flash image frame should be 24 bytes
        assert_eq!(data.len(), 24);
        // Check frame type
        assert_eq!(data[6], 0xD2);
        assert_eq!(data[7], 0x2D);
    }

    #[test]
    fn test_erase_all_frame() {
        let frame = SebootFrame::erase_all();
        let data = frame.build();
        // Check erase_size is 0xFFFFFFFF
        assert_eq!(&data[16..20], &[0xFF, 0xFF, 0xFF, 0xFF]);
    }

    #[test]
    fn test_reset_frame() {
        let frame = SebootFrame::reset();
        let data = frame.build();
        // Reset frame should be 12 bytes
        assert_eq!(data.len(), 12);
        assert_eq!(data[6], 0x87);
        assert_eq!(data[7], 0x78);
    }

    #[test]
    fn test_contains_handshake_ack() {
        let mut data = vec![0x00, 0x00];
        data.extend_from_slice(&SebootAck::HANDSHAKE_ACK[..10]);
        data.extend_from_slice(&[0x00, 0x00]);
        assert!(contains_handshake_ack(&data));
    }
}
