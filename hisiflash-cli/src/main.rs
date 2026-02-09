//! hisiflash CLI - Command-line tool for flashing HiSilicon chips.
//!
//! ## Features
//!
//! - Flash FWPKG firmware packages
//! - Write raw binary files to flash
//! - Erase flash memory
//! - Interactive serial port selection
//! - Shell completion generation
//! - Environment variable support
//! - Internationalization (i18n) support

use anyhow::{Context, Result};
use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::{Shell, generate};
use console::style;
use env_logger::Env;
use hisiflash::{ChipFamily, Fwpkg, FwpkgVersion, PartitionType};
use indicatif::{ProgressBar, ProgressStyle};
use log::{debug, error, warn};
use rust_i18n::t;
use std::io;
use std::path::PathBuf;

mod commands;
mod config;
mod serial;

use config::Config;
use serial::{SerialOptions, ask_remember_port, select_serial_port};

// Initialize i18n with locale files from the locales directory
rust_i18n::i18n!("locales", fallback = "en");

/// hisiflash - A cross-platform tool for flashing HiSilicon chips.
///
/// Environment variables:
///   HISIFLASH_PORT              - Default serial port
///   HISIFLASH_BAUD              - Default baud rate (default: 921600)
///   HISIFLASH_CHIP              - Default chip type (ws63, bs2x, bs25)
///   HISIFLASH_LANG              - Language/locale (en, zh-CN)
///   HISIFLASH_NON_INTERACTIVE   - Non-interactive mode (disable prompts)
#[derive(Parser)]
#[command(name = "hisiflash")]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
#[command(after_help = "For more information, visit: https://github.com/sanchuanhehe/hisiflash")]
#[allow(clippy::struct_excessive_bools)]
struct Cli {
    /// Serial port to use (auto-detected if not specified).
    #[arg(short, long, global = true, env = "HISIFLASH_PORT")]
    port: Option<String>,

