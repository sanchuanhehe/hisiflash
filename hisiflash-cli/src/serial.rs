//! Interactive serial port selection.
//!
//! This module provides interactive serial port selection similar to espflash,
//! with support for:
//! - Auto-detection of known USB devices
//! - Interactive selection via dialoguer
//! - Remembering selected ports in configuration
//! - Non-interactive mode for CI/CD

use std::cmp::Ordering;

use crate::config::Config;
use anyhow::{Context, Result};
use console::style;
use dialoguer::{theme::ColorfulTheme, Confirm, Select};
use hisiflash::connection::detect::{self, DetectedPort, UsbDevice};
use log::{debug, error, info};

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

/// Select a serial port interactively or automatically.
pub fn select_serial_port(options: &SerialOptions, config: &Config) -> Result<SelectedPort> {
    // If port explicitly specified, use it
    if let Some(port_name) = &options.port {
        return Ok(find_port_by_name(port_name));
    }

    // If port in config, use it
    if let Some(port_name) = &config.port.connection.serial {
        debug!("Using port from config: {port_name}");
        return Ok(find_port_by_name(port_name));
    }

    // Detect available ports
    let ports = detect::detect_ports();
    
    if ports.is_empty() {
        anyhow::bail!("No serial ports found. Please connect a device or specify a port with -p.");
    }

    // Filter to known devices
    let known_ports: Vec<_> = ports
        .iter()
        .filter(|p| is_known_device(p, config))
        .collect();

    // If exactly one known port and not forcing confirmation, use it
    if known_ports.len() == 1 && !options.confirm_port {
        let port = known_ports[0].clone();
        info!(
            "Auto-selected port: {} [{}]",
            port.name,
            port.device.name()
        );
        return Ok(SelectedPort {
            port,
            is_known: true,
        });
    }

    // In non-interactive mode, fail if multiple ports
    if options.non_interactive && ports.len() > 1 {
        anyhow::bail!(
            "Multiple serial ports found. Use -p to specify a port or disable --non-interactive."
        );
    }

    // Interactive selection
    // Use all ports if list_all_ports is set, otherwise prefer known ports
    let selection_ports = if options.list_all_ports || known_ports.is_empty() {
        ports
    } else {
        known_ports.into_iter().cloned().collect()
    };

    match selection_ports.len().cmp(&1) {
        Ordering::Greater => select_port_interactive(selection_ports, config),
        Ordering::Equal => {
            // Single port - ask for confirmation if unknown
            let port = selection_ports.into_iter().next().unwrap();
            if options.non_interactive || port.device.is_known() {
                Ok(SelectedPort {
                    is_known: port.device.is_known(),
                    port,
                })
            } else {
                confirm_single_port(port, config)
            }
        }
        Ordering::Less => anyhow::bail!("No serial ports available."),
    }
}

/// Find a port by name.
fn find_port_by_name(name: &str) -> SelectedPort {
    let ports = detect::detect_ports();
    
    // Try exact match first
    if let Some(port) = ports.iter().find(|p| p.name == name) {
        return SelectedPort {
            port: port.clone(),
            is_known: port.device.is_known(),
        };
    }

    // Try case-insensitive match (Windows)
    if let Some(port) = ports.iter().find(|p| p.name.eq_ignore_ascii_case(name)) {
        return SelectedPort {
            port: port.clone(),
            is_known: port.device.is_known(),
        };
    }

    // Port not found in detected list, but user explicitly specified it
    // Create a placeholder port info
    SelectedPort {
        port: DetectedPort {
            name: name.to_string(),
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
    if port.device.is_known() {
        return true;
    }

    // Check configured USB devices
    if let (Some(vid), Some(pid)) = (port.vid, port.pid) {
        for device in &config.port.usb_device {
            if device.matches(vid, pid) {
                return true;
            }
        }
    }

    false
}

/// Interactive port selection.
fn select_port_interactive(mut ports: Vec<DetectedPort>, config: &Config) -> Result<SelectedPort> {
    println!(
        "{} Detected {} serial port(s)",
        style("ℹ").blue(),
        ports.len()
    );
    println!(
        "{}",
        style("  Known devices are highlighted").dim()
    );

    // Sort: known devices first
    ports.sort_by_key(|p| !is_known_device(p, config));

    // Build display names
    let port_names: Vec<String> = ports
        .iter()
        .map(|port| {
            let name = if is_known_device(port, config) {
                style(&port.name).bold().to_string()
            } else {
                port.name.clone()
            };

            let device_info = if port.device.is_known() {
                format!(" [{}]", style(port.device.name()).yellow())
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

    // Setup Ctrl-C handler to restore cursor
    #[allow(clippy::unwrap_used)] // ctrlc handler setup
    ctrlc::set_handler(move || {
        let term = dialoguer::console::Term::stdout();
        let _ = term.show_cursor();
        std::process::exit(130);
    })
    .ok(); // Ignore error if handler already set

    let selection = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("Select a serial port")
        .items(&port_names)
        .default(0)
        .interact_opt()
        .context("Failed to show port selection")?;

    match selection {
        Some(index) => {
            let port = ports.into_iter().nth(index).unwrap();
            let is_known = is_known_device(&port, config);
            Ok(SelectedPort { port, is_known })
        }
        None => anyhow::bail!("Port selection cancelled"),
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
        .with_prompt(format!("Use serial port '{}'{}?", port.name, product_info))
        .default(true)
        .interact_opt()
        .context("Failed to show confirmation")?
        .unwrap_or(false);

    if confirmed {
        Ok(SelectedPort {
            port,
            is_known: false,
        })
    } else {
        anyhow::bail!("Port selection cancelled")
    }
}

/// Ask user if they want to remember this port.
pub fn ask_remember_port(port: &DetectedPort, config: &mut Config) -> Result<()> {
    if let (Some(vid), Some(pid)) = (port.vid, port.pid) {
        // Check if already known
        for device in &config.port.usb_device {
            if device.matches(vid, pid) {
                return Ok(()); // Already saved
            }
        }

        let confirmed = Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt("Remember this serial port for future use?")
            .default(false)
            .interact_opt()
            .context("Failed to show confirmation")?
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
    use super::*;

    #[test]
    fn test_serial_options_default() {
        let options = SerialOptions::default();
        assert!(options.port.is_none());
        assert!(!options.list_all_ports);
        assert!(!options.non_interactive);
        assert!(!options.confirm_port);
    }
}
