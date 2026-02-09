//! FWPKG firmware package format.
//!
//! FWPKG is HiSilicon's firmware package format, containing multiple
//! binary images and their metadata.
//!
//! ## Format Versions
//!
//! There are two generations of FWPKG format:
//!
//! ### V1 (Original Format)
//! - Magic: `0xEFBEADDF`
//! - Header: 12 bytes (no package name)
//! - BinInfo: 52 bytes (32-byte name field)
//!
//! ### V2 (New Format)
//! - Magic: `0xEFBEADD0` ~ `0xEFBEADDE`
//! - Header: 272 bytes (includes 260-byte package name)
//! - BinInfo: 284 bytes (260-byte name field, supports UTF-8)
//!
//! ## V1 Format Overview
//!
//! ```text
//! +------------------+
//! |   Header (12B)   |
//! +------------------+
//! |  BinInfo[0] 52B  |
//! +------------------+
//! |  BinInfo[1] 52B  |
//! +------------------+
//! |       ...        |
//! +------------------+
//! |  BinInfo[n] 52B  |
//! +------------------+
//! |   Binary Data    |
//! |       ...        |
//! +------------------+
//! ```
//!
//! ## V2 Format Overview
//!
//! ```text
//! +--------------------+
//! |   Header (272B)    |
//! |  (includes name)   |
//! +--------------------+
//! |  BinInfo[0] 284B   |
//! +--------------------+
//! |  BinInfo[1] 284B   |
//! +--------------------+
//! |        ...         |
//! +--------------------+
//! |  BinInfo[n] 284B   |
//! +--------------------+
//! |    Binary Data     |
//! |        ...         |
//! +--------------------+
//! ```

use crate::error::{Error, Result};
use crate::protocol::crc::crc16_xmodem;
use byteorder::{LittleEndian, ReadBytesExt};
use log::debug;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

/// FWPKG V1 magic number (little-endian).
/// Stored as 0xDFADBEEF, reads as 0xEFBEADDF.
pub const FWPKG_MAGIC_V1: u32 = 0xEFBEADDF;

/// FWPKG V2 magic number range: 0xEFBEADD0 ~ 0xEFBEADDE.
pub const FWPKG_MAGIC_V2_MIN: u32 = 0xEFBEADD0;
/// FWPKG V2 magic number range maximum: 0xEFBEADDE.
pub const FWPKG_MAGIC_V2_MAX: u32 = 0xEFBEADDE;

/// Legacy alias for V1 magic.
pub const FWPKG_MAGIC: u32 = FWPKG_MAGIC_V1;

/// Maximum number of partitions in a FWPKG.
pub const MAX_PARTITIONS: usize = 255;

/// V1 Header size in bytes.
pub const HEADER_SIZE_V1: usize = 12;

/// V2 Header size in bytes (includes 260-byte name).
pub const HEADER_SIZE_V2: usize = 272;

/// Legacy alias for V1 header size.
pub const HEADER_SIZE: usize = HEADER_SIZE_V1;

/// V1 BinInfo size in bytes.
/// name\[32\] + offset(4) + length(4) + burn_addr(4) + burn_size(4) + type(4) = 52
pub const BIN_INFO_SIZE_V1: usize = 52;

/// V2 BinInfo size in bytes.
pub const BIN_INFO_SIZE_V2: usize = 284;

/// Legacy alias for V1 BinInfo size.
pub const BIN_INFO_SIZE: usize = BIN_INFO_SIZE_V1;

/// V1 name field size.
pub const NAME_SIZE_V1: usize = 32;

/// V2 name field size.
pub const NAME_SIZE_V2: usize = 260;

/// FWPKG format version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FwpkgVersion {
    /// V1: Original format with 32-byte names.
    V1,
    /// V2: New format with 260-byte names and package name.
    V2,
}

/// FWPKG file header.
///
/// V1: 12 bytes (no name field)
/// V2: 272 bytes (includes 260-byte name field)
#[derive(Debug, Clone)]
pub struct FwpkgHeader {
    /// Magic number (V1: 0xEFBEADDF, V2: 0xEFBEADD0~0xEFBEADDE).
    pub magic: u32,
    /// CRC16-XMODEM checksum (starting from cnt field).
    pub crc: u16,
    /// Number of partitions.
    pub cnt: u16,
    /// Total firmware size.
    pub len: u32,
    /// Package name (V2 only, empty for V1).
    pub name: String,
    /// Format version.
    pub version: FwpkgVersion,
}

