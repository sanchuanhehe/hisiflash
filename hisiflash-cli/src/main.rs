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

use anyhow::Result;
use clap::error::ErrorKind;
use clap::parser::ValueSource;
use clap::{CommandFactory, FromArgMatches, Parser, Subcommand, ValueEnum};
use clap_complete::Shell;
use console::style;
use env_logger::Env;
use hisiflash::{ChipFamily, Error as LibError};
use log::debug;
use rust_i18n::t;
use std::env;
use std::path::PathBuf;
use std::sync::OnceLock;
use thiserror::Error;

/// Whether stderr is a terminal (set once at startup).
static STDERR_IS_TTY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
/// Whether process received SIGINT/Ctrl-C.
static INTERRUPTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Ensures Ctrl-C handler is installed only once.
static SIGNAL_HANDLER_INSTALLED: OnceLock<()> = OnceLock::new();

/// Check if emoji/animations should be used (TTY and colors enabled).
pub(crate) fn use_fancy_output() -> bool {
    STDERR_IS_TTY.load(std::sync::atomic::Ordering::Relaxed) && console::colors_enabled_stderr()
}

pub(crate) fn was_interrupted() -> bool {
    INTERRUPTED.load(std::sync::atomic::Ordering::Relaxed)
}

pub(crate) fn clear_interrupted_flag() {
    INTERRUPTED.store(false, std::sync::atomic::Ordering::Relaxed);
}

fn install_signal_handler() -> Result<()> {
    if SIGNAL_HANDLER_INSTALLED.get().is_some() {
        return Ok(());
    }

    ctrlc::set_handler(|| {
        INTERRUPTED.store(true, std::sync::atomic::Ordering::Relaxed);
    })
    .map_err(|e| anyhow::anyhow!("failed to install Ctrl-C handler: {e}"))?;

    let _ = SIGNAL_HANDLER_INSTALLED.set(());
    Ok(())
}

mod commands;
mod config;
mod help;
mod serial;

use commands::completions::{cmd_completions, cmd_completions_install};
use commands::firmware::resolve_firmware;
use commands::flash::{cmd_erase, cmd_flash, cmd_write, cmd_write_program};
use commands::info::{cmd_info, cmd_list_ports};
use commands::monitor::cmd_monitor;
use config::Config;
use help::{build_localized_command, detect_locale};
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
pub(crate) struct Cli {
    /// Serial port to use (auto-detected if not specified).
    #[arg(short, long, global = true, env = "HISIFLASH_PORT")]
    pub(crate) port: Option<String>,

    /// Baud rate for data transfer.
    #[arg(
        short,
        long,
        global = true,
        default_value = "921600",
        env = "HISIFLASH_BAUD"
    )]
    pub(crate) baud: u32,

    /// Target chip type.
    #[arg(
        short,
        long,
        global = true,
        default_value = "ws63",
        env = "HISIFLASH_CHIP"
    )]
    pub(crate) chip: Chip,

    /// Language/locale for messages (e.g., en, zh-CN).
    #[arg(long, global = true, env = "HISIFLASH_LANG")]
    pub(crate) lang: Option<String>,

    /// Verbose output level (-v, -vv, -vvv for increasing detail).
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    pub(crate) verbose: u8,

    /// Quiet mode (suppress non-essential output).
    #[arg(short, long, global = true)]
    pub(crate) quiet: bool,

    /// Non-interactive mode (fail instead of prompting).
    #[arg(long, global = true, env = "HISIFLASH_NON_INTERACTIVE")]
    pub(crate) non_interactive: bool,

    /// Confirm port selection even for auto-detected ports.
    #[arg(long, global = true)]
    pub(crate) confirm_port: bool,

    /// List all available ports (including unknown types).
    #[arg(long, global = true)]
    pub(crate) list_all_ports: bool,

    /// Path to a configuration file.
    #[arg(long = "config", global = true, value_name = "PATH")]
    pub(crate) config_path: Option<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

