//! Firmware info and port listing command implementations.

use anyhow::{Context, Result};
use console::style;
use hisiflash::{Fwpkg, FwpkgVersion, PartitionType};
use rust_i18n::t;
use std::path::PathBuf;

/// List ports command implementation.
pub(crate) fn cmd_list_ports(json: bool) {
    let detected = hisiflash::connection::detect::detect_ports();

    if json {
        let ports: Vec<serde_json::Value> = detected
            .iter()
            .map(|p| {
                serde_json::json!({
                    "name": p.name,
                    "device": p.device.name(),
                    "known": p.device.is_known(),
                    "vid": p.vid,
                    "pid": p.pid,
                    "manufacturer": p.manufacturer,
                    "product": p.product,
                    "serial": p.serial,
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&ports).unwrap_or_default()
        );
        return;
    }

    eprintln!("{}", style(t!("list_ports.header")).bold().underlined());

    if detected.is_empty() {
        eprintln!("  {}", style(t!("list_ports.no_ports")).dim());
    } else {
        for port in &detected {
            let device_type = if port.device.is_known() {
                format!(" [{}]", style(port.device.name()).yellow())
            } else {
                String::new()
            };

            let product = port.product.as_deref().unwrap_or("");
            let vid_pid = if let (Some(vid), Some(pid)) = (port.vid, port.pid) {
                format!(" ({vid:04X}:{pid:04X})")
            } else {
                String::new()
            };

            eprintln!(
                "  {} {}{}{}{}",
                style("•").green(),
                style(&port.name).cyan(),
                device_type,
                vid_pid,
                if !product.is_empty() {
                    format!(" - {}", style(product).dim())
                } else {
                    String::new()
                }
            );
        }

        // Show auto-detection result
        if let Ok(auto_port) = hisiflash::connection::detect::auto_detect_port() {
            eprintln!(
                "\n{} {}",
                style("→").green().bold(),
                t!(
                    "list_ports.auto_detected",
                    port = style(&auto_port.name).cyan().bold().to_string()
                )
            );
        }
    }
}

/// Info command implementation.
pub(crate) fn cmd_info(firmware: &PathBuf, json: bool) -> Result<()> {
    if json {
        return cmd_info_json(firmware);
    }

    eprintln!(
        "{} {}",
        style("📦").cyan(),
        t!("flash.loading_firmware", path = firmware.display())
    );

    let fwpkg = Fwpkg::from_file(firmware)
        .with_context(|| t!("error.load_firmware", path = firmware.display().to_string()))?;

    eprintln!("\n{}", style(t!("info.header")).bold().underlined());

    // Show format version
    let version_str = match fwpkg.version() {
        FwpkgVersion::V1 => "V1 (32-byte names)",
        FwpkgVersion::V2 => "V2 (260-byte names)",
    };
    eprintln!("  {}: {}", t!("info.format"), version_str);

    // Show package name for V2
    if !fwpkg.package_name().is_empty() {
        eprintln!("  {}: {}", t!("info.package_name"), fwpkg.package_name());
    }

    eprintln!(
        "  {}",
        t!("info.partitions", count = fwpkg.partition_count())
    );
    eprintln!("  {}", t!("info.total_size", size = fwpkg.header.len));
    eprintln!(
        "  {}",
        t!("info.crc", crc = format!("{:04X}", fwpkg.header.crc))
    );

    // Verify CRC
    match fwpkg.verify_crc() {
        Ok(()) => eprintln!(
            "  {}",
            t!("info.crc_valid", status = t!("info.yes").to_string())
        ),
        Err(_) => eprintln!(
            "  {}",
            t!("info.crc_valid", status = t!("info.no").to_string())
        ),
    }

    eprintln!(
        "\n{}",
        style(t!("info.partitions_header")).bold().underlined()
    );
    for (i, bin) in fwpkg.bins.iter().enumerate() {
        let type_str = format_partition_type(bin.partition_type);

        eprintln!("\n  [{:2}] {}", i, style(&bin.name).cyan().bold());
        eprintln!("       {}", t!("info.type", "type" = type_str));
        eprintln!(
            "       {}",
            t!("info.offset", offset = format!("{:08X}", bin.offset))
        );
        eprintln!("       {}", t!("info.length", length = bin.length));
        eprintln!(
            "       {}",
            t!("info.burn_addr", addr = format!("{:08X}", bin.burn_addr))
        );
        eprintln!("       {}", t!("info.burn_size", size = bin.burn_size));
    }

    Ok(())
}

/// Info command `--json` output: structured JSON to stdout.
fn cmd_info_json(firmware: &PathBuf) -> Result<()> {
    let fwpkg = Fwpkg::from_file(firmware)
        .with_context(|| t!("error.load_firmware", path = firmware.display().to_string()))?;

    let version_str = match fwpkg.version() {
        FwpkgVersion::V1 => "V1",
        FwpkgVersion::V2 => "V2",
    };

    let crc_valid = fwpkg.verify_crc().is_ok();

    let partitions: Vec<serde_json::Value> = fwpkg
        .bins
        .iter()
        .map(|bin| {
            serde_json::json!({
                "name": bin.name,
                "type": partition_type_str(bin.partition_type),
                "offset": format!("0x{:08X}", bin.offset),
                "length": bin.length,
                "burn_addr": format!("0x{:08X}", bin.burn_addr),
                "burn_size": bin.burn_size,
                "is_loaderboot": bin.is_loaderboot(),
            })
        })
        .collect();

    let info = serde_json::json!({
        "format": version_str,
        "package_name": fwpkg.package_name(),
        "partition_count": fwpkg.partition_count(),
        "total_size": fwpkg.header.len,
        "crc": format!("0x{:04X}", fwpkg.header.crc),
        "crc_valid": crc_valid,
        "partitions": partitions,
    });

    println!(
        "{}",
        serde_json::to_string_pretty(&info).unwrap_or_default()
    );
    Ok(())
}

/// Format partition type as a plain string (no ANSI colors) for JSON output.
pub(crate) fn partition_type_str(pt: PartitionType) -> &'static str {
    match pt {
        PartitionType::Loader => "Loader",
        PartitionType::Normal => "Normal",
        PartitionType::KvNv => "KV-NV",
        PartitionType::Efuse => "eFuse",
        PartitionType::Otp => "OTP",
        PartitionType::Flashboot => "FlashBoot",
        PartitionType::Factory => "Factory",
        PartitionType::Version => "Version",
        PartitionType::SecurityA => "Security-A",
        PartitionType::SecurityB => "Security-B",
        PartitionType::SecurityC => "Security-C",
        PartitionType::ProtocolA => "Protocol-A",
        PartitionType::AppsA => "Apps-A",
        PartitionType::RadioConfig => "RadioConfig",
        PartitionType::Rom => "ROM",
        PartitionType::Emmc => "eMMC",
        PartitionType::Database => "Database",
        PartitionType::Unknown(_) => "Unknown",
    }
}

/// Format partition type for display (with ANSI colors).
pub(crate) fn format_partition_type(pt: PartitionType) -> String {
    match pt {
        PartitionType::Loader => style("Loader").yellow().to_string(),
        PartitionType::Normal => "Normal".to_string(),
        PartitionType::KvNv => style("KV-NV").magenta().to_string(),
        PartitionType::Efuse => style("eFuse").red().to_string(),
        PartitionType::Otp => style("OTP").red().to_string(),
        PartitionType::Flashboot => style("FlashBoot").yellow().to_string(),
        PartitionType::Factory => style("Factory").blue().to_string(),
        PartitionType::Version => "Version".to_string(),
        PartitionType::SecurityA => style("Security-A").red().to_string(),
        PartitionType::SecurityB => style("Security-B").red().to_string(),
        PartitionType::SecurityC => style("Security-C").red().to_string(),
        PartitionType::ProtocolA => "Protocol-A".to_string(),
        PartitionType::AppsA => "Apps-A".to_string(),
        PartitionType::RadioConfig => "RadioConfig".to_string(),
        PartitionType::Rom => "ROM".to_string(),
        PartitionType::Emmc => "eMMC".to_string(),
        PartitionType::Database => style("Database").dim().to_string(),
        PartitionType::Unknown(v) => format!("Unknown({v})"),
    }
}
