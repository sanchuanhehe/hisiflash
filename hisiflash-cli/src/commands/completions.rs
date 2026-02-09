//! Shell completion generation and installation.

use anyhow::{Context, Result};
use clap::CommandFactory;
use clap_complete::{Shell, generate};
use console::style;
use std::env;
use std::fs;
use std::io;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use crate::Cli;

/// Generate shell completions to stdout.
pub(crate) fn cmd_completions(shell: Shell) {
    let mut cmd = Cli::command();
    let name = cmd.get_name().to_string();
    generate(shell, &mut cmd, name, &mut io::stdout());
}

/// Detect the user's current shell from environment.
pub(crate) fn detect_shell_type() -> Option<Shell> {
    // Try $SHELL first (Unix)
    if let Ok(shell_path) = env::var("SHELL") {
        let shell_name = Path::new(&shell_path)
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
pub(crate) fn cmd_completions_install(shell_arg: Option<Shell>) -> Result<()> {
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
