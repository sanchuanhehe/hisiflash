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
use std::env;
use std::io;
use std::path::PathBuf;

/// Whether stderr is a terminal (set once at startup).
static STDERR_IS_TTY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// Check if emoji/animations should be used (TTY and colors enabled).
fn use_fancy_output() -> bool {
    STDERR_IS_TTY.load(std::sync::atomic::Ordering::Relaxed) && console::colors_enabled_stderr()
}

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

    /// Path to a configuration file.
    #[arg(long = "config", global = true, value_name = "PATH")]
    config_path: Option<PathBuf>,

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

        /// Output information as JSON to stdout.
        #[arg(long)]
        json: bool,
    },

    /// List available serial ports.
    ListPorts {
        /// Output port list as JSON to stdout.
        #[arg(long)]
        json: bool,
    },

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
    // Inspect raw args early to support localized --help handling and early --lang
    let raw_args: Vec<String> = env::args().collect();

    // Extract --lang if provided early so help text is localized
    let mut early_lang: Option<String> = None;
    for (i, arg) in raw_args.iter().enumerate() {
        if let Some(val) = arg.strip_prefix("--lang=") {
            early_lang = Some(val.to_string());
        } else if arg == "--lang" && i + 1 < raw_args.len() {
            early_lang = Some(raw_args[i + 1].clone());
        }
    }

    let locale = early_lang.clone().unwrap_or_else(detect_locale);
    rust_i18n::set_locale(&locale);

    // --- NO_COLOR and TTY detection (clig.dev best practice) ---
    let stderr_is_tty = console::Term::stderr().is_term();
    STDERR_IS_TTY.store(stderr_is_tty, std::sync::atomic::Ordering::Relaxed);

    if env::var("NO_COLOR").is_ok() || !stderr_is_tty {
        // Disable all color output
        console::set_colors_enabled(false);
        console::set_colors_enabled_stderr(false);
    }

    debug!("Using locale: {}", locale);

    // If user asked for help (-h/--help), print localized help via clap with
    // translated section headings. This intercepts before clap's auto-help so
    // we can apply help_template with i18n strings.
    if raw_args.iter().any(|a| a == "-h" || a == "--help") {
        let mut app = build_localized_command();

        // Determine subcommand if provided
        let subcmd_names: Vec<String> = app
            .get_subcommands()
            .map(|s| s.get_name().to_string())
            .collect();
        let found = raw_args
            .iter()
            .skip(1)
            .find(|token| subcmd_names.iter().any(|n| n == token.as_str()));

        if let Some(cmd_name) = found {
            // Print subcommand help
            if let Some(sub) = app.get_subcommands().find(|s| s.get_name() == cmd_name.as_str()) {
                let _ = sub.clone().print_help();
            }
        } else {
            let _ = app.print_help();
        }
        std::process::exit(0);
    }

    let cli = Cli::parse();

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
    let mut config = if let Some(ref path) = cli.config_path {
        Config::load_from_path(path)
    } else {
        Config::load()
    };

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
        Commands::Info { firmware, json } => {
            cmd_info(firmware, *json)?;
        },
        Commands::ListPorts { json } => {
            cmd_list_ports(*json);
        },
        Commands::Monitor { monitor_baud } => {
            cmd_monitor(&cli, &mut config, *monitor_baud)?;
        },
        Commands::Completions { shell } => {
            cmd_completions(*shell);
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

/// Format partition type as a plain string (no ANSI colors) for JSON output.
fn partition_type_str(pt: PartitionType) -> &'static str {
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
fn cmd_info(firmware: &PathBuf, json: bool) -> Result<()> {
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

/// Info command --json output: structured JSON to stdout.
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

    println!("{}", serde_json::to_string_pretty(&info).unwrap_or_default());
    Ok(())
}

/// List ports command implementation.
fn cmd_list_ports(json: bool) {
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
        println!("{}", serde_json::to_string_pretty(&ports).unwrap_or_default());
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

/// Build a clap `Command` with localized help_template applied to all levels.
///
/// This uses clap as the single source of truth for options and subcommands,
/// while localizing section headings (USAGE/COMMANDS/OPTIONS). No need to
/// maintain a parallel hand-written help printer.
fn build_localized_command() -> clap::Command {
    let tpl = format!(
        "{bin} {version}\n\n{about}\n\n\
         {usage_h}:\n  {usage}\n\n\
         {cmds_h}:\n{subcommands}\n\n\
         {opts_h}:\n{options}\n",
        bin = "{bin}",
        version = "{version}",
        about = "{about}",
        usage_h = t!("help.usage_heading"),
        usage = "{usage}",
        cmds_h = t!("help.commands_heading"),
        subcommands = "{subcommands}",
        opts_h = t!("help.options_heading"),
        options = "{options}",
    );

    let sub_tpl = format!(
        "{bin} {version}\n\n{about}\n\n\
         {usage_h}:\n  {usage}\n\n\
         {opts_h}:\n{options}\n",
        bin = "{bin}",
        version = "{version}",
        about = "{about}",
        usage_h = t!("help.usage_heading"),
        usage = "{usage}",
        opts_h = t!("help.options_heading"),
        options = "{options}",
    );

    Cli::command()
        .help_template(&tpl)
        .mut_subcommands(move |sub| sub.help_template(sub_tpl.clone()))
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
