//! Interactive serial port selection.
//!
//! This module provides interactive serial port selection similar to espflash,
//! with support for:
//! - Auto-detection of known USB devices
//! - Interactive selection via dialoguer
//! - Remembering selected ports in configuration
//! - Non-interactive mode for CI/CD

use {
    crate::{CliError, config::Config},
    anyhow::Result,
    console::style,
    dialoguer::{Confirm, Error as DialoguerError, Select, theme::ColorfulTheme},
    hisiflash::{DetectedPort, TransportKind, UsbDevice, discover_ports},
    log::{debug, error, info},
    rust_i18n::t,
    std::{cmp::Ordering, io::IsTerminal},
};

/// Options for serial port selection.
#[derive(Debug, Clone, Default)]
pub struct SerialOptions {
    /// Explicit port specified via CLI.
    pub port: Option<String>,
    /// List all ports (including unknown types).
    pub list_all_ports: bool,
    /// Non-interactive mode (fail if multiple ports).
    pub non_interactive: bool,
    /// Force confirmation even for single recognized port.
    pub confirm_port: bool,
}

/// Result of port selection including whether it was a known device.
pub struct SelectedPort {
    /// The selected port info.
    pub port: DetectedPort,
    /// Whether this port matched a known/configured device.
    pub is_known: bool,
}

fn usage_err(message: &str) -> anyhow::Error {
    // Keep serial-selection user-facing failures in Usage class so they map
    // to CLI exit code 2 (instead of generic runtime code 1).
    // This is important for CI/script callers that branch on usage errors.
    CliError::Usage(message.to_string()).into()
}

fn select_non_interactive_port(
    selection_ports: Vec<DetectedPort>,
    config: &Config,
) -> Result<SelectedPort> {
    // Non-interactive mode must be deterministic and never prompt.
    // 0 or >1 candidates are treated as usage/setup issues (exit 2),
    // while exactly one candidate is a valid auto-selection.
    match selection_ports
        .len()
        .cmp(&1)
    {
        Ordering::Equal => {
            let port = selection_ports
                .into_iter()
                .next()
                .expect("selection_ports has exactly 1 element here");
            Ok(SelectedPort {
                is_known: is_known_device(&port, config),
                port,
            })
        },
        Ordering::Greater => Err(usage_err(t!("serial.multiple_ports").as_ref())),
        Ordering::Less => Err(usage_err(t!("serial.no_ports_available").as_ref())),
    }
}

/// Select a serial port interactively or automatically.
pub fn select_serial_port(options: &SerialOptions, config: &Config) -> Result<SelectedPort> {
    // If port explicitly specified, use it
    if let Some(port_name) = &options.port {
        return Ok(find_port_by_name(port_name));
    }

    // If port in config, use it
    if let Some(port_name) = &config
        .port
        .connection
        .serial
    {
        debug!("Using port from config: {port_name}");
        return Ok(find_port_by_name(port_name));
    }

    // Detect available ports
    let ports = discover_ports();

    if ports.is_empty() {
        // No ports is treated as usage/setup error for CLI contract consistency.
        return Err(usage_err(t!("serial.no_ports_found").as_ref()));
    }

    // Filter to known devices (built-in + config)
    let known_ports: Vec<DetectedPort> = ports
        .iter()
        .filter(|p| is_known_device(p, config))
        .cloned()
        .collect();

    // Select candidate set: known first unless user asks for all
    let selection_ports: Vec<DetectedPort> = if options.list_all_ports || known_ports.is_empty() {
        ports
    } else {
        known_ports
    };

    // Non-interactive mode must never prompt
    if options.non_interactive {
        return select_non_interactive_port(selection_ports, config);
    }

    match selection_ports
        .len()
        .cmp(&1)
    {
        Ordering::Greater => {
            ensure_interactive_terminal()?;
            select_port_interactive(selection_ports, config)
        },
        Ordering::Equal => {
            let port = selection_ports
                .into_iter()
                .next()
                .expect("selection_ports has exactly 1 element here");
            let is_known = is_known_device(&port, config);

            if is_known && !options.confirm_port {
                info!(
                    "Auto-selected port: {} [{}]",
                    port.name,
                    port.device
                        .name()
                );
                Ok(SelectedPort { port, is_known })
            } else {
                ensure_interactive_terminal()?;
                confirm_single_port(port, config)
            }
        },
        Ordering::Less => Err(usage_err(t!("serial.no_ports_available").as_ref())),
    }
}