impl FwpkgHeader {
    /// Read V1 header from a reader (12 bytes).
    pub fn read_v1<R: Read>(reader: &mut R) -> Result<Self> {
        let magic = reader.read_u32::<LittleEndian>()?;
        let crc = reader.read_u16::<LittleEndian>()?;
        let cnt = reader.read_u16::<LittleEndian>()?;
        let len = reader.read_u32::<LittleEndian>()?;

        Ok(Self {
            magic,
            crc,
            cnt,
            len,
            name: String::new(),
            version: FwpkgVersion::V1,
        })
    }

    /// Read V2 header from a reader (272 bytes).
    pub fn read_v2<R: Read>(reader: &mut R) -> Result<Self> {
        let magic = reader.read_u32::<LittleEndian>()?;
        let crc = reader.read_u16::<LittleEndian>()?;
        let cnt = reader.read_u16::<LittleEndian>()?;
        let len = reader.read_u32::<LittleEndian>()?;

        // Read 260-byte name field
        let mut name_bytes = [0u8; NAME_SIZE_V2];
        reader.read_exact(&mut name_bytes)?;
        let name_end = name_bytes
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(NAME_SIZE_V2);
        let name = String::from_utf8_lossy(&name_bytes[..name_end]).to_string();

        Ok(Self {
            magic,
            crc,
            cnt,
            len,
            name,
            version: FwpkgVersion::V2,
        })
    }

    /// Read header from a reader (auto-detect version).
    pub fn read_from<R: Read>(reader: &mut R) -> Result<Self> {
        // First read the magic to determine version
        let magic = reader.read_u32::<LittleEndian>()?;
        let crc = reader.read_u16::<LittleEndian>()?;
        let cnt = reader.read_u16::<LittleEndian>()?;
        let len = reader.read_u32::<LittleEndian>()?;

        // Detect version based on magic
        let (name, version) = if magic == FWPKG_MAGIC_V1 {
            (String::new(), FwpkgVersion::V1)
        } else if (FWPKG_MAGIC_V2_MIN..=FWPKG_MAGIC_V2_MAX).contains(&magic) {
            // V2: read the 260-byte name field
            let mut name_bytes = [0u8; NAME_SIZE_V2];
            reader.read_exact(&mut name_bytes)?;
            let name_end = name_bytes
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(NAME_SIZE_V2);
            let name = String::from_utf8_lossy(&name_bytes[..name_end]).to_string();
            (name, FwpkgVersion::V2)
        } else {
            // Invalid magic, but still return for error reporting
            (String::new(), FwpkgVersion::V1)
        };

        Ok(Self {
            magic,
            crc,
            cnt,
            len,
            name,
            version,
        })
    }

    /// Check if the magic number is valid.
    pub fn is_valid(&self) -> bool {
        let valid_magic = self.magic == FWPKG_MAGIC_V1
            || (FWPKG_MAGIC_V2_MIN..=FWPKG_MAGIC_V2_MAX).contains(&self.magic);
        valid_magic && (self.cnt as usize) <= MAX_PARTITIONS
    }

    /// Get the header size based on version.
    pub fn header_size(&self) -> usize {
        match self.version {
            FwpkgVersion::V1 => HEADER_SIZE_V1,
            FwpkgVersion::V2 => HEADER_SIZE_V2,
        }
    }

    /// Get the BinInfo size based on version.
    pub fn bin_info_size(&self) -> usize {
        match self.version {
            FwpkgVersion::V1 => BIN_INFO_SIZE_V1,
            FwpkgVersion::V2 => BIN_INFO_SIZE_V2,
        }
    }
}

/// Partition/Image type.
///
/// Based on HiSilicon's IMAGE_TYPE enum from fbb_burntool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum PartitionType {
    /// LoaderBoot (first-stage bootloader).
    Loader = 0,
    /// Normal firmware partition.
    Normal = 1,
    /// Key-Value NV storage.
    KvNv = 2,
    /// eFuse data.
    Efuse = 3,
    /// OTP data.
    Otp = 4,
    /// FlashBoot (second-stage bootloader).
    Flashboot = 5,
    /// Factory data.
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
    /// Apps partition A.
    AppsA = 12,
    /// Radio configuration.
    RadioConfig = 13,
    /// ROM image.
    Rom = 14,
    /// eMMC image.
    Emmc = 15,
    /// Database (typically skipped in UI).
    Database = 16,
    /// Unknown partition type.
    Unknown(u32),
}

