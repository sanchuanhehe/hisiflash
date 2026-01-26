//! hisiflash CLI - Command-line tool for flashing HiSilicon chips.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use console::style;
use env_logger::Env;
use hisiflash::{ChipFamily, Fwpkg, Ws63Flasher};
use indicatif::{ProgressBar, ProgressStyle};
use log::error;
use std::path::PathBuf;

mod commands;

/// hisiflash - A cross-platform tool for flashing HiSilicon chips.
#[derive(Parser)]
#[command(name = "hisiflash")]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
struct Cli {
    /// Serial port to use (auto-detected if not specified).
    #[arg(short, long, env = "HISIFLASH_PORT")]
    port: Option<String>,

    /// Baud rate for data transfer.
    #[arg(short, long, default_value = "921600", env = "HISIFLASH_BAUD")]
    baud: u32,

    /// Target chip type.
    #[arg(short, long, default_value = "ws63", env = "HISIFLASH_CHIP")]
    chip: Chip,

    /// Verbose output level (-v, -vv, -vvv).
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,

    #[command(subcommand)]
    command: Commands,
}

/// Supported chip types.
#[derive(Clone, Copy, Debug, ValueEnum)]
enum Chip {
    /// WS63 chip (WiFi + BLE, default).
    Ws63,
    /// BS2X series (BS21, BLE only).
    Bs2x,
    /// BS25 (BLE with enhanced features).
    Bs25,
}

impl From<Chip> for ChipFamily {
    fn from(chip: Chip) -> Self {
        match chip {
            Chip::Ws63 => ChipFamily::Ws63,
            Chip::Bs2x => ChipFamily::Bs2x,
            Chip::Bs25 => ChipFamily::Bs25,
        }
    }
}

/// Available commands.
#[derive(Subcommand)]
enum Commands {
    /// Flash a FWPKG firmware package.
    Flash {
        /// Path to the FWPKG firmware file.
        firmware: PathBuf,

        /// Only flash specified partitions (comma-separated).
        #[arg(short = 'F', long)]
        filter: Option<String>,

        /// Use late baud rate change (after LoaderBoot).
        #[arg(long)]
        late_baud: bool,

        /// Skip CRC verification.
        #[arg(long)]
        skip_verify: bool,
    },

    /// Write raw binary files to flash.
    Write {
        /// LoaderBoot binary file.
        #[arg(long, required = true)]
        loaderboot: PathBuf,

        /// Binary file to flash (format: file:address, can be repeated).
        #[arg(short = 'B', long = "bin", value_parser = parse_bin_arg)]
        bins: Vec<(PathBuf, u32)>,

        /// Use late baud rate change.
        #[arg(long)]
        late_baud: bool,
    },

    /// Write a single binary with program data.
    WriteProgram {
        /// LoaderBoot binary file.
        #[arg(long, required = true)]
        loaderboot: PathBuf,

        /// Program binary file.
        program: PathBuf,

        /// Flash address for program.
        #[arg(short, long, value_parser = parse_hex_u32)]
        address: u32,

        /// Use late baud rate change.
        #[arg(long)]
        late_baud: bool,
    },

    /// Erase flash memory.
    Erase {
        /// Erase entire flash (required confirmation).
        #[arg(long)]
        all: bool,
    },

    /// Show information about a firmware file.
    Info {
        /// Path to the FWPKG firmware file.
        firmware: PathBuf,
    },

    /// List available serial ports.
    ListPorts,
}

/// Parse binary argument in format "file:address".
fn parse_bin_arg(s: &str) -> Result<(PathBuf, u32), String> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 2 {
        return Err(format!(
            "Invalid format: '{s}'. Expected 'file:address' (e.g., 'firmware.bin:0x00800000')"
        ));
    }

    let path = PathBuf::from(parts[0]);
    let addr = parse_hex_u32(parts[1])?;

    Ok((path, addr))
}

