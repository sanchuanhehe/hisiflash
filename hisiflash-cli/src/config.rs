//! Configuration file support for hisiflash.
//!
//! Configuration is loaded from multiple sources with the following priority (highest first):
//! 1. Command-line arguments
//! 2. Environment variables (HISIFLASH_*)
//! 3. Local config file (./hisiflash.toml or ./hisiflash_ports.toml)
//! 4. Global config file (~/.config/hisiflash/config.toml)

use directories::ProjectDirs;
use log::{debug, info, warn};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// USB device identification for port matching.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UsbDevice {
    /// USB Vendor ID.
    pub vid: u16,
    /// USB Product ID.
    pub pid: u16,
}

impl UsbDevice {
    /// Check if this device matches the given USB info.
    pub fn matches(&self, vid: u16, pid: u16) -> bool {
        self.vid == vid && self.pid == pid
    }
}

/// Connection configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConnectionConfig {
    /// Preferred serial port (e.g., "/dev/ttyUSB0" or "COM3").
    pub serial: Option<String>,
    /// Default baud rate.
    pub baud: Option<u32>,
}

/// Port-specific configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PortConfig {
    /// Connection settings.
    #[serde(default)]
    pub connection: ConnectionConfig,
    /// Known USB devices for auto-detection.
    #[serde(default)]
    pub usb_device: Vec<UsbDevice>,
}

/// Flash configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FlashConfig {
    /// Default chip type.
    pub chip: Option<String>,
    /// Skip verification by default.
    #[serde(default)]
    pub skip_verify: bool,
    /// Use late baud rate change.
    #[serde(default)]
    pub late_baud: bool,
}

/// Main configuration structure.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    /// Port configuration.
    #[serde(default)]
    pub port: PortConfig,
    /// Flash configuration.
    #[serde(default)]
    pub flash: FlashConfig,
}

impl Config {
    /// Load configuration from all available sources.
    pub fn load() -> Self {
        let mut config = Self::default();

        // Load global config
        if let Some(global_path) = Self::global_config_path() {
            if global_path.exists() {
                if let Some(global_config) = Self::load_from_file(&global_path) {
                    debug!("Loaded global config from {}", global_path.display());
                    config.merge(global_config);
                }
            }
        }

        // Load local config (overrides global)
        if let Some(local_config) = Self::load_from_file(Path::new("hisiflash.toml")) {
            debug!("Loaded local config from hisiflash.toml");
            config.merge(local_config);
        }

        // Load ports config
        if let Some(ports_config) = Self::load_ports_config() {
            config.port = ports_config;
        }

        config
    }

    /// Load configuration from a specific file path (--config flag).
    pub fn load_from_path(path: &Path) -> Self {
        if let Some(config) = Self::load_from_file(path) {
            debug!("Loaded config from {}", path.display());
            config
        } else {
            warn!(
                "Could not load config from {}, using defaults",
                path.display()
            );
            Self::default()
        }
    }

    /// Load configuration from a specific file.
    fn load_from_file(path: &Path) -> Option<Self> {
        if !path.exists() {
            return None;
        }

        match fs::read_to_string(path) {
            Ok(content) => match toml::from_str(&content) {
                Ok(config) => Some(config),
                Err(e) => {
                    warn!("Failed to parse config file {}: {}", path.display(), e);
                    None
                },
            },
            Err(e) => {
                warn!("Failed to read config file {}: {}", path.display(), e);
                None
            },
        }
    }

    /// Load ports configuration from hisiflash_ports.toml.
    fn load_ports_config() -> Option<PortConfig> {
        let local_path = Path::new("hisiflash_ports.toml");
        if local_path.exists() {
            if let Ok(content) = fs::read_to_string(local_path) {
                if let Ok(config) = toml::from_str(&content) {
                    debug!("Loaded ports config from hisiflash_ports.toml");
                    return Some(config);
                }
            }
        }

        // Try global ports config
        if let Some(global_dir) = Self::global_config_dir() {
            let global_path = global_dir.join("ports.toml");
            if global_path.exists() {
                if let Ok(content) = fs::read_to_string(&global_path) {
                    if let Ok(config) = toml::from_str(&content) {
                        debug!("Loaded ports config from {}", global_path.display());
                        return Some(config);
                    }
                }
            }
        }

        None
    }

    /// Get the global configuration directory.
    pub fn global_config_dir() -> Option<PathBuf> {
        ProjectDirs::from("", "", "hisiflash").map(|dirs| dirs.config_dir().to_path_buf())
    }

    /// Get the global configuration file path.
    pub fn global_config_path() -> Option<PathBuf> {
        Self::global_config_dir().map(|dir| dir.join("config.toml"))
    }

    /// Merge another config into this one.
    fn merge(&mut self, other: Self) {
        // Port config
        if other.port.connection.serial.is_some() {
            self.port.connection.serial = other.port.connection.serial;
        }
        if other.port.connection.baud.is_some() {
            self.port.connection.baud = other.port.connection.baud;
        }
        self.port.usb_device.extend(other.port.usb_device);

        // Flash config
        if other.flash.chip.is_some() {
            self.flash.chip = other.flash.chip;
        }
        if other.flash.skip_verify {
            self.flash.skip_verify = true;
        }
        if other.flash.late_baud {
            self.flash.late_baud = true;
        }
    }