    /// Baud rate for data transfer.
    #[arg(
        short,
        long,
        global = true,
        default_value = "921600",
        env = "HISIFLASH_BAUD"
    )]
    baud: u32,

    /// Target chip type.
    #[arg(
        short,
        long,
        global = true,
        default_value = "ws63",
        env = "HISIFLASH_CHIP"
    )]
    chip: Chip,

    /// Language/locale for messages (e.g., en, zh-CN).
    #[arg(long, global = true, env = "HISIFLASH_LANG")]
    lang: Option<String>,

    /// Verbose output level (-v, -vv, -vvv for increasing detail).
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    verbose: u8,

    /// Quiet mode (suppress non-essential output).
    #[arg(short, long, global = true)]
    quiet: bool,

    /// Non-interactive mode (fail instead of prompting).
    #[arg(long, global = true, env = "HISIFLASH_NON_INTERACTIVE")]
    non_interactive: bool,

    /// Confirm port selection even for auto-detected ports.
    #[arg(long, global = true)]
    confirm_port: bool,

    /// List all available ports (including unknown types).
    #[arg(long, global = true)]
    list_all_ports: bool,

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
        #[arg(long)]
        filter: Option<String>,

        /// Use late baud rate change (after LoaderBoot).
        #[arg(long)]
        late_baud: bool,

        /// Skip CRC verification.
        #[arg(long)]
        skip_verify: bool,

        /// Open serial monitor after flashing.
        #[arg(long)]
        monitor: bool,
    },

    /// Write raw binary files to flash.
    Write {
        /// LoaderBoot binary file.
        #[arg(long, required = true)]
        loaderboot: PathBuf,

        /// Binary file to flash (format: file:address, can be repeated).
        #[arg(long = "bin", value_parser = parse_bin_arg)]
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

    /// Open serial monitor.
    Monitor {
        /// Baud rate for monitoring (default: 115200).
        #[arg(long, default_value = "115200")]
        monitor_baud: u32,
    },

    /// Generate shell completion scripts.
    Completions {
        /// Shell type for completions.
        #[arg(value_enum)]
        shell: Shell,
    },

    /// Show localized help for a subcommand
    Help {
        /// Subcommand name to show help for (e.g., flash, write)
        command: Option<String>,
    },
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

/// Parse hexadecimal address (supports 0x prefix and underscores).
fn parse_hex_u32(s: &str) -> Result<u32, String> {
    let s = s.trim();
    let s = s.trim_start_matches("0x").trim_start_matches("0X");
    // Support underscore separators like 0x00_80_00_00
    let s: String = s.chars().filter(|c| *c != '_').collect();
    u32::from_str_radix(&s, 16).map_err(|e| format!("Invalid hex address: {e}"))
}

/// Supported locales for i18n
const SUPPORTED_LOCALES: &[&str] = &["en", "zh-CN"];

/// Detect the best matching locale from system settings.
///
/// This function tries to match the system locale to one of the supported locales.
/// It handles various locale formats like:
/// - `zh_CN.UTF-8` -> `zh-CN`
/// - `zh-CN` -> `zh-CN`
/// - `zh` -> `zh-CN`
/// - `en_US.UTF-8` -> `en`
/// - `C` or `POSIX` -> `en`
fn detect_locale() -> String {
    let system_locale = sys_locale::get_locale().unwrap_or_else(|| "en".to_string());

    // Normalize the locale string
    // Remove encoding suffix (e.g., .UTF-8)
    let locale = system_locale.split('.').next().unwrap_or(&system_locale);

    // Replace underscore with hyphen for BCP 47 format
    let locale = locale.replace('_', "-");

    // Try exact match first
    if SUPPORTED_LOCALES.contains(&locale.as_str()) {
        return locale;
    }

    // Try matching by language code (first part before hyphen)
    let lang_code = locale.split('-').next().unwrap_or(&locale);

    match lang_code.to_lowercase().as_str() {
        "zh" => "zh-CN".to_string(), // Chinese -> Simplified Chinese
        _ => "en".to_string(),       // English and all others fallback to English
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize i18n locale
    let locale = cli.lang.clone().unwrap_or_else(detect_locale);
    rust_i18n::set_locale(&locale);
    debug!("Using locale: {locale}");

    // Setup logging based on verbosity
    let log_level = if cli.quiet {
        "warn"
    } else {
        match cli.verbose {
            0 => "info",
            1 => "debug",
            _ => "trace",
        }
    };
    env_logger::Builder::from_env(Env::default().default_filter_or(log_level))
        .format_target(cli.verbose >= 2)
        .format_timestamp(if cli.verbose >= 2 {
            Some(env_logger::TimestampPrecision::Millis)
        } else {
            None
        })
        .init();

    debug!(
        "hisiflash v{} (verbose level: {})",
        env!("CARGO_PKG_VERSION"),
        cli.verbose
    );

    // Load configuration
    let mut config = Config::load();

    match &cli.command {
        Commands::Flash {
            firmware,
            filter,
            late_baud,
            skip_verify,
            monitor,
        } => {
            cmd_flash(
                &cli,
                &mut config,
                firmware,
                filter.as_ref(),
                *late_baud,
                *skip_verify,
            )?;
            if *monitor {
                // TODO: Implement monitor after flash
                warn!("Monitor after flash not yet implemented");
            }
        },
        Commands::Write {
            loaderboot,
            bins,
            late_baud,
        } => {
            cmd_write(&cli, &mut config, loaderboot, bins, *late_baud)?;
        },
        Commands::WriteProgram {
            loaderboot,
            program,
            address,
            late_baud,
        } => {
            cmd_write_program(
                &cli,
                &mut config,
                loaderboot,
                program.clone(),
                *address,
                *late_baud,
            )?;
        },
        Commands::Erase { all } => {
            cmd_erase(&cli, &mut config, *all)?;
        },
        Commands::Info { firmware } => {
            cmd_info(firmware)?;
        },
        Commands::ListPorts => {
            cmd_list_ports();
        },
        Commands::Monitor { monitor_baud } => {
            cmd_monitor(&cli, &mut config, *monitor_baud)?;
        },
        Commands::Completions { shell } => {
            cmd_completions(*shell);
        },
        Commands::Help { command } => {
            cmd_help(command.as_deref());
        },
    }

    Ok(())
}

/// Get serial port from CLI args or interactive selection.
fn get_port(cli: &Cli, config: &mut Config) -> Result<String> {
    let options = SerialOptions {
        port: cli.port.clone(),
        list_all_ports: cli.list_all_ports,
        non_interactive: cli.non_interactive,
        confirm_port: cli.confirm_port,
    };

    let selected = select_serial_port(&options, config)?;

    // Ask to remember if not a known device and interactive mode
    if !selected.is_known && !cli.non_interactive {
        ask_remember_port(&selected.port, config)?;
    }

    Ok(selected.port.name)
}

/// Flash command implementation.
fn cmd_flash(
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
    let pb = if cli.quiet {
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
fn cmd_write(
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
fn cmd_write_program(
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
fn cmd_erase(cli: &Cli, config: &mut Config, all: bool) -> Result<()> {
    if !all {
        error!("{}", t!("erase.need_all_flag"));
        if !cli.quiet {
            eprintln!("{} {}", style("⚠").yellow(), t!("erase.use_all_flag"));
        }
        std::process::exit(2);
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

/// Format partition type for display.
fn format_partition_type(pt: PartitionType) -> String {
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

/// Info command implementation.
fn cmd_info(firmware: &PathBuf) -> Result<()> {
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

/// List ports command implementation.
fn cmd_list_ports() {
    eprintln!("{}", style(t!("list_ports.header")).bold().underlined());

    let detected = hisiflash::connection::detect::detect_ports();

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

/// Monitor command implementation.
fn cmd_monitor(cli: &Cli, config: &mut Config, monitor_baud: u32) -> Result<()> {
    use std::io::Write;

    let port = get_port(cli, config)?;

    eprintln!(
        "{} {}",
        style("📡").cyan(),
        t!(
            "monitor.opening",
            port = style(&port).green().to_string(),
            baud = monitor_baud
        )
    );
    eprintln!("{}", style(t!("monitor.exit_hint")).dim());

    // Simple serial monitor
    let mut serial = serialport::new(&port, monitor_baud)
        .timeout(std::time::Duration::from_millis(100))
        .open()
        .with_context(|| t!("error.open_port", port = port.clone()))?;

    let mut buf = [0u8; 1024];
    loop {
        match serial.read(&mut buf) {
            Ok(n) if n > 0 => {
                // Print received data
                let data = &buf[..n];
                if let Ok(s) = std::str::from_utf8(data) {
                    print!("{s}");
                } else {
                    // Hex dump for non-UTF8 data
                    for byte in data {
                        print!("{byte:02X} ");
                    }
                }
                io::stdout().flush().ok();
            },
            Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => {
                // Timeout is expected, continue
            },
            Err(e) => {
                return Err(e).context(t!("error.serial_error").to_string());
            },
            _ => {},
        }
    }
}

/// Generate shell completions.
fn cmd_completions(shell: Shell) {
    let mut cmd = Cli::command();
    let name = cmd.get_name().to_string();
    generate(shell, &mut cmd, name, &mut io::stdout());
}

/// Localized help printer for subcommands.
fn cmd_help(cmd: Option<&str>) {
    // Top-level help
    if cmd.is_none() {
        eprintln!("{}", t!("help.header"));
        eprintln!();
        eprintln!("  {}", t!("help.usage"));
        eprintln!("");
        eprintln!("{}:", t!("help.commands"));
        eprintln!("  flash    - {}", t!("help.flash.summary"));
        eprintln!("  write    - {}", t!("help.write.summary"));
        eprintln!("  write-program - {}", t!("help.write_program.summary"));
        eprintln!("  erase    - {}", t!("help.erase.summary"));
        eprintln!("  info     - {}", t!("help.info.summary"));
        eprintln!("  list-ports - {}", t!("help.list_ports.summary"));
        eprintln!("  monitor  - {}", t!("help.monitor.summary"));
        eprintln!("  completions - {}", t!("help.completions.summary"));
        eprintln!("  help     - {}", t!("help.help.summary"));
        eprintln!("");
        eprintln!("{}", t!("help.more"));
        return;
    }

    match cmd.unwrap() {
        "flash" => {
            eprintln!("{}", t!("help.flash.title"));
            eprintln!("");
            eprintln!("{}", t!("help.flash.usage"));
            eprintln!("");
            eprintln!("{}", t!("help.flash.description"));
            eprintln!("");
            eprintln!("{}", t!("help.flash.options_header"));
            eprintln!("  - --filter <FILTER>    {}", t!("help.flash.opt.filter"));
            eprintln!("  - --port <PORT>        {}", t!("help.opt.port"));
            eprintln!("  - --baud <BAUD>        {}", t!("help.opt.baud"));
            eprintln!("  - --chip <CHIP>        {}", t!("help.opt.chip"));
            eprintln!("  - --lang <LANG>        {}", t!("help.opt.lang"));
            eprintln!(
                "  - --late-baud          {}",
                t!("help.flash.opt.late_baud")
            );
            eprintln!(
                "  - --skip-verify        {}",
                t!("help.flash.opt.skip_verify")
            );
            eprintln!("  - --monitor            {}", t!("help.flash.opt.monitor"));
            eprintln!("  - -v/--verbose         {}", t!("help.opt.verbose"));
            eprintln!("  - -q/--quiet           {}", t!("help.opt.quiet"));
        },
        "write" => {
            eprintln!("{}", t!("help.write.title"));
            eprintln!();
            eprintln!("{}", t!("help.write.usage"));
            eprintln!("");
            eprintln!("{}", t!("help.write.description"));
            eprintln!("");
            eprintln!("{}", t!("help.write.options_header"));
            eprintln!(
                "  - --loaderboot <FILE>  {}",
                t!("help.write.opt.loaderboot")
            );
            eprintln!("  - --bin <FILE:ADDR>    {}", t!("help.write.opt.bins"));
            eprintln!("  - --late-baud          {}", t!("help.opt.late_baud"));
        },
        "erase" => {
            eprintln!("{}", t!("help.erase.title"));
            eprintln!("");
            eprintln!("{}", t!("help.erase.usage"));
            eprintln!("");
            eprintln!("{}", t!("help.erase.description"));
            eprintln!("");
            eprintln!("  --all    {}", t!("help.erase.opt.all"));
        },
        "info" => {
            eprintln!("{}", t!("help.info.title"));
            eprintln!("");
            eprintln!("{}", t!("help.info.usage"));
            eprintln!("");
            eprintln!("{}", t!("help.info.description"));
        },
        "list-ports" => {
            eprintln!("{}", t!("help.list_ports.title"));
            eprintln!("");
            eprintln!("{}", t!("help.list_ports.usage"));
            eprintln!("");
            eprintln!("{}", t!("help.list_ports.description"));
            eprintln!("  --list-all-ports  {}", t!("help.list_ports.opt.list_all"));
        },
        "monitor" => {
            eprintln!("{}", t!("help.monitor.title"));
            eprintln!("");
            eprintln!("{}", t!("help.monitor.usage"));
            eprintln!("");
            eprintln!("{}", t!("help.monitor.description"));
            eprintln!("  --monitor-baud <BAUD> {}", t!("help.monitor.opt.baud"));
        },
        "completions" => {
            eprintln!("{}", t!("help.completions.title"));
            eprintln!("");
            eprintln!("{}", t!("help.completions.description"));
        },
        other => {
            eprintln!("{}: {}", t!("help.unknown"), other);
        },
    }
}

#[cfg(test)]
mod locale_tests {
    use super::*;

    /// Helper to test locale matching logic without sys_locale
    fn match_locale(locale: &str) -> String {
        // Normalize the locale string
        let locale = locale.split('.').next().unwrap_or(locale);
        let locale = locale.replace('_', "-");

        if SUPPORTED_LOCALES.contains(&locale.as_str()) {
            return locale;
        }

        let lang_code = locale.split('-').next().unwrap_or(&locale);
        match lang_code.to_lowercase().as_str() {
            "zh" => "zh-CN".to_string(),
            _ => "en".to_string(),
        }
    }

    #[test]
    fn test_locale_chinese_variants() {
        assert_eq!(match_locale("zh_CN.UTF-8"), "zh-CN");
        assert_eq!(match_locale("zh_CN"), "zh-CN");
        assert_eq!(match_locale("zh-CN"), "zh-CN");
        assert_eq!(match_locale("zh_TW.UTF-8"), "zh-CN"); // Taiwan -> Simplified (fallback)
        assert_eq!(match_locale("zh"), "zh-CN");
    }

    #[test]
    fn test_locale_english_variants() {
        assert_eq!(match_locale("en_US.UTF-8"), "en");
        assert_eq!(match_locale("en_GB.UTF-8"), "en");
        assert_eq!(match_locale("en_US"), "en");
        assert_eq!(match_locale("en"), "en");
    }

    #[test]
    fn test_locale_posix_defaults() {
        assert_eq!(match_locale("C"), "en");
        assert_eq!(match_locale("POSIX"), "en");
        assert_eq!(match_locale("C.UTF-8"), "en");
    }

    #[test]
    fn test_locale_unsupported_fallback() {
        assert_eq!(match_locale("de_DE.UTF-8"), "en"); // German -> English
        assert_eq!(match_locale("ja_JP.UTF-8"), "en"); // Japanese -> English
        assert_eq!(match_locale("fr_FR"), "en"); // French -> English
    }
}