impl From<u32> for PartitionType {
    fn from(value: u32) -> Self {
        match value {
            0 => Self::Loader,
            1 => Self::Normal,
            2 => Self::KvNv,
            3 => Self::Efuse,
            4 => Self::Otp,
            5 => Self::Flashboot,
            6 => Self::Factory,
            7 => Self::Version,
            8 => Self::SecurityA,
            9 => Self::SecurityB,
            10 => Self::SecurityC,
            11 => Self::ProtocolA,
            12 => Self::AppsA,
            13 => Self::RadioConfig,
            14 => Self::Rom,
            15 => Self::Emmc,
            16 => Self::Database,
            v => Self::Unknown(v),
        }
    }
}

impl PartitionType {
    /// Returns the numeric value of this partition type.
    pub fn as_u32(&self) -> u32 {
        match self {
            Self::Loader => 0,
            Self::Normal => 1,
            Self::KvNv => 2,
            Self::Efuse => 3,
            Self::Otp => 4,
            Self::Flashboot => 5,
            Self::Factory => 6,
            Self::Version => 7,
            Self::SecurityA => 8,
            Self::SecurityB => 9,
            Self::SecurityC => 10,
            Self::ProtocolA => 11,
            Self::AppsA => 12,
            Self::RadioConfig => 13,
            Self::Rom => 14,
            Self::Emmc => 15,
            Self::Database => 16,
            Self::Unknown(v) => *v,
        }
    }

    /// Alias for Loader (for backward compatibility).
    #[allow(non_upper_case_globals)]
    pub const LoaderBoot: Self = Self::Loader;
}

/// FWPKG partition information.
///
/// V1: 52 bytes (32-byte name)
/// V2: 284 bytes (260-byte name)
#[derive(Debug, Clone)]
pub struct FwpkgBinInfo {
    /// Partition name (max 31 chars for V1, 259 chars for V2).
    pub name: String,
    /// Offset within the FWPKG file.
    pub offset: u32,
    /// Data length.
    pub length: u32,
    /// Burn address in flash.
    pub burn_addr: u32,
    /// Burn size (may differ from length due to alignment).
    pub burn_size: u32,
    /// Partition type.
    pub partition_type: PartitionType,
}

impl FwpkgBinInfo {
    /// Read V1 BinInfo from a reader (52 bytes).
    pub fn read_v1<R: Read>(reader: &mut R) -> Result<Self> {
        let mut name_bytes = [0u8; NAME_SIZE_V1];
        reader.read_exact(&mut name_bytes)?;

        // Find NUL terminator
        let name_end = name_bytes
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(NAME_SIZE_V1);
        let name = String::from_utf8_lossy(&name_bytes[..name_end]).to_string();

        let offset = reader.read_u32::<LittleEndian>()?;
        let length = reader.read_u32::<LittleEndian>()?;
        let burn_addr = reader.read_u32::<LittleEndian>()?;
        let burn_size = reader.read_u32::<LittleEndian>()?;
        let type_value = reader.read_u32::<LittleEndian>()?;

        Ok(Self {
            name,
            offset,
            length,
            burn_addr,
            burn_size,
            partition_type: type_value.into(),
        })
    }

    /// Read V2 BinInfo from a reader (284 bytes).
    pub fn read_v2<R: Read>(reader: &mut R) -> Result<Self> {
        let mut name_bytes = [0u8; NAME_SIZE_V2];
        reader.read_exact(&mut name_bytes)?;

        // Find NUL terminator - V2 uses UTF-8
        let name_end = name_bytes
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(NAME_SIZE_V2);
        let name = String::from_utf8_lossy(&name_bytes[..name_end]).to_string();

        let offset = reader.read_u32::<LittleEndian>()?;
        let length = reader.read_u32::<LittleEndian>()?;
        let burn_addr = reader.read_u32::<LittleEndian>()?;
        let burn_size = reader.read_u32::<LittleEndian>()?;
        let type_value = reader.read_u32::<LittleEndian>()?;

        // V2 BinInfo: 260 + 4*5 = 280 bytes, so 4 bytes padding
        let mut padding = [0u8; 4];
        reader.read_exact(&mut padding)?;

        Ok(Self {
            name,
            offset,
            length,
            burn_addr,
            burn_size,
            partition_type: type_value.into(),
        })
    }

