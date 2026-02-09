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
use log::{debug, error};
use rust_i18n::t;
use std::env;
use std::fs;
use std::io;
use std::io::Write as _;
use std::path::{Path, PathBuf};

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
    /// BS2X series (BS21, BLE only) — planned, not yet supported.
    Bs2x,
    /// BS25 (BLE with enhanced features) — planned, not yet supported.
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
        /// Shell type for completions (auto-detected if not specified with --install).
        #[arg(value_enum)]
        shell: Option<Shell>,

        /// Automatically install completions to your shell configuration.
        #[arg(long)]
        install: bool,
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

    debug!("Using locale: {locale}");

    // If user asked for help (-h/--help) or provided no arguments at all,
    // print localized help via clap with translated section headings.
    // This intercepts before clap's auto-help so we can apply help_template
    // with i18n strings.
    let wants_help = raw_args.iter().any(|a| a == "-h" || a == "--help");
    let no_args = raw_args.len() <= 1;

    if wants_help || no_args {
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
            if let Some(sub) = app
                .get_subcommands()
                .find(|s| s.get_name() == cmd_name.as_str())
            {
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
                eprintln!();
                cmd_monitor(&cli, &mut config, 115200)?;
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
        Commands::Completions { shell, install } => {
            if *install {
                cmd_completions_install(*shell)?;
            } else {
                let shell = shell.unwrap_or_else(|| {
                    eprintln!(
                        "{} specify a shell type, e.g.: hisiflash completions bash",
                        style("Error:").red().bold()
                    );
                    eprintln!(
                        "  Or use {} to auto-install completions.",
                        style("hisiflash completions --install").cyan()
                    );
                    std::process::exit(1);
                });
                cmd_completions(shell);
            }
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

    println!(
        "{}",
        serde_json::to_string_pretty(&info).unwrap_or_default()
    );
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

/// Monitor command implementation.
fn cmd_monitor(cli: &Cli, config: &mut Config, monitor_baud: u32) -> Result<()> {
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

/// Detect the user's current shell from environment.
fn detect_shell_type() -> Option<Shell> {
    // Try $SHELL first (Unix)
    if let Ok(shell_path) = env::var("SHELL") {
        let shell_name = std::path::Path::new(&shell_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");
        return match shell_name {
            "bash" => Some(Shell::Bash),
            "zsh" => Some(Shell::Zsh),
            "fish" => Some(Shell::Fish),
            "elvish" => Some(Shell::Elvish),
            "pwsh" | "powershell" => Some(Shell::PowerShell),
            _ => None,
        };
    }

    // On Windows, try PSModulePath for PowerShell detection
    if cfg!(windows) && env::var("PSModulePath").is_ok() {
        return Some(Shell::PowerShell);
    }

    None
}

/// Get the completion script installation path for a given shell.
fn get_completion_install_path(shell: Shell) -> Result<PathBuf> {
    match shell {
        Shell::Bash => {
            // ~/.local/share/bash-completion/completions/hisiflash
            let dir = dirs_for_data().join("bash-completion").join("completions");
            Ok(dir.join("hisiflash"))
        },
        Shell::Zsh => {
            // ~/.zfunc/_hisiflash (common convention)
            let home = home_dir()?;
            let dir = home.join(".zfunc");
            Ok(dir.join("_hisiflash"))
        },
        Shell::Fish => {
            // ~/.config/fish/completions/hisiflash.fish
            let config_dir = xdg_config_dir();
            Ok(config_dir
                .join("fish")
                .join("completions")
                .join("hisiflash.fish"))
        },
        Shell::PowerShell => {
            // $PROFILE directory / hisiflash.ps1
            if let Ok(profile) = env::var("PROFILE") {
                let dir = PathBuf::from(&profile)
                    .parent()
                    .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
                Ok(dir.join("hisiflash.ps1"))
            } else {
                let home = home_dir()?;
                let dir = home.join(".config").join("powershell").join("completions");
                Ok(dir.join("hisiflash.ps1"))
            }
        },
        Shell::Elvish => {
            let config_dir = xdg_config_dir();
            Ok(config_dir.join("elvish").join("lib").join("hisiflash.elv"))
        },
        _ => anyhow::bail!("Unsupported shell for auto-install"),
    }
}

/// Get home directory.
fn home_dir() -> Result<PathBuf> {
    env::var("HOME")
        .or_else(|_| env::var("USERPROFILE"))
        .map(PathBuf::from)
        .context("Could not determine home directory")
}

/// Get XDG config directory (~/.config by default).
fn xdg_config_dir() -> PathBuf {
    env::var("XDG_CONFIG_HOME").map_or_else(
        |_| home_dir().unwrap_or_default().join(".config"),
        PathBuf::from,
    )
}

/// Get XDG data directory.
fn dirs_for_data() -> PathBuf {
    env::var("XDG_DATA_HOME").map_or_else(
        |_| home_dir().unwrap_or_default().join(".local").join("share"),
        PathBuf::from,
    )
}

/// Install shell completions automatically.
fn cmd_completions_install(shell_arg: Option<Shell>) -> Result<()> {
    let shell = match shell_arg {
        Some(s) => s,
        None => detect_shell_type().context(
            "Could not detect your shell. Please specify it explicitly:\n  \
             hisiflash completions --install bash",
        )?,
    };

    let path = get_completion_install_path(shell)?;

    // Generate the completion script to a buffer
    let mut buf = Vec::new();
    let mut cmd = Cli::command();
    let name = cmd.get_name().to_string();
    generate(shell, &mut cmd, name, &mut buf);

    // Create parent directory
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
    }

    // Write the completion file
    fs::write(&path, &buf)
        .with_context(|| format!("Failed to write completion file: {}", path.display()))?;

    eprintln!(
        "{} Installed {} completions to {}",
        style("✓").green().bold(),
        style(format!("{shell:?}")).cyan(),
        style(path.display()).yellow()
    );

    // Shell-specific post-install instructions
    match shell {
        Shell::Bash => {
            eprintln!();
            eprintln!("Completions will be loaded automatically on new terminals.");
            eprintln!(
                "To activate now: {}",
                style(format!("source {}", path.display())).cyan()
            );
        },
        Shell::Zsh => {
            let home = home_dir().unwrap_or_default();
            let zshrc = home.join(".zshrc");
            let fpath_line = "fpath=(~/.zfunc $fpath)";

            // Check if fpath line already exists in .zshrc
            let needs_fpath = if let Ok(content) = fs::read_to_string(&zshrc) {
                !content.contains(fpath_line)
            } else {
                true
            };

            if needs_fpath {
                // Append fpath line to .zshrc
                let mut file = fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&zshrc)
                    .with_context(|| format!("Failed to update {}", zshrc.display()))?;
                writeln!(file, "\n# hisiflash completions")?;
                writeln!(file, "{fpath_line}")?;
                writeln!(file, "autoload -Uz compinit && compinit")?;
                eprintln!(
                    "{} Added fpath to {}",
                    style("✓").green().bold(),
                    style(zshrc.display()).yellow()
                );
            }

            eprintln!();
            eprintln!("Restart your shell or run: {}", style("exec zsh").cyan());
        },
        Shell::Fish => {
            eprintln!();
            eprintln!("Completions will be loaded automatically on new Fish sessions.");
        },
        Shell::PowerShell => {
            eprintln!();
            eprintln!("Add this to your PowerShell profile to load on startup:");
            eprintln!(
                "  {}",
                style(format!("Import-Module {}", path.display())).cyan()
            );
        },
        Shell::Elvish => {
            eprintln!();
            eprintln!("Completions will be loaded automatically on new Elvish sessions.");
        },
        _ => {},
    }

    Ok(())
}

/// Build a clap `Command` with fully localized help output.
///
/// Uses clap as the single source of truth for structure (args, subcommands),
/// while replacing all user-visible text (section headings, command descriptions,
/// argument help) with translations from the locale files.
fn build_localized_command() -> clap::Command {
    let tpl = format!(
        "{bin} {version}\n\n{about}\n\n\
         {usage_h}:\n  {usage}\n\n\
         {cmds_h}:\n{subcommands}\n\n\
         {opts_h}:\n{options}\n\n\
         {after_help}\n",
        bin = "{bin}",
        version = "{version}",
        about = "{about}",
        usage_h = t!("help.usage_heading"),
        usage = "{usage}",
        cmds_h = t!("help.commands_heading"),
        subcommands = "{subcommands}",
        opts_h = t!("help.options_heading"),
        options = "{options}",
        after_help = "{after-help}",
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
        .about(t!("app.about").to_string())
        .after_help(t!("app.after_help").to_string())
        .mut_args(localize_arg)
        .mut_subcommands(move |sub| {
            let name = sub.get_name().to_string();
            let about_key = format!("cmd.{}.about", name.replace('-', "_"));
            let localized = t!(&about_key).to_string();
            let sub = if localized != about_key {
                sub.about(localized)
            } else {
                sub
            };
            sub.help_template(sub_tpl.clone()).mut_args(localize_arg)
        })
}

/// Replace an arg's help text with its localized version if available.
///
/// Looks up `arg.<id>.help` in the current locale. If found, replaces the
/// arg's help text; otherwise keeps the original (English from doc comments).
fn localize_arg(arg: clap::Arg) -> clap::Arg {
    let id = arg.get_id().as_str().to_string();
    let key = format!("arg.{id}.help");
    let localized = t!(&key).to_string();
    if localized != key {
        arg.help(localized)
    } else {
        arg
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

#[cfg(test)]
mod cli_tests {
    use super::*;
    use clap::CommandFactory;

    // ---- clap validation ----

    #[test]
    fn test_cli_command_is_valid() {
        // Verifies that all derive macros produce a valid clap Command
        Cli::command().debug_assert();
    }

    #[test]
    fn test_cli_parse_flash() {
        let cli = Cli::try_parse_from([
            "hisiflash",
            "--port",
            "/dev/ttyUSB0",
            "--baud",
            "460800",
            "flash",
            "firmware.fwpkg",
        ]);
        assert!(cli.is_ok());
        let cli = cli.unwrap();
        assert_eq!(cli.port.as_deref(), Some("/dev/ttyUSB0"));
        assert_eq!(cli.baud, 460800);
        assert!(matches!(cli.command, Commands::Flash { .. }));
    }

    #[test]
    fn test_cli_parse_flash_with_all_options() {
        let cli = Cli::try_parse_from([
            "hisiflash",
            "flash",
            "fw.fwpkg",
            "--filter",
            "app,flashboot",
            "--late-baud",
            "--skip-verify",
            "--monitor",
        ])
        .unwrap();
        if let Commands::Flash {
            firmware,
            filter,
            late_baud,
            skip_verify,
            monitor,
        } = cli.command
        {
            assert_eq!(firmware.to_str().unwrap(), "fw.fwpkg");
            assert_eq!(filter.as_deref(), Some("app,flashboot"));
            assert!(late_baud);
            assert!(skip_verify);
            assert!(monitor);
        } else {
            panic!("Expected Flash command");
        }
    }

    #[test]
    fn test_cli_parse_write() {
        let cli = Cli::try_parse_from([
            "hisiflash",
            "write",
            "--loaderboot",
            "lb.bin",
            "--bin",
            "app.bin:0x00800000",
        ])
        .unwrap();
        if let Commands::Write {
            loaderboot,
            bins,
            late_baud,
        } = cli.command
        {
            assert_eq!(loaderboot.to_str().unwrap(), "lb.bin");
            assert_eq!(bins.len(), 1);
            assert_eq!(bins[0].0.to_str().unwrap(), "app.bin");
            assert_eq!(bins[0].1, 0x00800000);
            assert!(!late_baud);
        } else {
            panic!("Expected Write command");
        }
    }

    #[test]
    fn test_cli_parse_write_program() {
        let cli = Cli::try_parse_from([
            "hisiflash",
            "write-program",
            "--loaderboot",
            "lb.bin",
            "program.bin",
            "--address",
            "0x00800000",
        ])
        .unwrap();
        assert!(matches!(cli.command, Commands::WriteProgram { .. }));
    }

    #[test]
    fn test_cli_parse_erase() {
        let cli = Cli::try_parse_from(["hisiflash", "erase", "--all"]).unwrap();
        if let Commands::Erase { all } = cli.command {
            assert!(all);
        } else {
            panic!("Expected Erase command");
        }
    }

    #[test]
    fn test_cli_parse_info() {
        let cli = Cli::try_parse_from(["hisiflash", "info", "firmware.fwpkg"]).unwrap();
        assert!(matches!(cli.command, Commands::Info { json: false, .. }));
    }

    #[test]
    fn test_cli_parse_info_json() {
        let cli = Cli::try_parse_from(["hisiflash", "info", "--json", "firmware.fwpkg"]).unwrap();
        if let Commands::Info { json, .. } = cli.command {
            assert!(json);
        } else {
            panic!("Expected Info command");
        }
    }

    #[test]
    fn test_cli_parse_list_ports() {
        let cli = Cli::try_parse_from(["hisiflash", "list-ports"]).unwrap();
        assert!(matches!(cli.command, Commands::ListPorts { json: false }));
    }

    #[test]
    fn test_cli_parse_list_ports_json() {
        let cli = Cli::try_parse_from(["hisiflash", "list-ports", "--json"]).unwrap();
        if let Commands::ListPorts { json } = cli.command {
            assert!(json);
        } else {
            panic!("Expected ListPorts command");
        }
    }

    #[test]
    fn test_cli_parse_monitor() {
        let cli = Cli::try_parse_from(["hisiflash", "monitor", "--monitor-baud", "9600"]).unwrap();
        if let Commands::Monitor { monitor_baud } = cli.command {
            assert_eq!(monitor_baud, 9600);
        } else {
            panic!("Expected Monitor command");
        }
    }

    #[test]
    fn test_cli_parse_monitor_default_baud() {
        let cli = Cli::try_parse_from(["hisiflash", "monitor"]).unwrap();
        if let Commands::Monitor { monitor_baud } = cli.command {
            assert_eq!(monitor_baud, 115200);
        } else {
            panic!("Expected Monitor command");
        }
    }

    #[test]
    fn test_cli_parse_completions() {
        let cli = Cli::try_parse_from(["hisiflash", "completions", "bash"]).unwrap();
        assert!(matches!(cli.command, Commands::Completions { .. }));
    }

    #[test]
    fn test_cli_default_values() {
        let cli = Cli::try_parse_from(["hisiflash", "list-ports"]).unwrap();
        assert_eq!(cli.baud, 921600);
        assert!(matches!(cli.chip, Chip::Ws63));
        assert!(!cli.quiet);
        assert!(!cli.non_interactive);
        assert!(!cli.confirm_port);
        assert!(!cli.list_all_ports);
        assert!(cli.port.is_none());
        assert!(cli.lang.is_none());
        assert!(cli.config_path.is_none());
        assert_eq!(cli.verbose, 0);
    }

    #[test]
    fn test_cli_global_options() {
        let cli = Cli::try_parse_from([
            "hisiflash",
            "--port",
            "COM3",
            "--baud",
            "115200",
            "--chip",
            "bs2x",
            "--lang",
            "zh-CN",
            "-vv",
            "--quiet",
            "--non-interactive",
            "--confirm-port",
            "--list-all-ports",
            "--config",
            "/tmp/config.toml",
            "list-ports",
        ])
        .unwrap();
        assert_eq!(cli.port.as_deref(), Some("COM3"));
        assert_eq!(cli.baud, 115200);
        assert!(matches!(cli.chip, Chip::Bs2x));
        assert_eq!(cli.lang.as_deref(), Some("zh-CN"));
        assert_eq!(cli.verbose, 2);
        assert!(cli.quiet);
        assert!(cli.non_interactive);
        assert!(cli.confirm_port);
        assert!(cli.list_all_ports);
    }

    #[test]
    fn test_cli_missing_subcommand() {
        let result = Cli::try_parse_from(["hisiflash"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_cli_invalid_chip() {
        let result = Cli::try_parse_from(["hisiflash", "--chip", "invalid_chip", "list-ports"]);
        assert!(result.is_err());
    }

    // ---- parse_bin_arg ----

    #[test]
    fn test_parse_bin_arg_valid() {
        let (path, addr) = parse_bin_arg("app.bin:0x00800000").unwrap();
        assert_eq!(path.to_str().unwrap(), "app.bin");
        assert_eq!(addr, 0x00800000);
    }

    #[test]
    fn test_parse_bin_arg_no_prefix() {
        let (path, addr) = parse_bin_arg("data.bin:800000").unwrap();
        assert_eq!(path.to_str().unwrap(), "data.bin");
        assert_eq!(addr, 0x00800000);
    }

    #[test]
    fn test_parse_bin_arg_invalid_no_colon() {
        assert!(parse_bin_arg("app.bin").is_err());
    }

    #[test]
    fn test_parse_bin_arg_invalid_too_many_colons() {
        assert!(parse_bin_arg("a:b:c").is_err());
    }

    #[test]
    fn test_parse_bin_arg_invalid_address() {
        assert!(parse_bin_arg("app.bin:ZZZZ").is_err());
    }

    // ---- parse_hex_u32 ----

    #[test]
    fn test_parse_hex_u32_with_prefix() {
        assert_eq!(parse_hex_u32("0x00800000").unwrap(), 0x00800000);
        assert_eq!(parse_hex_u32("0X00800000").unwrap(), 0x00800000);
    }

    #[test]
    fn test_parse_hex_u32_without_prefix() {
        assert_eq!(parse_hex_u32("DEADBEEF").unwrap(), 0xDEADBEEF);
        assert_eq!(parse_hex_u32("ff").unwrap(), 0xFF);
    }

    #[test]
    fn test_parse_hex_u32_with_underscores() {
        assert_eq!(parse_hex_u32("0x00_80_00_00").unwrap(), 0x00800000);
    }

    #[test]
    fn test_parse_hex_u32_with_whitespace() {
        assert_eq!(parse_hex_u32("  0xFF  ").unwrap(), 0xFF);
    }

    #[test]
    fn test_parse_hex_u32_invalid() {
        assert!(parse_hex_u32("not_hex").is_err());
        assert!(parse_hex_u32("0xGG").is_err());
    }

    #[test]
    fn test_parse_hex_u32_overflow() {
        assert!(parse_hex_u32("0x1FFFFFFFF").is_err());
    }

    #[test]
    fn test_parse_hex_u32_zero() {
        assert_eq!(parse_hex_u32("0x0").unwrap(), 0);
        assert_eq!(parse_hex_u32("0").unwrap(), 0);
    }

    // ---- Chip conversion ----

    #[test]
    fn test_chip_to_chip_family() {
        use hisiflash::ChipFamily;
        assert_eq!(ChipFamily::from(Chip::Ws63), ChipFamily::Ws63);
        assert_eq!(ChipFamily::from(Chip::Bs2x), ChipFamily::Bs2x);
        assert_eq!(ChipFamily::from(Chip::Bs25), ChipFamily::Bs25);
    }

    // ---- partition_type_str ----

    #[test]
    fn test_partition_type_str_values() {
        use hisiflash::PartitionType;
        assert_eq!(partition_type_str(PartitionType::Loader), "Loader");
        assert_eq!(partition_type_str(PartitionType::Normal), "Normal");
        assert_eq!(partition_type_str(PartitionType::KvNv), "KV-NV");
        assert_eq!(partition_type_str(PartitionType::Flashboot), "FlashBoot");
        assert_eq!(partition_type_str(PartitionType::Factory), "Factory");
        assert_eq!(partition_type_str(PartitionType::Unknown(99)), "Unknown");
    }

    // ---- build_localized_command ----

    #[test]
    fn test_build_localized_command_creates_valid_command() {
        rust_i18n::set_locale("en");
        let cmd = build_localized_command();
        // Should be valid
        cmd.clone().debug_assert();
        assert_eq!(cmd.get_name(), "hisiflash");
    }

    #[test]
    fn test_build_localized_command_has_subcommands() {
        rust_i18n::set_locale("en");
        let cmd = build_localized_command();
        let subcmd_names: Vec<_> = cmd
            .get_subcommands()
            .map(|s| s.get_name().to_string())
            .collect();
        assert!(subcmd_names.contains(&"flash".to_string()));
        assert!(subcmd_names.contains(&"write".to_string()));
        assert!(subcmd_names.contains(&"erase".to_string()));
        assert!(subcmd_names.contains(&"info".to_string()));
        assert!(subcmd_names.contains(&"list-ports".to_string()));
        assert!(subcmd_names.contains(&"monitor".to_string()));
        assert!(subcmd_names.contains(&"completions".to_string()));
    }

    #[test]
    fn test_build_localized_command_zh_cn() {
        rust_i18n::set_locale("zh-CN");
        let cmd = build_localized_command();
        cmd.clone().debug_assert();
        // About should be Chinese
        let about = cmd
            .get_about()
            .map(std::string::ToString::to_string)
            .unwrap_or_default();
        assert!(
            about.contains("海思"),
            "About should be in Chinese: {about}"
        );
    }

    // ---- localize_arg ----

    #[test]
    fn test_localize_arg_known_key() {
        rust_i18n::set_locale("zh-CN");
        let arg = clap::Arg::new("port").help("original help");
        let localized = localize_arg(arg);
        let help = localized
            .get_help()
            .map(std::string::ToString::to_string)
            .unwrap_or_default();
        // Should be Chinese translation
        assert!(!help.is_empty());
        assert_ne!(help, "original help");
    }

    #[test]
    fn test_localize_arg_unknown_key() {
        rust_i18n::set_locale("en");
        let arg = clap::Arg::new("nonexistent_arg_xyz").help("keep this");
        let localized = localize_arg(arg);
        let help = localized
            .get_help()
            .map(std::string::ToString::to_string)
            .unwrap_or_default();
        // Should keep original since no translation exists
        assert_eq!(help, "keep this");
    }
}
