//! FWPKG firmware package format.
//!
//! FWPKG is HiSilicon's firmware package format, containing multiple
//! binary images and their metadata.
//!
//! ## Format Overview
//!
//! ```text
//! +------------------+
//! |   Header (12B)   |
//! +------------------+
//! |  BinInfo[0] 56B  |
//! +------------------+
//! |  BinInfo[1] 56B  |
//! +------------------+
//! |       ...        |
//! +------------------+
//! |  BinInfo[n] 56B  |
//! +------------------+
//! |   Binary Data    |
//! |       ...        |
//! +------------------+
//! ```

use crate::error::{Error, Result};
use crate::protocol::crc::crc16_xmodem;
use byteorder::{LittleEndian, ReadBytesExt};
use log::debug;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

/// FWPKG magic number (little-endian).
/// Stored as 0xDFADBEEF, reads as 0xEFBEADDF.
pub const FWPKG_MAGIC: u32 = 0xEFBEADDF;

/// Maximum number of partitions in a FWPKG.
pub const MAX_PARTITIONS: usize = 16;

/// Header size in bytes.
pub const HEADER_SIZE: usize = 12;

/// BinInfo size in bytes.
pub const BIN_INFO_SIZE: usize = 56;

/// FWPKG file header (12 bytes).
#[derive(Debug, Clone, Copy)]
pub struct FwpkgHeader {
    /// Magic number (0xEFBEADDF).
    pub magic: u32,
    /// CRC16-XMODEM checksum (starting from cnt field).
    pub crc: u16,
    /// Number of partitions.
    pub cnt: u16,
    /// Total firmware size.
    pub len: u32,
}

impl FwpkgHeader {
    /// Read header from a reader.
    pub fn read_from<R: Read>(reader: &mut R) -> Result<Self> {
        let magic = reader.read_u32::<LittleEndian>()?;
        let crc = reader.read_u16::<LittleEndian>()?;
        let cnt = reader.read_u16::<LittleEndian>()?;
        let len = reader.read_u32::<LittleEndian>()?;

        Ok(Self {
            magic,
            crc,
            cnt,
            len,
        })
    }

    /// Check if the magic number is valid.
    pub fn is_valid(&self) -> bool {
        self.magic == FWPKG_MAGIC && (self.cnt as usize) <= MAX_PARTITIONS
    }
}

/// Partition type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartitionType {
    /// LoaderBoot (first-stage bootloader).
    LoaderBoot,
    /// Normal firmware partition.
    Normal,
    /// Unknown partition type.
    Unknown(u32),
}

impl From<u32> for PartitionType {
    fn from(value: u32) -> Self {
        match value {
            0 => Self::LoaderBoot,
            1 => Self::Normal,
            v => Self::Unknown(v),
        }
    }
}

/// FWPKG partition information (56 bytes).
#[derive(Debug, Clone)]
pub struct FwpkgBinInfo {
    /// Partition name (max 31 chars + NUL).
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
    /// Read BinInfo from a reader.
    pub fn read_from<R: Read>(reader: &mut R) -> Result<Self> {
        let mut name_bytes = [0u8; 32];
        reader.read_exact(&mut name_bytes)?;

        // Find NUL terminator
        let name_end = name_bytes.iter().position(|&c| c == 0).unwrap_or(32);
        let name = String::from_utf8_lossy(&name_bytes[..name_end]).to_string();

        let offset = reader.read_u32::<LittleEndian>()?;
        let length = reader.read_u32::<LittleEndian>()?;
        let burn_addr = reader.read_u32::<LittleEndian>()?;
        let burn_size = reader.read_u32::<LittleEndian>()?;
        let type_value = reader.read_u32::<LittleEndian>()?;

        // Skip remaining bytes (56 - 32 - 20 = 4 bytes padding if any)
        // Actually 32 + 4*5 = 52, so 4 bytes remain
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

    /// Check if this is the LoaderBoot partition.
    pub fn is_loaderboot(&self) -> bool {
        self.partition_type == PartitionType::LoaderBoot
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
        if data.len() < HEADER_SIZE {
            return Err(Error::InvalidFwpkg("File too small for header".into()));
        }

        let mut cursor = std::io::Cursor::new(&data);

        // Read header
        let header = FwpkgHeader::read_from(&mut cursor)?;

        if !header.is_valid() {
            return Err(Error::InvalidFwpkg(format!(
                "Invalid magic: expected {:#010X}, got {:#010X}",
                FWPKG_MAGIC, header.magic
            )));
        }

        debug!(
            "FWPKG header: {} partitions, {} bytes total",
            header.cnt, header.len
        );

        // Read partition info
        let bin_count = header.cnt as usize;
        let expected_size = HEADER_SIZE + bin_count * BIN_INFO_SIZE;

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
            let bin_info = FwpkgBinInfo::read_from(&mut cursor)?;
            debug!(
                "  [{}] {} @ 0x{:08X}, {} bytes -> 0x{:08X}",
                i, bin_info.name, bin_info.offset, bin_info.length, bin_info.burn_addr
            );
            bins.push(bin_info);
        }

        Ok(Self { header, bins, data })
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
    pub fn verify_crc(&self) -> Result<()> {
        // CRC is calculated from cnt field onwards
        if self.data.len() < HEADER_SIZE {
            return Err(Error::InvalidFwpkg("File too small".into()));
        }

        // CRC covers: cnt(2) + len(4) + all BinInfo + binary data
        let crc_start = 6; // After magic(4) + crc(2)
        let crc_data = &self.data[crc_start..];

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
        assert_eq!(PartitionType::from(0), PartitionType::LoaderBoot);
        assert_eq!(PartitionType::from(1), PartitionType::Normal);
        assert_eq!(PartitionType::from(99), PartitionType::Unknown(99));
    }
}