/// Parse hexadecimal address.
fn parse_hex_u32(s: &str) -> Result<u32, String> {
    let s = s.trim_start_matches("0x").trim_start_matches("0X");
    u32::from_str_radix(s, 16).map_err(|e| format!("Invalid hex address: {e}"))
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Setup logging
    let log_level = match cli.verbose {
        0 => "info",
        1 => "debug",
        _ => "trace",
    };
    env_logger::Builder::from_env(Env::default().default_filter_or(log_level)).init();

    match &cli.command {
        Commands::Flash {
            firmware,
            filter,
            late_baud,
            skip_verify,
        } => {
            cmd_flash(&cli, firmware, filter.as_ref(), *late_baud, *skip_verify)?;
        },
        Commands::Write {
            loaderboot,
            bins,
            late_baud,
        } => {
            cmd_write(&cli, loaderboot, bins, *late_baud)?;
        },
        Commands::WriteProgram {
            loaderboot,
            program,
            address,
            late_baud,
        } => {
            cmd_write_program(&cli, loaderboot, program.clone(), *address, *late_baud)?;
        },
        Commands::Erase { all } => {
            cmd_erase(&cli, *all)?;
        },
        Commands::Info { firmware } => {
            cmd_info(firmware)?;
        },
        Commands::ListPorts => {
            cmd_list_ports();
        },
    }

    Ok(())
}

/// Flash command implementation.
fn cmd_flash(
    cli: &Cli,
    firmware: &PathBuf,
    filter: Option<&String>,
    late_baud: bool,
    skip_verify: bool,
) -> Result<()> {
    println!(
        "{} Loading firmware: {}",
        style("📦").cyan(),
        firmware.display()
    );

    // Load FWPKG
    let fwpkg = Fwpkg::from_file(firmware)
        .with_context(|| format!("Failed to load firmware: {}", firmware.display()))?;

    // Verify CRC
    if !skip_verify {
        fwpkg
            .verify_crc()
            .context("Firmware CRC verification failed")?;
        println!("{} CRC verification passed", style("✓").green());
    }

    // Show partition info
    println!(
        "{} Found {} partition(s):",
        style("ℹ").blue(),
        fwpkg.partition_count()
    );
    for bin in &fwpkg.bins {
        let type_str = if bin.is_loaderboot() {
            "(LoaderBoot)"
        } else {
            ""
        };
        println!(
            "    {} {} @ 0x{:08X} ({} bytes) {}",
            style("•").dim(),
            bin.name,
            bin.burn_addr,
            bin.length,
            style(type_str).yellow()
        );
    }

    // Get port
    let port = get_port(cli)?;
    println!(
        "{} Using port: {} @ {} baud",
        style("🔌").cyan(),
        port,
        cli.baud
    );

    // Create flasher
    let mut flasher = Ws63Flasher::new(&port, cli.baud)?
        .with_late_baud(late_baud)
        .with_verbose(cli.verbose);

    // Connect
    println!(
        "{} Waiting for device... (reset to enter download mode)",
        style("⏳").yellow()
    );
    flasher.connect()?;
    println!("{} Connected!", style("✓").green());

    // Create progress bar
    let pb = ProgressBar::new(100);
    #[allow(clippy::unwrap_used)] // Static template string, unwrap is safe
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}% {msg}")
            .unwrap()
            .progress_chars("#>-"),
    );

    // Flash
    let filter_names: Option<Vec<&str>> = filter.as_ref().map(|f| f.split(',').collect());
    let filter_slice = filter_names.as_deref();

    let mut current_partition = String::new();

    flasher.flash_fwpkg(&fwpkg, filter_slice, |name, current, total| {
        if name != current_partition {
            current_partition = name.to_string();
            pb.set_message(format!("Flashing {name}"));
        }
        if total > 0 {
            pb.set_position((current * 100 / total) as u64);
        }
    })?;

    pb.finish_with_message("Complete!");

    // Reset device
    println!("{} Resetting device...", style("🔄").cyan());
    flasher.reset()?;

    println!(
        "\n{} Flashing completed successfully!",
        style("🎉").green().bold()
    );

    Ok(())
}

/// Write command implementation.
fn cmd_write(
    cli: &Cli,
    loaderboot: &PathBuf,
    bins: &[(PathBuf, u32)],
    late_baud: bool,
) -> Result<()> {
    println!(
        "{} Loading LoaderBoot: {}",
        style("📦").cyan(),
        loaderboot.display()
    );

    let lb_data = std::fs::read(loaderboot)
        .with_context(|| format!("Failed to read LoaderBoot: {}", loaderboot.display()))?;

    let mut bin_data: Vec<(Vec<u8>, u32)> = Vec::new();
    for (path, addr) in bins {
        println!(
            "{} Loading binary: {} -> 0x{:08X}",
            style("📦").cyan(),
            path.display(),
            addr
        );
        let data = std::fs::read(path)
            .with_context(|| format!("Failed to read binary: {}", path.display()))?;
        bin_data.push((data, *addr));
    }

    let port = get_port(cli)?;
    println!(
        "{} Using port: {} @ {} baud",
        style("🔌").cyan(),
        port,
        cli.baud
    );

    let mut flasher = Ws63Flasher::new(&port, cli.baud)?
        .with_late_baud(late_baud)
        .with_verbose(cli.verbose);

    println!("{} Waiting for device...", style("⏳").yellow());
    flasher.connect()?;
    println!("{} Connected!", style("✓").green());

    let bins_ref: Vec<(&[u8], u32)> = bin_data.iter().map(|(d, a)| (d.as_slice(), *a)).collect();
    flasher.write_bins(&lb_data, &bins_ref)?;

    flasher.reset()?;
    println!(
        "\n{} Write completed successfully!",
        style("🎉").green().bold()
    );

    Ok(())
}