fn ensure_interactive_terminal() -> Result<()> {
    if std::io::stdin().is_terminal() && std::io::stderr().is_terminal() {
        Ok(())
    } else {
        Err(CliError::Usage(t!("serial.interactive_requires_tty").to_string()).into())
    }
}

fn map_prompt_error(err: DialoguerError) -> anyhow::Error {
    match err {
        DialoguerError::IO(io_err) => {
            if io_err.kind() == std::io::ErrorKind::Interrupted {
                CliError::Cancelled(t!("serial.selection_cancelled").to_string()).into()
            } else {
                CliError::Usage(t!("serial.prompt_failed").to_string()).into()
            }
        },
    }
}

/// Find a port by name.
fn find_port_by_name(name: &str) -> SelectedPort {
    let ports = discover_ports();

    // Try exact match first
    if let Some(port) = ports
        .iter()
        .find(|p| p.name == name)
    {
        return SelectedPort {
            port: port.clone(),
            is_known: port
                .device
                .is_known(),
        };
    }

    // Try case-insensitive match (Windows)
    if let Some(port) = ports
        .iter()
        .find(|p| {
            p.name
                .eq_ignore_ascii_case(name)
        })
    {
        return SelectedPort {
            port: port.clone(),
            is_known: port
                .device
                .is_known(),
        };
    }

    // Port not found in detected list, but user explicitly specified it
    // Create a placeholder port info
    SelectedPort {
        port: DetectedPort {
            name: name.to_string(),
            transport: TransportKind::Serial,
            device: UsbDevice::Unknown,
            vid: None,
            pid: None,
            manufacturer: None,
            product: None,
            serial: None,
        },
        is_known: false,
    }
}

/// Check if a port matches a known device (from config or built-in list).
fn is_known_device(port: &DetectedPort, config: &Config) -> bool {
    // Check built-in device types
    if port
        .device
        .is_known()
    {
        return true;
    }

    // Check configured USB devices
    if let (Some(vid), Some(pid)) = (port.vid, port.pid) {
        for device in &config
            .port
            .usb_device
        {
            if device.matches(vid, pid) {
                return true;
            }
        }
    }

    false
}

/// Interactive port selection.
fn select_port_interactive(mut ports: Vec<DetectedPort>, config: &Config) -> Result<SelectedPort> {
    eprintln!(
        "{} {}",
        style("ℹ").blue(),
        t!("serial.detected_ports", count = ports.len())
    );
    eprintln!("{}", style(t!("serial.known_devices_hint")).dim());

    // Sort: known devices first
    ports.sort_by_key(|p| !is_known_device(p, config));

    // Build display names
    let port_names: Vec<String> = ports
        .iter()
        .map(|port| {
            let name = if is_known_device(port, config) {
                style(&port.name)
                    .bold()
                    .to_string()
            } else {
                port.name
                    .clone()
            };

            let device_info = if port
                .device
                .is_known()
            {
                format!(
                    " [{}]",
                    style(
                        port.device
                            .name()
                    )
                    .yellow()
                )
            } else if let (Some(vid), Some(pid)) = (port.vid, port.pid) {
                format!(" ({vid:04X}:{pid:04X})")
            } else {
                String::new()
            };

            let product = port
                .product
                .as_ref()
                .map(|p| format!(" - {}", style(p).dim()))
                .unwrap_or_default();

            format!("{name}{device_info}{product}")
        })
        .collect();

    // Truncate labels to fit terminal width to prevent wrapping in narrow
    // terminals.
    let term_width = console::Term::stderr()
        .size()
        .1 as usize;
    let max_item_width = term_width.saturating_sub(4);
    let port_names: Vec<String> = port_names
        .into_iter()
        .map(|n| console::truncate_str(&n, max_item_width, "\u{2026}").into_owned())
        .collect();

    let selection = Select::with_theme(&ColorfulTheme::default())
        .with_prompt(t!("serial.select_prompt").to_string())
        .items(&port_names)
        .default(0)
        .interact_opt()
        .map_err(map_prompt_error)?;

    match selection {
        Some(index) => {
            let port = ports
                .into_iter()
                .nth(index)
                .ok_or_else(|| anyhow::anyhow!("Invalid port index: {index}"))?;
            let is_known = is_known_device(&port, config);
            Ok(SelectedPort { port, is_known })
        },
        None => Err(CliError::Cancelled(t!("serial.selection_cancelled").to_string()).into()),
    }
}