    /// Read BinInfo from a reader based on version.
    pub fn read_from<R: Read>(reader: &mut R, version: FwpkgVersion) -> Result<Self> {
        match version {
            FwpkgVersion::V1 => Self::read_v1(reader),
            FwpkgVersion::V2 => Self::read_v2(reader),
        }
    }

    /// Check if this is the LoaderBoot partition.
    pub fn is_loaderboot(&self) -> bool {
        self.partition_type == PartitionType::Loader
    }
}

/// Parsed FWPKG firmware package.
pub struct Fwpkg {
    /// File header.
    pub header: FwpkgHeader,
    /// Partition information.
    pub bins: Vec<FwpkgBinInfo>,
    /// Raw file data.
    data: Vec<u8>,
}

impl Fwpkg {
    /// Load a FWPKG from a file.
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        debug!("Loading FWPKG from: {}", path.display());

        let file = File::open(path)?;
        let mut reader = BufReader::new(file);

        // Read all data into memory
        let mut data = Vec::new();
        reader.read_to_end(&mut data)?;

        Self::from_bytes(data)
    }

    /// Parse a FWPKG from raw bytes.
    pub fn from_bytes(data: Vec<u8>) -> Result<Self> {
        if data.len() < HEADER_SIZE_V1 {
            return Err(Error::InvalidFwpkg("File too small for header".into()));
        }

        let mut cursor = std::io::Cursor::new(&data);

        // Read header (auto-detects version)
        let header = FwpkgHeader::read_from(&mut cursor)?;

        if !header.is_valid() {
            return Err(Error::InvalidFwpkg(format!(
                "Invalid magic: expected {:#010X} (V1) or {:#010X}~{:#010X} (V2), got {:#010X}",
                FWPKG_MAGIC_V1, FWPKG_MAGIC_V2_MIN, FWPKG_MAGIC_V2_MAX, header.magic
            )));
        }

        debug!(
            "FWPKG {:?} header: {} partitions, {} bytes total{}",
            header.version,
            header.cnt,
            header.len,
            if header.name.is_empty() {
                String::new()
            } else {
                format!(", name: {}", header.name)
            }
        );

        // Read partition info
        let bin_count = header.cnt as usize;
        let expected_size = header.header_size() + bin_count * header.bin_info_size();

        if data.len() < expected_size {
            return Err(Error::InvalidFwpkg(format!(
                "File too small for {} partitions (need {} bytes, got {})",
                bin_count,
                expected_size,
                data.len()
            )));
        }

        let mut bins = Vec::with_capacity(bin_count);
        for i in 0..bin_count {
            let bin_info = FwpkgBinInfo::read_from(&mut cursor, header.version)?;
            debug!(
                "  [{}] {} @ 0x{:08X}, {} bytes -> 0x{:08X} (type: {:?})",
                i,
                bin_info.name,
                bin_info.offset,
                bin_info.length,
                bin_info.burn_addr,
                bin_info.partition_type
            );
            bins.push(bin_info);
        }

        Ok(Self { header, bins, data })
    }

    /// Get the format version.
    pub fn version(&self) -> FwpkgVersion {
        self.header.version
    }

    /// Get the package name (V2 only, empty for V1).
    pub fn package_name(&self) -> &str {
        &self.header.name
    }

    /// Get the LoaderBoot partition, if present.
    pub fn loaderboot(&self) -> Option<&FwpkgBinInfo> {
        self.bins.iter().find(|b| b.is_loaderboot())
    }

    /// Get all normal (non-LoaderBoot) partitions.
    pub fn normal_bins(&self) -> impl Iterator<Item = &FwpkgBinInfo> {
        self.bins.iter().filter(|b| !b.is_loaderboot())
    }

    /// Get the binary data for a partition.
    pub fn bin_data(&self, bin: &FwpkgBinInfo) -> Result<&[u8]> {
        let start = bin.offset as usize;
        let end = start + bin.length as usize;

        if end > self.data.len() {
            return Err(Error::InvalidFwpkg(format!(
                "Partition {} data out of bounds (offset {}, length {}, file size {})",
                bin.name,
                bin.offset,
                bin.length,
                self.data.len()
            )));
        }

        Ok(&self.data[start..end])
    }

    /// Verify the CRC checksum.
    ///
    /// CRC is calculated from the `cnt` field onwards (excluding magic and crc fields).
    /// For V1: covers cnt(2) + len(4) + BinInfo[] (header total - 6 bytes)
    /// For V2: covers cnt(2) + len(4) + name(260) + BinInfo[] (header total - 6 bytes)
    pub fn verify_crc(&self) -> Result<()> {
        let header_size = self.header.header_size();
        if self.data.len() < header_size {
            return Err(Error::InvalidFwpkg("File too small".into()));
        }

        // CRC covers: everything after magic(4) + crc(2), up to end of BinInfo array
        // Per fbb_burntool: crcDataLen = sizeof(FWPKG_HEAD) - 6 + sizeof(IMAGE_INFO) * imageNum
        let crc_start = 6; // After magic(4) + crc(2)
        let crc_end = header_size + self.bins.len() * self.header.bin_info_size();

        if self.data.len() < crc_end {
            return Err(Error::InvalidFwpkg(
                "File too small for CRC verification".into(),
            ));
        }

        let crc_data = &self.data[crc_start..crc_end];
        let calculated_crc = crc16_xmodem(crc_data);

        if calculated_crc != self.header.crc {
            return Err(Error::CrcMismatch {
                expected: self.header.crc,
                actual: calculated_crc,
            });
        }

        debug!("FWPKG CRC verified: {:#06X}", self.header.crc);
        Ok(())
    }

    /// Get the total number of partitions.
    pub fn partition_count(&self) -> usize {
        self.bins.len()
    }

    /// Find a partition by name.
    pub fn find_by_name(&self, name: &str) -> Option<&FwpkgBinInfo> {
        self.bins.iter().find(|b| b.name == name)
    }
}

