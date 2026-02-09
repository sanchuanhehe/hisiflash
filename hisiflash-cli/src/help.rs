//! Localized help output for the CLI.
//!
//! Builds a clap `Command` with fully translated section headings,
//! subcommand descriptions, and argument help text.

use clap::CommandFactory;
use rust_i18n::t;

use crate::Cli;

/// Supported locales for i18n.
pub(crate) const SUPPORTED_LOCALES: &[&str] = &["en", "zh-CN"];

/// Detect the best matching locale from system settings.
///
/// This function tries to match the system locale to one of the supported locales.
/// It handles various locale formats like:
/// - `zh_CN.UTF-8` -> `zh-CN`
/// - `zh-CN` -> `zh-CN`
/// - `zh` -> `zh-CN`
/// - `en_US.UTF-8` -> `en`
/// - `C` or `POSIX` -> `en`
pub(crate) fn detect_locale() -> String {
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

/// Build a clap `Command` with fully localized help output.
///
/// Uses clap as the single source of truth for structure (args, subcommands),
/// while replacing all user-visible text (section headings, command descriptions,
/// argument help) with translations from the locale files.
pub(crate) fn build_localized_command() -> clap::Command {
    // Leak localized heading strings once (CLI runs once, tiny and harmless)
    let args_heading: &'static str =
        Box::leak(t!("help.arguments_heading").to_string().into_boxed_str());
    let opts_heading: &'static str =
        Box::leak(t!("help.options_heading").to_string().into_boxed_str());

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
         {all_args}\n",
        bin = "{bin}",
        version = "{version}",
        about = "{about}",
        usage_h = t!("help.usage_heading"),
        usage = "{usage}",
        all_args = "{all-args}",
    );

    Cli::command()
        .help_template(&tpl)
        .about(t!("app.about").to_string())
        .after_help(t!("app.after_help").to_string())
        .disable_help_flag(true)
        .disable_version_flag(true)
        .arg(
            clap::Arg::new("help")
                .short('h')
                .long("help")
                .help(t!("arg.help_flag.help").to_string())
                .help_heading(opts_heading)
                .action(clap::ArgAction::Help)
                .global(true),
        )
        .arg(
            clap::Arg::new("version")
                .short('V')
                .long("version")
                .help(t!("arg.version_flag.help").to_string())
                .help_heading(opts_heading)
                .action(clap::ArgAction::Version)
                .global(true),
        )
        .mut_args(move |arg| {
            let arg = localize_arg(arg);
            if arg.get_short().is_none() && arg.get_long().is_none() {
                arg.help_heading(args_heading)
            } else {
                arg.help_heading(opts_heading)
            }
        })
        .mut_subcommands(move |sub| {
            let name = sub.get_name().to_string();
            let about_key = format!("cmd.{}.about", name.replace('-', "_"));
            let localized = t!(&about_key).to_string();
            let sub = if localized != about_key {
                sub.about(localized)
            } else {
                sub
            };
            sub.help_template(sub_tpl.clone()).mut_args(move |arg| {
                let arg = localize_arg(arg);
                if arg.get_short().is_none() && arg.get_long().is_none() {
                    arg.help_heading(args_heading)
                } else {
                    arg.help_heading(opts_heading)
                }
            })
        })
        .disable_help_subcommand(true)
        .subcommand(clap::Command::new("help").about(t!("cmd.help.about").to_string()))
}

/// Replace an arg's help text with its localized version if available.
///
/// Looks up `arg.<id>.help` in the current locale. If found, replaces the
/// arg's help text; otherwise keeps the original (English from doc comments).
pub(crate) fn localize_arg(arg: clap::Arg) -> clap::Arg {
    let id = arg.get_id().as_str().to_string();
    let key = format!("arg.{id}.help");
    let localized = t!(&key).to_string();
    if localized != key {
        arg.help(localized)
    } else {
        arg
    }
}
