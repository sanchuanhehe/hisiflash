//! Build script for hisiflash-cli: auto-configures git hooks.

use std::process::Command;

/// Auto-configure git hooks and other build-time setup.
fn main() {
    // Auto-configure git hooks path on build
    // This ensures every developer gets pre-push checks without manual setup
    if std::path::Path::new("../.githooks").exists() || std::path::Path::new(".githooks").exists() {
        let _ = Command::new("git")
            .args(["config", "core.hooksPath", ".githooks"])
            .status();
    }
}