impl std::fmt::Debug for Fwpkg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Fwpkg")
            .field("header", &self.header)
            .field("bins", &self.bins)
            .field("data_len", &self.data.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_partition_type_from_u32() {
        assert_eq!(PartitionType::from(0), PartitionType::Loader);
        assert_eq!(PartitionType::from(1), PartitionType::Normal);
        assert_eq!(PartitionType::from(2), PartitionType::KvNv);
        assert_eq!(PartitionType::from(5), PartitionType::Flashboot);
        assert_eq!(PartitionType::from(16), PartitionType::Database);
        assert_eq!(PartitionType::from(99), PartitionType::Unknown(99));
    }

    #[test]
    fn test_partition_type_as_u32() {
        assert_eq!(PartitionType::Loader.as_u32(), 0);
        assert_eq!(PartitionType::Normal.as_u32(), 1);
        assert_eq!(PartitionType::Unknown(42).as_u32(), 42);
    }

    #[test]
    fn test_magic_constants() {
        assert_eq!(FWPKG_MAGIC_V1, 0xEFBEADDF);
        assert_eq!(FWPKG_MAGIC_V2_MIN, 0xEFBEADD0);
        assert_eq!(FWPKG_MAGIC_V2_MAX, 0xEFBEADDE);
        // V1 magic should be just above V2 range
        const { assert!(FWPKG_MAGIC_V1 > FWPKG_MAGIC_V2_MAX) };
    }

    #[test]
    fn test_header_sizes() {
        assert_eq!(HEADER_SIZE_V1, 12);
        assert_eq!(HEADER_SIZE_V2, 272); // 12 + 260
        assert_eq!(BIN_INFO_SIZE_V1, 52); // 32 + 4*5 = 52 (no padding)
        assert_eq!(BIN_INFO_SIZE_V2, 284); // 260 + 20 + 4 padding
    }

    #[test]
    fn test_fwpkg_version_header_size() {
        let v1_header = FwpkgHeader {
            magic: FWPKG_MAGIC_V1,
            crc: 0,
            cnt: 0,
            len: 0,
            name: String::new(),
            version: FwpkgVersion::V1,
        };
        assert_eq!(v1_header.header_size(), HEADER_SIZE_V1);
        assert_eq!(v1_header.bin_info_size(), BIN_INFO_SIZE_V1);

        let v2_header = FwpkgHeader {
            magic: FWPKG_MAGIC_V2_MIN,
            crc: 0,
            cnt: 0,
            len: 0,
            name: "test".to_string(),
            version: FwpkgVersion::V2,
        };
        assert_eq!(v2_header.header_size(), HEADER_SIZE_V2);
        assert_eq!(v2_header.bin_info_size(), BIN_INFO_SIZE_V2);
    }
}
