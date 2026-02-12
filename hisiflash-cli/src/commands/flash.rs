//! Flash, write, and erase command implementations.

use anyhow::{Context, Result};
use console::style;
use hisiflash::{ChipFamily, Fwpkg};
use indicatif::{ProgressBar, ProgressStyle};
use rust_i18n::t;
use std::path::PathBuf;

use crate::config::Config;
use crate::{Cli, CliError, get_port, use_fancy_output};

/// Flash command implementation.
pub(crate) fn cmd_flash(
    cli: &Cli,
    config: &mut Config,
    firmware: &PathBuf,
    filter: Option<&String>,
    late_baud: bool,
    skip_verify: bool,
) -> Result<()> {
    if !cli.quiet {
        eprintln!(
            "{} {}",
            style("📦").cyan(),
            t!("flash.loading_firmware", path = firmware.display())
        );
    }

    // Load FWPKG
    let fwpkg = Fwpkg::from_file(firmware)
        .with_context(|| t!("error.load_firmware", path = firmware.display().to_string()))?;

    // Verify CRC
    if !skip_verify {
        fwpkg
            .verify_crc()
            .context(t!("error.crc_failed").to_string())?;
        if !cli.quiet {
            eprintln!("{} {}", style("✓").green(), t!("flash.crc_passed"));
        }
    }

    // Show partition info
    if !cli.quiet {
        eprintln!(
            "{} {}",
            style("ℹ").blue(),
            t!("flash.found_partitions", count = fwpkg.partition_count())
        );
        for bin in &fwpkg.bins {
            let type_str = if bin.is_loaderboot() {
                "(LoaderBoot)"
            } else {
                ""
            };
            eprintln!(
                "    {} {} @ 0x{:08X} ({} bytes) {}",
                style("•").dim(),
                bin.name,
                bin.burn_addr,
                bin.length,
                style(type_str).yellow()
            );
        }
    }

    // Get port
    let port = get_port(cli, config)?;
    if !cli.quiet {
        eprintln!(
            "{} {}",
            style("🔌").cyan(),
            t!("common.using_port", port = port, baud = cli.baud)
        );
    }

    // Create flasher using chip abstraction
    let chip: ChipFamily = cli.chip.into();
    let mut flasher = chip.create_flasher(&port, cli.baud, late_baud, cli.verbose)?;

    // Connect
    if !cli.quiet {
        eprintln!("{} {}", style("⏳").yellow(), t!("common.waiting_device"));
    }
    flasher.connect()?;
    if !cli.quiet {
        eprintln!("{} {}", style("✓").green(), t!("common.connected"));
    }

    // Create progress bar
    let pb = if cli.quiet || !use_fancy_output() {
        ProgressBar::hidden()
    } else {
        let pb = ProgressBar::new(100);
        #[allow(clippy::unwrap_used)] // Static template string
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}% {msg}")
                .unwrap()
                .progress_chars("#>-"),
        );
        pb.set_draw_target(indicatif::ProgressDrawTarget::stderr());
        pb
    };

    // Flash
    let filter_names: Option<Vec<&str>> = filter.as_ref().map(|f| f.split(',').collect());
    let filter_slice = filter_names.as_deref();

    let mut current_partition = String::new();

    flasher.flash_fwpkg(
        &fwpkg,
        filter_slice,
        &mut |name: &str, current: usize, total: usize| {
            if name != current_partition {
                current_partition = name.to_string();
                pb.set_message(t!("flash.flashing", name = name).to_string());
            }
            if total > 0 {
                pb.set_position((current * 100 / total) as u64);
            }
        },
    )?;

    pb.finish_with_message(t!("common.complete").to_string());

    // Reset device
    if !cli.quiet {
        eprintln!("{} {}", style("🔄").cyan(), t!("common.resetting"));
    }
    flasher.reset()?;

    // Close the flasher to release the serial port
    flasher.close();

    if !cli.quiet {
        eprintln!("\n{} {}", style("🎉").green().bold(), t!("flash.completed"));
    }

    Ok(())
}