    /// Save the port configuration (remembers serial port).
    #[allow(dead_code, clippy::unused_self)]
    pub fn save_port(
        &self,
        serial: &str,
        vid: Option<u16>,
        pid: Option<u16>,
    ) -> anyhow::Result<()> {
        let path = Path::new("hisiflash_ports.toml");

        let mut port_config = PortConfig::default();
        port_config.connection.serial = Some(serial.to_string());

        if let (Some(vid), Some(pid)) = (vid, pid) {
            port_config.usb_device.push(UsbDevice { vid, pid });
        }

        let content = toml::to_string_pretty(&port_config)?;
        fs::write(path, content)?;
        info!("Saved port configuration to {}", path.display());

        Ok(())
    }

    /// Save USB device for future auto-detection.
    pub fn remember_usb_device(&mut self, vid: u16, pid: u16) -> anyhow::Result<()> {
        let device = UsbDevice { vid, pid };

        // Don't add duplicates
        if self.port.usb_device.contains(&device) {
            return Ok(());
        }

        // Try to save to local file first, fall back to global
        let path =
            if Path::new("hisiflash_ports.toml").exists() || Path::new("hisiflash.toml").exists() {
                PathBuf::from("hisiflash_ports.toml")
            } else if let Some(global_dir) = Self::global_config_dir() {
                fs::create_dir_all(&global_dir)?;
                global_dir.join("ports.toml")
            } else {
                PathBuf::from("hisiflash_ports.toml")
            };

        self.port.usb_device.push(device);

        let content = toml::to_string_pretty(&self.port)?;
        fs::write(&path, content)?;
        info!("Saved USB device to {}", path.display());

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Default values ----

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert!(config.port.connection.serial.is_none());
        assert!(config.port.connection.baud.is_none());
        assert!(config.port.usb_device.is_empty());
        assert!(config.flash.chip.is_none());
        assert!(!config.flash.skip_verify);
        assert!(!config.flash.late_baud);
    }

    #[test]
    fn test_default_connection_config() {
        let conn = ConnectionConfig::default();
        assert!(conn.serial.is_none());
        assert!(conn.baud.is_none());
    }

    #[test]
    fn test_default_port_config() {
        let port = PortConfig::default();
        assert!(port.connection.serial.is_none());
        assert!(port.usb_device.is_empty());
    }

    #[test]
    fn test_default_flash_config() {
        let flash = FlashConfig::default();
        assert!(flash.chip.is_none());
        assert!(!flash.skip_verify);
        assert!(!flash.late_baud);
    }

    // ---- UsbDevice ----

    #[test]
    fn test_usb_device_matches() {
        let device = UsbDevice {
            vid: 0x1A86,
            pid: 0x7523,
        };
        assert!(device.matches(0x1A86, 0x7523));
        assert!(!device.matches(0x1A86, 0x7522));
        assert!(!device.matches(0x10C4, 0x7523));
    }

    #[test]
    fn test_usb_device_eq() {
        let a = UsbDevice { vid: 0x1A86, pid: 0x7523 };
        let b = UsbDevice { vid: 0x1A86, pid: 0x7523 };
        let c = UsbDevice { vid: 0x10C4, pid: 0xEA60 };
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn test_usb_device_clone() {
        let a = UsbDevice { vid: 0x1A86, pid: 0x7523 };
        let b = a.clone();
        assert_eq!(a, b);
    }

    // ---- Config merge ----

    #[test]
    fn test_config_merge_serial() {
        let mut base = Config::default();
        let mut other = Config::default();
        other.port.connection.serial = Some("/dev/ttyUSB0".to_string());
        other.flash.chip = Some("ws63".to_string());

        base.merge(other);

        assert_eq!(base.port.connection.serial.as_deref(), Some("/dev/ttyUSB0"));
        assert_eq!(base.flash.chip.as_deref(), Some("ws63"));
    }

    #[test]
    fn test_config_merge_baud() {
        let mut base = Config::default();
        base.port.connection.baud = Some(115200);

        let mut other = Config::default();
        other.port.connection.baud = Some(921600);

        base.merge(other);
        assert_eq!(base.port.connection.baud, Some(921600));
    }

    #[test]
    fn test_config_merge_does_not_overwrite_with_none() {
        let mut base = Config::default();
        base.port.connection.serial = Some("/dev/ttyUSB0".to_string());
        base.port.connection.baud = Some(115200);

        let other = Config::default(); // all None
        base.merge(other);

        assert_eq!(base.port.connection.serial.as_deref(), Some("/dev/ttyUSB0"));
        assert_eq!(base.port.connection.baud, Some(115200));
    }

    #[test]
    fn test_config_merge_usb_devices_extend() {
        let mut base = Config::default();
        base.port.usb_device.push(UsbDevice { vid: 0x1A86, pid: 0x7523 });

        let mut other = Config::default();
        other.port.usb_device.push(UsbDevice { vid: 0x10C4, pid: 0xEA60 });

        base.merge(other);
        assert_eq!(base.port.usb_device.len(), 2);
    }

    #[test]
    fn test_config_merge_skip_verify() {
        let mut base = Config::default();
        let mut other = Config::default();
        other.flash.skip_verify = true;
        base.merge(other);
        assert!(base.flash.skip_verify);
    }

    #[test]
    fn test_config_merge_late_baud() {
        let mut base = Config::default();
        let mut other = Config::default();
        other.flash.late_baud = true;
        base.merge(other);
        assert!(base.flash.late_baud);
    }

    // ---- TOML serialization/deserialization ----

    #[test]
    fn test_config_from_toml() {
        let toml_str = r#"
[port.connection]
serial = "/dev/ttyUSB0"
baud = 921600

[[port.usb_device]]
vid = 6790
pid = 29987

[flash]
chip = "ws63"
skip_verify = true
late_baud = false
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.port.connection.serial.as_deref(), Some("/dev/ttyUSB0"));
        assert_eq!(config.port.connection.baud, Some(921600));
        assert_eq!(config.port.usb_device.len(), 1);
        assert_eq!(config.port.usb_device[0].vid, 6790);
        assert_eq!(config.port.usb_device[0].pid, 29987);
        assert_eq!(config.flash.chip.as_deref(), Some("ws63"));
        assert!(config.flash.skip_verify);
        assert!(!config.flash.late_baud);
    }