/// Supported chip types.
#[derive(Clone, Copy, Debug, ValueEnum)]
pub(crate) enum Chip {
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

impl Chip {
    fn from_config_name(name: &str) -> Option<Self> {
        match ChipFamily::from_name(name)? {
            ChipFamily::Ws63 => Some(Self::Ws63),
            ChipFamily::Bs2x => Some(Self::Bs2x),
            ChipFamily::Bs25 => Some(Self::Bs25),
            _ => None,
        }
    }
}

#[derive(Debug, Error)]
pub(crate) enum CliError {
    #[error("{0}")]
    Usage(String),
    #[error("{0}")]
    Config(String),
    #[error("{0}")]
    Cancelled(String),
}

impl CliError {
    fn exit_code(&self) -> i32 {
        match self {
            Self::Usage(_) => 2,
            Self::Config(_) => 3,
            Self::Cancelled(_) => 130,
        }
    }
}

/// Available commands.
#[derive(Subcommand)]
enum Commands {
    /// Flash a FWPKG firmware package.
    Flash {
        /// Path to the FWPKG firmware file (auto-detected if omitted).
        firmware: Option<PathBuf>,

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

        /// Baud rate for serial monitor (used with --monitor).
        #[arg(long, default_value = "115200")]
        monitor_baud: u32,

        /// Clean monitor output by filtering non-printable control characters.
        #[arg(long = "monitor-clean-output", action = clap::ArgAction::Set, default_value_t = true)]
        monitor_clean_output: bool,

        /// Show raw monitor output without control-character filtering.
        #[arg(long = "monitor-raw", conflicts_with = "monitor_clean_output")]
        monitor_raw: bool,
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

        /// Show timestamps on each line.
        #[arg(long)]
        timestamp: bool,

        /// Save output to a log file.
        #[arg(long, value_name = "FILE")]
        log: Option<PathBuf>,

        /// Clean output by filtering non-printable control characters.
        #[arg(long = "clean-output", action = clap::ArgAction::Set, default_value_t = true)]
        clean_output: bool,

        /// Show raw serial output without control-character filtering.
        #[arg(long, conflicts_with = "clean_output")]
        raw: bool,
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
    let Some((path_str, addr_str)) = s.rsplit_once(':') else {
        return Err(format!(
            "Invalid format: '{s}'. Expected 'file:address' (e.g., 'firmware.bin:0x00800000')"
        ));
    };

    if path_str.is_empty() || addr_str.is_empty() {
        return Err(format!(
            "Invalid format: '{s}'. Expected 'file:address' (e.g., 'firmware.bin:0x00800000')"
        ));
    }

    let path = PathBuf::from(path_str);
    let addr = parse_hex_u32(addr_str)?;

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

fn main() {
    match run() {
        Ok(()) => {},
        Err(err) => {
            let code = map_exit_code(&err);
            if code == 130 {
                eprintln!("{} {err}", style("Cancelled:").yellow().bold());
            } else {
                eprintln!("{} {err}", style("Error:").red().bold());
            }
            std::process::exit(code);
        },
    }
}

fn run() -> Result<()> {
    install_signal_handler()?;
    clear_interrupted_flag();

    let raw_args: Vec<String> = env::args().collect();
    let result = run_with_args(&raw_args);

    if was_interrupted() {
        match result {
            Err(err) if err.downcast_ref::<CliError>().is_some() => Err(err),
            _ => Err(CliError::Cancelled(t!("error.interrupted").to_string()).into()),
        }
    } else {
        result
    }
}

fn run_with_args(raw_args: &[String]) -> Result<()> {
    // Inspect raw args early to support localized --help handling and early --lang
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
    let wants_short_help = raw_args.iter().any(|a| a == "-h");
    let wants_long_help = raw_args.iter().any(|a| a == "--help");
    let wants_help = wants_short_help || wants_long_help;
    let has_help_subcmd = raw_args.iter().skip(1).any(|a| a == "help");
    let no_args = raw_args.len() <= 1;

    if wants_help || no_args || has_help_subcmd {
        let mut app = build_localized_command();
        let use_long = wants_long_help || has_help_subcmd;

        // Collect real subcommand names (exclude our synthetic "help" entry)
        let subcmd_names: Vec<String> = app
            .get_subcommands()
            .filter(|s| s.get_name() != "help")
            .map(|s| s.get_name().to_string())
            .collect();
        let found = raw_args
            .iter()
            .skip(1)
            .find(|token| subcmd_names.iter().any(|n| n == token.as_str()));

        if let Some(cmd_name) = found {
            // Build the parent command to propagate global args to subcommands
            app.build();
            if let Some(sub) = app
                .get_subcommands_mut()
                .find(|s| s.get_name() == cmd_name.as_str())
            {
                if use_long {
                    let _ = sub.print_long_help();
                } else {
                    let _ = sub.print_help();
                }
            }
        } else if use_long {
            let _ = app.print_long_help();
        } else {
            let _ = app.print_help();
        }
        return Ok(());
    }

    let app = Cli::command();
    let matches = match app.try_get_matches_from(raw_args.to_owned()) {
        Ok(matches) => matches,
        Err(err) => match err.kind() {
            ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => {
                err.print()?;
                return Ok(());
            },
            _ => return Err(CliError::Usage(err.to_string()).into()),
        },
    };
    let mut cli = Cli::from_arg_matches(&matches).map_err(|e| CliError::Usage(e.to_string()))?;

    // Setup logging based on verbosity
    let log_level = if cli.quiet {
        "error"
    } else {
        match cli.verbose {
            0 => "warn",
            1 => "info",
            2 => "debug",
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

    apply_config_defaults(&mut cli, &matches, &config)?;

    match &cli.command {
        Commands::Flash {
            firmware,
            filter,
            late_baud,
            skip_verify,
            monitor,
            monitor_baud,
            monitor_clean_output,
            monitor_raw,
        } => {
            let firmware = resolve_firmware(firmware.as_ref(), cli.non_interactive, cli.quiet)?;
            cmd_flash(
                &cli,
                &mut config,
                &firmware,
                filter.as_ref(),
                *late_baud,
                *skip_verify,
            )?;
            if *monitor {
                eprintln!();
                cmd_monitor(
                    &cli,
                    &mut config,
                    *monitor_baud,
                    false,
                    *monitor_clean_output && !*monitor_raw,
                    None,
                )?;
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
        Commands::Monitor {
            monitor_baud,
            timestamp,
            log,
            clean_output,
            raw,
        } => {
            cmd_monitor(
                &cli,
                &mut config,
                *monitor_baud,
                *timestamp,
                *clean_output && !*raw,
                log.as_ref(),
            )?;
        },
        Commands::Completions { shell, install } => {
            if *install {
                cmd_completions_install(*shell)?;
            } else {
                let shell = (*shell).ok_or_else(|| {
                    CliError::Usage(
                        "specify a shell type, e.g.: hisiflash completions bash\n  Or use hisiflash completions --install to auto-install completions.".to_string(),
                    )
                })?;
                cmd_completions(shell);
            }
        },
    }

    Ok(())
}

fn apply_config_defaults(cli: &mut Cli, matches: &clap::ArgMatches, config: &Config) -> Result<()> {
    if matches.value_source("baud") == Some(ValueSource::DefaultValue)
        && let Some(config_baud) = config.port.connection.baud
    {
        cli.baud = config_baud;
    }

    if matches.value_source("chip") == Some(ValueSource::DefaultValue)
        && let Some(config_chip_name) = config.flash.chip.as_deref()
    {
        let config_chip = Chip::from_config_name(config_chip_name).ok_or_else(|| {
            CliError::Config(
                t!(
                    "error.invalid_config_chip",
                    chip = config_chip_name,
                    supported = "ws63, bs2x, bs25"
                )
                .to_string(),
            )
        })?;
        cli.chip = config_chip;
    }

    match &mut cli.command {
        Commands::Flash {
            late_baud,
            skip_verify,
            ..
        } => {
            if !matches!(
                matches
                    .subcommand()
                    .and_then(|(_, m)| m.value_source("late_baud")),
                Some(ValueSource::CommandLine)
            ) {
                *late_baud = config.flash.late_baud;
            }
            if !matches!(
                matches
                    .subcommand()
                    .and_then(|(_, m)| m.value_source("skip_verify")),
                Some(ValueSource::CommandLine)
            ) {
                *skip_verify = config.flash.skip_verify;
            }
        },
        Commands::Write { late_baud, .. } | Commands::WriteProgram { late_baud, .. } => {
            if !matches!(
                matches
                    .subcommand()
                    .and_then(|(_, m)| m.value_source("late_baud")),
                Some(ValueSource::CommandLine)
            ) {
                *late_baud = config.flash.late_baud;
            }
        },
        _ => {},
    }

    Ok(())
}

fn map_exit_code(err: &anyhow::Error) -> i32 {
    if let Some(cli_err) = err.downcast_ref::<CliError>() {
        return cli_err.exit_code();
    }

    if let Some(lib_err) = err.downcast_ref::<LibError>() {
        return match lib_err {
            LibError::DeviceNotFound => 4,
            LibError::Config(_) => 3,
            LibError::Unsupported(_) => 5,
            _ => 1,
        };
    }

    1
}

/// Get serial port from CLI args or interactive selection.
pub(crate) fn get_port(cli: &Cli, config: &mut Config) -> Result<String> {
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

#[cfg(test)]
mod locale_tests {
    use crate::help::SUPPORTED_LOCALES;

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
    use crate::commands::info::partition_type_str;
    use crate::help::{build_localized_command, localize_arg};
    use clap::CommandFactory;
    use std::sync::Mutex;

    // ---- use_fancy_output ----

    #[test]
    fn test_use_fancy_output_follows_tty_flag() {
        // When STDERR_IS_TTY is false, use_fancy_output should return false.
        STDERR_IS_TTY.store(false, std::sync::atomic::Ordering::Relaxed);
        assert!(!use_fancy_output());
        // Restore to true for other tests.
        STDERR_IS_TTY.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    // ---- parse_hex_u32 additional edge cases ----

    #[test]
    fn test_cli_parse_flash_without_firmware() {
        // firmware is now optional: `flash` alone should parse successfully
        let cli = Cli::try_parse_from(["hisiflash", "flash"]);
        assert!(cli.is_ok());
        if let Commands::Flash { firmware, .. } = &cli.unwrap().command {
            assert!(firmware.is_none());
        } else {
            panic!("Expected Flash command");
        }
    }

    /// Global lock for `rust_i18n::set_locale` which mutates global state.
    /// Only held during set_locale + command construction; assertions run
    /// lock-free so tests can maximally overlap.
    static LOCALE_LOCK: Mutex<()> = Mutex::new(());

    /// Build a localized command for the given locale.
    /// Holds the lock only during `set_locale` + `build_localized_command`;
    /// the returned `Command` has all i18n strings baked in and is safe
    /// to inspect without the lock.
    fn localized_cmd(locale: &str) -> clap::Command {
        let _lock = LOCALE_LOCK.lock().unwrap();
        rust_i18n::set_locale(locale);
        build_localized_command()
    }

    /// Call `localize_arg` under the locale lock.
    fn localized_arg(locale: &str, arg: clap::Arg) -> clap::Arg {
        let _lock = LOCALE_LOCK.lock().unwrap();
        rust_i18n::set_locale(locale);
        localize_arg(arg)
    }

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
            monitor_baud,
            monitor_clean_output,
            monitor_raw,
        } = cli.command
        {
            assert_eq!(firmware.unwrap().to_str().unwrap(), "fw.fwpkg");
            assert_eq!(filter.as_deref(), Some("app,flashboot"));
            assert!(late_baud);
            assert!(skip_verify);
            assert!(monitor);
            assert_eq!(monitor_baud, 115200);
            assert!(monitor_clean_output);
            assert!(!monitor_raw);
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
        if let Commands::Monitor { monitor_baud, .. } = cli.command {
            assert_eq!(monitor_baud, 9600);
        } else {
            panic!("Expected Monitor command");
        }
    }

    #[test]
    fn test_cli_parse_monitor_default_baud() {
        let cli = Cli::try_parse_from(["hisiflash", "monitor"]).unwrap();
        if let Commands::Monitor {
            monitor_baud,
            clean_output,
            raw,
            ..
        } = cli.command
        {
            assert_eq!(monitor_baud, 115200);
            assert!(clean_output);
            assert!(!raw);
        } else {
            panic!("Expected Monitor command");
        }
    }

    #[test]
    fn test_cli_parse_monitor_raw() {
        let cli = Cli::try_parse_from(["hisiflash", "monitor", "--raw"]).unwrap();
        if let Commands::Monitor {
            clean_output, raw, ..
        } = cli.command
        {
            assert!(raw);
            assert!(clean_output);
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
    fn test_apply_config_defaults_for_flash() {
        let mut config = Config::default();
        config.port.connection.baud = Some(460800);
        config.flash.chip = Some("bs2x".to_string());
        config.flash.late_baud = true;
        config.flash.skip_verify = true;

        let cmd = Cli::command();
        let matches = cmd
            .try_get_matches_from(["hisiflash", "flash", "firmware.fwpkg"])
            .unwrap();
        let mut cli = Cli::from_arg_matches(&matches).unwrap();

        apply_config_defaults(&mut cli, &matches, &config).unwrap();

        assert_eq!(cli.baud, 460800);
        assert!(matches!(cli.chip, Chip::Bs2x));

        if let Commands::Flash {
            late_baud,
            skip_verify,
            ..
        } = cli.command
        {
            assert!(late_baud);
            assert!(skip_verify);
        } else {
            panic!("Expected Flash command");
        }
    }

    #[test]
    fn test_apply_config_does_not_override_explicit_cli_values() {
        let mut config = Config::default();
        config.port.connection.baud = Some(460800);
        config.flash.chip = Some("bs2x".to_string());
        config.flash.late_baud = false;
        config.flash.skip_verify = true;

        let cmd = Cli::command();
        let matches = cmd
            .try_get_matches_from([
                "hisiflash",
                "--baud",
                "115200",
                "--chip",
                "ws63",
                "flash",
                "firmware.fwpkg",
                "--late-baud",
            ])
            .unwrap();
        let mut cli = Cli::from_arg_matches(&matches).unwrap();

        apply_config_defaults(&mut cli, &matches, &config).unwrap();

        assert_eq!(cli.baud, 115200);
        assert!(matches!(cli.chip, Chip::Ws63));

        if let Commands::Flash {
            late_baud,
            skip_verify,
            ..
        } = cli.command
        {
            assert!(late_baud);
            assert!(skip_verify);
        } else {
            panic!("Expected Flash command");
        }
    }

    #[test]
    fn test_run_no_args_returns_ok() {
        let args = vec!["hisiflash".to_string()];
        let result = run_with_args(&args);
        assert!(result.is_ok());
    }

    #[test]
    fn test_run_version_returns_ok() {
        let args = vec!["hisiflash".to_string(), "--version".to_string()];
        let result = run_with_args(&args);
        assert!(result.is_ok());
    }

    #[test]
    fn test_run_short_version_returns_ok() {
        let args = vec!["hisiflash".to_string(), "-V".to_string()];
        let result = run_with_args(&args);
        assert!(result.is_ok());
    }

    #[test]
    fn test_apply_config_defaults_invalid_chip() {
        let mut config = Config::default();
        config.flash.chip = Some("unknown-chip".to_string());

        let cmd = Cli::command();
        let matches = cmd
            .try_get_matches_from(["hisiflash", "list-ports"])
            .unwrap();
        let mut cli = Cli::from_arg_matches(&matches).unwrap();

        let result = apply_config_defaults(&mut cli, &matches, &config);
        assert!(result.is_err());
    }

    #[test]
    fn test_map_exit_code_cancelled_is_130() {
        let err = anyhow::Error::new(CliError::Cancelled("cancelled".to_string()));
        assert_eq!(map_exit_code(&err), 130);
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
    fn test_parse_bin_arg_windows_path() {
        let (path, addr) = parse_bin_arg("C:\\fw\\app.bin:0x00800000").unwrap();
        assert_eq!(path.to_string_lossy(), "C:\\fw\\app.bin");
        assert_eq!(addr, 0x00800000);
    }

    #[test]
    fn test_parse_bin_arg_invalid_missing_address() {
        assert!(parse_bin_arg("C:\\fw\\app.bin:").is_err());
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

    /// Render a command's short help to a String for assertion.
    fn render_help(cmd: &mut clap::Command) -> String {
        cmd.build();
        cmd.render_help().to_string()
    }

    /// Render a command's long help to a String for assertion.
    fn render_long_help(cmd: &mut clap::Command) -> String {
        cmd.build();
        cmd.render_long_help().to_string()
    }

    /// Get a built subcommand by name from the localized command.
    fn get_subcmd(app: &mut clap::Command, name: &str) -> clap::Command {
        app.build();
        app.get_subcommands()
            .find(|s| s.get_name() == name)
            .unwrap()
            .clone()
    }

    #[test]
    fn test_build_localized_command_creates_valid_command() {
        let cmd = localized_cmd("en");
        // Should be valid
        cmd.clone().debug_assert();
        assert_eq!(cmd.get_name(), "hisiflash");
    }

    #[test]
    fn test_build_localized_command_has_subcommands() {
        let cmd = localized_cmd("en");
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
        let cmd = localized_cmd("zh-CN");
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
        let localized = localized_arg("zh-CN", clap::Arg::new("port").help("original help"));
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
        let localized = localized_arg(
            "en",
            clap::Arg::new("nonexistent_arg_xyz").help("keep this"),
        );
        let help = localized
            .get_help()
            .map(std::string::ToString::to_string)
            .unwrap_or_default();
        // Should keep original since no translation exists
        assert_eq!(help, "keep this");
    }

    // ---- Regression tests for help localization (all paths) ----

    #[test]
    fn test_main_help_zh_cn_has_localized_headings() {
        let mut app = localized_cmd("zh-CN");
        let help = render_help(&mut app);
        assert!(help.contains("用法:"), "Missing '用法:' heading:\n{help}");
        assert!(help.contains("命令:"), "Missing '命令:' heading:\n{help}");
        assert!(help.contains("选项:"), "Missing '选项:' heading:\n{help}");
    }

    #[test]
    fn test_main_help_en_has_english_headings() {
        let mut app = localized_cmd("en");
        let help = render_help(&mut app);
        assert!(help.contains("USAGE:"), "Missing 'USAGE:' heading:\n{help}");
        assert!(
            help.contains("COMMANDS:"),
            "Missing 'COMMANDS:' heading:\n{help}"
        );
        assert!(
            help.contains("OPTIONS:"),
            "Missing 'OPTIONS:' heading:\n{help}"
        );
    }

    #[test]
    fn test_main_help_zh_cn_command_descriptions() {
        let mut app = localized_cmd("zh-CN");
        let help = render_help(&mut app);
        assert!(
            help.contains("烧录 FWPKG 固件包"),
            "flash description not localized:\n{help}"
        );
        assert!(
            help.contains("列出可用串口"),
            "list-ports description not localized:\n{help}"
        );
        assert!(
            help.contains("打印帮助信息或指定子命令的帮助"),
            "help description not localized:\n{help}"
        );
    }

    #[test]
    fn test_main_help_zh_cn_option_descriptions() {
        let mut app = localized_cmd("zh-CN");
        let help = render_help(&mut app);
        assert!(
            help.contains("使用的串口"),
            "--port help not localized:\n{help}"
        );
        assert!(
            help.contains("数据传输波特率"),
            "--baud help not localized:\n{help}"
        );
        assert!(
            help.contains("打印帮助信息"),
            "--help help not localized:\n{help}"
        );
        assert!(
            help.contains("打印版本信息"),
            "--version help not localized:\n{help}"
        );
    }

    #[test]
    fn test_main_help_no_english_leaks_zh_cn() {
        let mut app = localized_cmd("zh-CN");
        let help = render_help(&mut app);
        // These English strings should NOT appear in zh-CN output
        assert!(
            !help.contains("Print help"),
            "English 'Print help' leaked into zh-CN:\n{help}"
        );
        assert!(
            !help.contains("Print version"),
            "English 'Print version' leaked into zh-CN:\n{help}"
        );
        assert!(
            !help.contains("USAGE:"),
            "English 'USAGE:' leaked into zh-CN:\n{help}"
        );
        assert!(
            !help.contains("Commands:"),
            "English 'Commands:' leaked into zh-CN:\n{help}"
        );
        assert!(
            !help.contains("Options:"),
            "English 'Options:' heading leaked into zh-CN:\n{help}"
        );
    }

    #[test]
    fn test_subcmd_help_flash_zh_cn_has_localized_content() {
        let mut app = localized_cmd("zh-CN");
        let mut sub = get_subcmd(&mut app, "flash");
        let help = render_help(&mut sub);
        assert!(
            help.contains("烧录 FWPKG 固件包"),
            "flash about not localized:\n{help}"
        );
        assert!(
            help.contains("FWPKG 固件文件路径"),
            "firmware arg not localized:\n{help}"
        );
        assert!(
            help.contains("仅烧录指定分区"),
            "filter arg not localized:\n{help}"
        );
    }

    #[test]
    fn test_subcmd_help_flash_zh_cn_has_localized_headings() {
        let mut app = localized_cmd("zh-CN");
        let mut sub = get_subcmd(&mut app, "flash");
        let help = render_help(&mut sub);
        assert!(
            help.contains("用法:"),
            "Missing '用法:' in flash help:\n{help}"
        );
        assert!(
            help.contains("参数:"),
            "Missing '参数:' in flash help:\n{help}"
        );
        assert!(
            help.contains("选项:"),
            "Missing '选项:' in flash help:\n{help}"
        );
    }

    #[test]
    fn test_subcmd_help_flash_en_has_english_headings() {
        let mut app = localized_cmd("en");
        let mut sub = get_subcmd(&mut app, "flash");
        let help = render_long_help(&mut sub);
        assert!(
            help.contains("USAGE:"),
            "Missing 'USAGE:' in flash help:\n{help}"
        );
        assert!(
            help.contains("ARGUMENTS:"),
            "Missing 'ARGUMENTS:' in flash help:\n{help}"
        );
        assert!(
            help.contains("OPTIONS:"),
            "Missing 'OPTIONS:' in flash help:\n{help}"
        );
    }

    #[test]
    fn test_subcmd_help_flash_global_args_propagated() {
        let mut app = localized_cmd("zh-CN");
        let mut sub = get_subcmd(&mut app, "flash");
        let help = render_help(&mut sub);
        // Global options should appear in subcommand help
        assert!(
            help.contains("--port"),
            "Global --port missing from flash help:\n{help}"
        );
        assert!(
            help.contains("--baud"),
            "Global --baud missing from flash help:\n{help}"
        );
        assert!(
            help.contains("--chip"),
            "Global --chip missing from flash help:\n{help}"
        );
        assert!(
            help.contains("-h, --help"),
            "Global -h/--help missing from flash help:\n{help}"
        );
        assert!(
            help.contains("-V, --version"),
            "Global -V/--version missing from flash help:\n{help}"
        );
    }

    #[test]
    fn test_subcmd_help_flash_no_english_leaks_zh_cn() {
        let mut app = localized_cmd("zh-CN");
        let mut sub = get_subcmd(&mut app, "flash");
        let help = render_long_help(&mut sub);
        assert!(
            !help.contains("Print help"),
            "English 'Print help' leaked into flash zh-CN:\n{help}"
        );
        assert!(
            !help.contains("Print version"),
            "English 'Print version' leaked into flash zh-CN:\n{help}"
        );
        assert!(
            !help.contains("Options:"),
            "English 'Options:' heading leaked into flash zh-CN:\n{help}"
        );
        assert!(
            !help.contains("Arguments:"),
            "English 'Arguments:' heading leaked into flash zh-CN:\n{help}"
        );
    }

    #[test]
    fn test_subcmd_erase_no_arguments_heading() {
        // Erase has no positional args — should not have an "参数:" section
        let mut app = localized_cmd("zh-CN");
        let mut sub = get_subcmd(&mut app, "erase");
        let help = render_help(&mut sub);
        assert!(
            !help.contains("参数:\n"),
            "Erase should not have '参数:' heading with no positionals:\n{help}"
        );
    }

    #[test]
    fn test_subcmd_write_zh_cn_localized() {
        let mut app = localized_cmd("zh-CN");
        let mut sub = get_subcmd(&mut app, "write");
        let help = render_help(&mut sub);
        assert!(
            help.contains("将原始二进制文件写入 Flash"),
            "write about not localized:\n{help}"
        );
    }

    #[test]
    fn test_subcmd_completions_zh_cn_localized() {
        let mut app = localized_cmd("zh-CN");
        let mut sub = get_subcmd(&mut app, "completions");
        let help = render_help(&mut sub);
        assert!(
            help.contains("生成 Shell 补全脚本"),
            "completions about not localized:\n{help}"
        );
        assert!(
            help.contains("自动安装补全脚本"),
            "install arg not localized:\n{help}"
        );
    }

    #[test]
    fn test_long_help_has_more_detail_than_short() {
        let mut app = localized_cmd("zh-CN");
        let short = render_help(&mut app);
        let mut app2 = localized_cmd("zh-CN");
        let long = render_long_help(&mut app2);
        assert!(
            long.len() > short.len(),
            "Long help should be longer than short help"
        );
    }

    #[test]
    fn test_subcmd_long_help_has_more_detail_than_short() {
        let mut app = localized_cmd("zh-CN");
        let mut sub_short = get_subcmd(&mut app, "flash");
        let short = render_help(&mut sub_short);
        let mut app2 = localized_cmd("zh-CN");
        let mut sub_long = get_subcmd(&mut app2, "flash");
        let long = render_long_help(&mut sub_long);
        assert!(
            long.len() > short.len(),
            "Flash long help should be longer than short help"
        );
    }

    #[test]
    fn test_all_subcommands_have_localized_about_zh_cn() {
        let mut app = localized_cmd("zh-CN");
        app.build();
        let expected = [
            ("flash", "烧录"),
            ("write", "写入"),
            ("write-program", "写入"),
            ("erase", "擦除"),
            ("info", "显示"),
            ("list-ports", "列出"),
            ("monitor", "监视器"),
            ("completions", "补全"),
            ("help", "帮助"),
        ];
        for (name, keyword) in expected {
            let sub = app.get_subcommands().find(|s| s.get_name() == name);
            assert!(sub.is_some(), "Subcommand '{name}' not found");
            let about = sub
                .unwrap()
                .get_about()
                .map(std::string::ToString::to_string)
                .unwrap_or_default();
            assert!(
                about.contains(keyword),
                "Subcommand '{name}' about should contain '{keyword}', got: '{about}'"
            );
        }
    }
}