/// Write program command implementation.
fn cmd_write_program(
    cli: &Cli,
    loaderboot: &PathBuf,
    program: PathBuf,
    address: u32,
    late_baud: bool,
) -> Result<()> {
    cmd_write(cli, loaderboot, &[(program, address)], late_baud)
}

/// Erase command implementation.
fn cmd_erase(cli: &Cli, all: bool) -> Result<()> {
    if !all {
        error!("Please specify --all to erase entire flash");
        println!(
            "{} Use --all flag to confirm full erase",
            style("⚠").yellow()
        );
        return Ok(());
    }

    let port = get_port(cli)?;
    println!(
        "{} Using port: {} @ {} baud",
        style("🔌").cyan(),
        port,
        cli.baud
    );

    let mut flasher = Ws63Flasher::new(&port, cli.baud)?.with_verbose(cli.verbose);

    println!("{} Waiting for device...", style("⏳").yellow());
    flasher.connect()?;
    println!("{} Connected!", style("✓").green());

    println!(
        "{} Erasing flash... This may take a while.",
        style("🗑").red()
    );
    flasher.erase_all()?;

    println!("\n{} Erase completed!", style("✓").green().bold());

    Ok(())
}

/// Info command implementation.
fn cmd_info(firmware: &PathBuf) -> Result<()> {
    println!(
        "{} Loading firmware: {}",
        style("📦").cyan(),
        firmware.display()
    );

    let fwpkg = Fwpkg::from_file(firmware)
        .with_context(|| format!("Failed to load firmware: {}", firmware.display()))?;

    println!("\n{}", style("FWPKG Information").bold().underlined());
    println!("  Partitions: {}", fwpkg.partition_count());
    println!("  Total size: {} bytes", fwpkg.header.len);
    println!("  CRC: 0x{:04X}", fwpkg.header.crc);

    // Verify CRC
    match fwpkg.verify_crc() {
        Ok(()) => println!("  CRC Valid: {}", style("Yes").green()),
        Err(_) => println!("  CRC Valid: {}", style("No").red()),
    }

    println!("\n{}", style("Partitions").bold().underlined());
    for (i, bin) in fwpkg.bins.iter().enumerate() {
        let type_str = if bin.is_loaderboot() {
            style("LoaderBoot").yellow().to_string()
        } else {
            "Normal".to_string()
        };

        println!("\n  [{:2}] {}", i, style(&bin.name).cyan().bold());
        println!("       Type:       {type_str}");
        println!("       Offset:     0x{:08X}", bin.offset);
        println!("       Length:     {} bytes", bin.length);
        println!("       Burn Addr:  0x{:08X}", bin.burn_addr);
        println!("       Burn Size:  {} bytes", bin.burn_size);
    }

    Ok(())
}

/// List ports command implementation.
fn cmd_list_ports() {
    println!("{}", style("Available Serial Ports").bold().underlined());

    let detected = hisiflash::connection::detect::detect_ports();

    if detected.is_empty() {
        println!("  {}", style("No serial ports found").dim());
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

            println!(
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
            println!(
                "\n{} Auto-detected: {}",
                style("→").green().bold(),
                style(&auto_port.name).cyan().bold()
            );
        }
    }
}

/// Get serial port from CLI args or auto-detect.
fn get_port(cli: &Cli) -> Result<String> {
    match &cli.port {
        Some(p) => Ok(p.clone()),
        None => {
            // Try to auto-detect using VID/PID
            if let Ok(port) = hisiflash::connection::detect::auto_detect_port() {
                println!(
                    "{} Auto-detected port: {} [{}]",
                    style("🔍").cyan(),
                    style(&port.name).green(),
                    port.device.name()
                );
                Ok(port.name)
            } else {
                // Fall back to first available port
                hisiflash::connection::find_port(None).context(
                    "No serial port specified and auto-detection failed. Use -p to specify a port.",
                )
            }
        },
    }
}