    #[test]
    fn test_config_from_empty_toml() {
        let config: Config = toml::from_str("").unwrap();
        assert!(config.port.connection.serial.is_none());
        assert!(config.port.usb_device.is_empty());
        assert!(config.flash.chip.is_none());
    }

    #[test]
    fn test_config_from_partial_toml() {
        let toml_str = r#"
[flash]
chip = "bs2x"
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert!(config.port.connection.serial.is_none());
        assert_eq!(config.flash.chip.as_deref(), Some("bs2x"));
    }

    #[test]
    fn test_config_roundtrip_toml() {
        let mut config = Config::default();
        config.port.connection.serial = Some("COM3".to_string());
        config.port.connection.baud = Some(460800);
        config.flash.chip = Some("ws63".to_string());
        config.port.usb_device.push(UsbDevice { vid: 0x1A86, pid: 0x7523 });

        let serialized = toml::to_string_pretty(&config).unwrap();
        let deserialized: Config = toml::from_str(&serialized).unwrap();

        assert_eq!(deserialized.port.connection.serial.as_deref(), Some("COM3"));
        assert_eq!(deserialized.port.connection.baud, Some(460800));
        assert_eq!(deserialized.flash.chip.as_deref(), Some("ws63"));
        assert_eq!(deserialized.port.usb_device.len(), 1);
        assert_eq!(deserialized.port.usb_device[0].vid, 0x1A86);
    }

    #[test]
    fn test_port_config_toml_roundtrip() {
        let mut port = PortConfig::default();
        port.connection.serial = Some("/dev/ttyACM0".to_string());
        port.usb_device.push(UsbDevice { vid: 0x10C4, pid: 0xEA60 });
        port.usb_device.push(UsbDevice { vid: 0x0403, pid: 0x6001 });

        let serialized = toml::to_string_pretty(&port).unwrap();
        let deserialized: PortConfig = toml::from_str(&serialized).unwrap();

        assert_eq!(deserialized.connection.serial.as_deref(), Some("/dev/ttyACM0"));
        assert_eq!(deserialized.usb_device.len(), 2);
    }

    // ---- load_from_path with tempfile ----

    #[test]
    fn test_load_from_path_valid() {
        let dir = std::env::temp_dir().join("hisiflash_test_config");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("test_config.toml");
        fs::write(&path, r#"
[port.connection]
serial = "/dev/ttyUSB1"
[flash]
chip = "bs2x"
"#).unwrap();

        let config = Config::load_from_path(&path);
        assert_eq!(config.port.connection.serial.as_deref(), Some("/dev/ttyUSB1"));
        assert_eq!(config.flash.chip.as_deref(), Some("bs2x"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_load_from_path_nonexistent() {
        let config = Config::load_from_path(Path::new("/nonexistent/path/config.toml"));
        // Should return default
        assert!(config.port.connection.serial.is_none());
    }

    // ---- global_config_path ----

    #[test]
    fn test_global_config_path_is_some() {
        // On most systems this should return Some
        let path = Config::global_config_path();
        if let Some(p) = path {
            assert!(p.to_str().unwrap().contains("hisiflash"));
            assert!(p.to_str().unwrap().ends_with("config.toml"));
        }
    }

    #[test]
    fn test_global_config_dir_is_some() {
        let dir = Config::global_config_dir();
        if let Some(d) = dir {
            assert!(d.to_str().unwrap().contains("hisiflash"));
        }
    }
}