/// Write command implementation.
pub(crate) fn cmd_write(
    cli: &Cli,
    config: &mut Config,
    loaderboot: &PathBuf,
    bins: &[(PathBuf, u32)],
    late_baud: bool,
) -> Result<()> {
    if !cli.quiet {
        eprintln!(
            "{} {}",
            style("📦").cyan(),
            t!("write.loading_loaderboot", path = loaderboot.display())
        );
    }

    let lb_data = std::fs::read(loaderboot).with_context(|| {
        t!(
            "error.read_loaderboot",
            path = loaderboot.display().to_string()
        )
    })?;

    let mut bin_data: Vec<(Vec<u8>, u32)> = Vec::new();
    for (path, addr) in bins {
        if !cli.quiet {
            eprintln!(
                "{} {}",
                style("📦").cyan(),
                t!(
                    "write.loading_binary",
                    path = path.display(),
                    addr = format!("{:08X}", addr)
                )
            );
        }
        let data = std::fs::read(path)
            .with_context(|| t!("error.read_binary", path = path.display().to_string()))?;
        bin_data.push((data, *addr));
    }

    let port = get_port(cli, config)?;
    if !cli.quiet {
        eprintln!(
            "{} {}",
            style("🔌").cyan(),
            t!("common.using_port", port = port, baud = cli.baud)
        );
    }

    let chip: ChipFamily = cli.chip.into();
    let mut flasher = chip.create_flasher(&port, cli.baud, late_baud, cli.verbose)?;

    if !cli.quiet {
        eprintln!("{} {}", style("⏳").yellow(), t!("common.waiting_device"));
    }
    flasher.connect()?;
    if !cli.quiet {
        eprintln!("{} {}", style("✓").green(), t!("common.connected"));
    }

    let bins_ref: Vec<(&[u8], u32)> = bin_data.iter().map(|(d, a)| (d.as_slice(), *a)).collect();
    flasher.write_bins(&lb_data, &bins_ref)?;

    flasher.reset()?;
    flasher.close();

    if !cli.quiet {
        eprintln!("\n{} {}", style("🎉").green().bold(), t!("write.completed"));
    }

    Ok(())
}

/// Write program command implementation.
pub(crate) fn cmd_write_program(
    cli: &Cli,
    config: &mut Config,
    loaderboot: &PathBuf,
    program: PathBuf,
    address: u32,
    late_baud: bool,
) -> Result<()> {
    cmd_write(cli, config, loaderboot, &[(program, address)], late_baud)
}

/// Erase command implementation.
pub(crate) fn cmd_erase(cli: &Cli, config: &mut Config, all: bool) -> Result<()> {
    if !all {
        if !cli.quiet {
            eprintln!("{} {}", style("⚠").yellow(), t!("erase.use_all_flag"));
        }
        return Err(CliError::Usage(t!("erase.need_all_flag").to_string()).into());
    }

    let port = get_port(cli, config)?;
    if !cli.quiet {
        eprintln!(
            "{} {}",
            style("🔌").cyan(),
            t!("common.using_port", port = port, baud = cli.baud)
        );
    }

    let chip: ChipFamily = cli.chip.into();
    let mut flasher = chip.create_flasher(&port, cli.baud, false, cli.verbose)?;

    if !cli.quiet {
        eprintln!("{} {}", style("⏳").yellow(), t!("common.waiting_device"));
    }
    flasher.connect()?;
    if !cli.quiet {
        eprintln!("{} {}", style("✓").green(), t!("common.connected"));
    }

    if !cli.quiet {
        eprintln!("{} {}", style("🗑").red(), t!("erase.erasing"));
    }
    flasher.erase_all()?;
    flasher.close();

    if !cli.quiet {
        eprintln!("\n{} {}", style("✓").green().bold(), t!("erase.completed"));
    }

    Ok(())
}