/// Confirm use of a single unrecognized port.
fn confirm_single_port(port: DetectedPort, _config: &Config) -> Result<SelectedPort> {
    let product_info = port
        .product
        .as_ref()
        .map(|p| format!(" - {p}"))
        .unwrap_or_default();

    let confirmed = Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt(
            t!(
                "serial.confirm_use",
                port = port
                    .name
                    .clone(),
                info = product_info
            )
            .to_string(),
        )
        .default(true)
        .interact_opt()
        .map_err(map_prompt_error)?
        .unwrap_or(false);

    if confirmed {
        Ok(SelectedPort {
            port,
            is_known: false,
        })
    } else {
        Err(CliError::Cancelled(t!("serial.selection_cancelled").to_string()).into())
    }
}

/// Ask user if they want to remember this port.
pub fn ask_remember_port(port: &DetectedPort, config: &mut Config) -> Result<()> {
    if let (Some(vid), Some(pid)) = (port.vid, port.pid) {
        // Check if already known
        for device in &config
            .port
            .usb_device
        {
            if device.matches(vid, pid) {
                return Ok(()); // Already saved
            }
        }

        let confirmed = Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt(t!("serial.remember_prompt").to_string())
            .default(false)
            .interact_opt()
            .map_err(map_prompt_error)?
            .unwrap_or(false);

        if confirmed {
            if let Err(e) = config.remember_usb_device(vid, pid) {
                error!("Failed to save port configuration: {e}");
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        console::{measure_text_width, style, truncate_str},
        hisiflash::{DetectedPort, TransportKind, UsbDevice},
    };

    fn strip_ansi_codes(s: &str) -> String {
        let mut out = String::new();
        let bytes = s.as_bytes();
        let mut i = 0usize;
        while i < bytes.len() {
            if bytes[i] == 0x1B && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
                i += 2;
                while i < bytes.len() && bytes[i] != b'm' {
                    i += 1;
                }
                if i < bytes.len() {
                    i += 1;
                }
            } else {
                out.push(bytes[i] as char);
                i += 1;
            }
        }
        out
    }

    // ---- SerialOptions ----

    #[test]
    fn test_serial_options_default() {
        let options = SerialOptions::default();
        assert!(
            options
                .port
                .is_none()
        );
        assert!(!options.list_all_ports);
        assert!(!options.non_interactive);
        assert!(!options.confirm_port);
    }

    #[test]
    fn test_truncate_port_label_right_preserves_left() {
        let port = "/dev/verylongttyusb0";
        let product = " - Very Long Product Name That Would Wrap";
        let name = format!("{port}{product}");
        let styled = style(&name)
            .bold()
            .to_string();

        let term_width = 30usize;
        let max_item_width = term_width.saturating_sub(4);
        let truncated = truncate_str(&styled, max_item_width, "…").into_owned();

        assert!(!truncated.contains('\n'));
        assert!(measure_text_width(&truncated) <= max_item_width);
        // Expect left side (port prefix) to be preserved in right-truncation.
        let stripped = strip_ansi_codes(&truncated);
        assert!(stripped.starts_with("/dev/verylong"));
    }

    #[test]
    fn test_truncate_port_label_handles_ansi() {
        let port = style("/dev/ttyUSB0").bold();
        let product = " - 产品信息";
        let name = format!("{port}{product}");
        let term_width = 10usize;
        let max_item_width = term_width.saturating_sub(4);
        let t = truncate_str(&name, max_item_width, "…").into_owned();
        assert!(!t.contains('\n'));
        assert!(measure_text_width(&t) <= max_item_width);
    }

    #[test]
    fn test_serial_options_with_port() {
        let options = SerialOptions {
            port: Some("/dev/ttyUSB0".to_string()),
            ..Default::default()
        };
        assert_eq!(
            options
                .port
                .as_deref(),
            Some("/dev/ttyUSB0")
        );
    }

    #[test]
    fn test_serial_options_clone() {
        let options = SerialOptions {
            port: Some("COM3".to_string()),
            list_all_ports: true,
            non_interactive: true,
            confirm_port: false,
        };
        let cloned = options.clone();
        assert_eq!(cloned.port, options.port);
        assert_eq!(cloned.list_all_ports, options.list_all_ports);
        assert_eq!(cloned.non_interactive, options.non_interactive);
    }

    // ---- is_known_device ----

    #[test]
    fn test_is_known_device_builtin() {
        let port = DetectedPort {
            name: "/dev/ttyUSB0".to_string(),
            transport: TransportKind::Serial,
            device: UsbDevice::Ch340,
            vid: Some(0x1A86),
            pid: Some(0x7523),
            manufacturer: None,
            product: None,
            serial: None,
        };
        let config = Config::default();
        assert!(is_known_device(&port, &config));
    }

    #[test]
    fn test_is_known_device_unknown() {
        let port = DetectedPort {
            name: "/dev/ttyUSB0".to_string(),
            transport: TransportKind::Serial,
            device: UsbDevice::Unknown,
            vid: Some(0x9999),
            pid: Some(0x9999),
            manufacturer: None,
            product: None,
            serial: None,
        };
        let config = Config::default();
        assert!(!is_known_device(&port, &config));
    }

    #[test]
    fn test_is_known_device_from_config() {
        let port = DetectedPort {
            name: "/dev/ttyUSB0".to_string(),
            transport: TransportKind::Serial,
            device: UsbDevice::Unknown,
            vid: Some(0xABCD),
            pid: Some(0x1234),
            manufacturer: None,
            product: None,
            serial: None,
        };
        let mut config = Config::default();
        config
            .port
            .usb_device
            .push(crate::config::UsbDevice {
                vid: 0xABCD,
                pid: 0x1234,
            });
        assert!(is_known_device(&port, &config));
    }

    #[test]
    fn test_is_known_device_no_vid_pid() {
        let port = DetectedPort {
            name: "/dev/ttyS0".to_string(),
            transport: TransportKind::Serial,
            device: UsbDevice::Unknown,
            vid: None,
            pid: None,
            manufacturer: None,
            product: None,
            serial: None,
        };
        let config = Config::default();
        assert!(!is_known_device(&port, &config));
    }

    // ---- SelectedPort ----

    #[test]
    fn test_selected_port_fields() {
        let sp = SelectedPort {
            port: DetectedPort {
                name: "COM1".to_string(),
                transport: TransportKind::Serial,
                device: UsbDevice::Cp210x,
                vid: Some(0x10C4),
                pid: Some(0xEA60),
                manufacturer: Some("Silicon Labs".to_string()),
                product: Some("CP2102".to_string()),
                serial: None,
            },
            is_known: true,
        };
        assert_eq!(
            sp.port
                .name,
            "COM1"
        );
        assert!(sp.is_known);
        assert!(
            sp.port
                .device
                .is_known()
        );
    }

    // ---- non-interactive error mapping regression ----

    #[test]
    fn test_select_non_interactive_multiple_ports_returns_usage_error() {
        let ports = vec![
            DetectedPort {
                name: "/dev/ttyUSB0".to_string(),
                transport: TransportKind::Serial,
                device: UsbDevice::Unknown,
                vid: None,
                pid: None,
                manufacturer: None,
                product: None,
                serial: None,
            },
            DetectedPort {
                name: "/dev/ttyUSB1".to_string(),
                transport: TransportKind::Serial,
                device: UsbDevice::Unknown,
                vid: None,
                pid: None,
                manufacturer: None,
                product: None,
                serial: None,
            },
        ];

        let result = select_non_interactive_port(ports, &Config::default());
        assert!(result.is_err());
        let err = result
            .err()
            .expect("expected error");
        assert!(
            err.downcast_ref::<CliError>()
                .is_some()
        );
        if let Some(cli_err) = err.downcast_ref::<CliError>() {
            assert!(matches!(cli_err, CliError::Usage(_)));
        }
    }

    #[test]
    fn test_select_non_interactive_no_ports_returns_usage_error() {
        let result = select_non_interactive_port(vec![], &Config::default());
        assert!(result.is_err());
        let err = result
            .err()
            .expect("expected error");
        assert!(
            err.downcast_ref::<CliError>()
                .is_some()
        );
        if let Some(cli_err) = err.downcast_ref::<CliError>() {
            assert!(matches!(cli_err, CliError::Usage(_)));
        }
    }

    #[test]
    fn test_select_non_interactive_single_port_returns_selected_port() {
        let ports = vec![DetectedPort {
            name: "/dev/ttyUSB0".to_string(),
            transport: TransportKind::Serial,
            device: UsbDevice::Unknown,
            vid: None,
            pid: None,
            manufacturer: None,
            product: None,
            serial: None,
        }];

        let selected = select_non_interactive_port(ports, &Config::default()).unwrap();
        assert_eq!(
            selected
                .port
                .name,
            "/dev/ttyUSB0"
        );
    }
}
