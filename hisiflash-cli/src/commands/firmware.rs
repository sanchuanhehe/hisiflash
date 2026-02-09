//! Auto-discovery and interactive selection of FWPKG firmware files.
//!
//! When the user omits the firmware path from the `flash` command, this module
//! searches the current directory tree for `.fwpkg` files and presents
//! an interactive selection if multiple candidates are found.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Context, Result};
use console::style;
use dialoguer::{Select, theme::ColorfulTheme};
use rust_i18n::t;

use crate::use_fancy_output;

/// Maximum directory depth when searching for firmware files.
const MAX_SEARCH_DEPTH: usize = 5;

/// Directories to skip during search.
const SKIP_DIRS: &[&str] = &[".git", "target", "node_modules", ".svn", ".hg"];

/// Priority directory prefixes (earlier = higher priority).
const PRIORITY_DIRS: &[&str] = &["output", "build", "out", "bin", "release", "firmware"];

/// A discovered firmware file candidate.
#[derive(Debug, Clone)]
pub struct FirmwareCandidate {
    /// Full path to the firmware file.
    pub path: PathBuf,
    /// File size in bytes.
    pub size: u64,
    /// Last modification time.
    pub modified: Option<SystemTime>,
    /// Priority score (lower = better).
    pub priority: u32,
}

impl FirmwareCandidate {
    /// Format file size in a human-readable way.
    pub fn human_size(&self) -> String {
        const KB: u64 = 1024;
        const MB: u64 = 1024 * 1024;
        #[allow(clippy::cast_precision_loss)]
        if self.size >= MB {
            format!("{:.1} MB", self.size as f64 / MB as f64)
        } else if self.size >= KB {
            format!("{:.1} KB", self.size as f64 / KB as f64)
        } else {
            format!("{} B", self.size)
        }
    }

    /// Format the display label for interactive selection.
    pub fn display_label(&self, base: &Path) -> String {
        let rel = self.path.strip_prefix(base).unwrap_or(&self.path).display();
        format!("{rel} ({})", self.human_size())
    }
}

/// Search for `.fwpkg` files under `base_dir` up to `MAX_SEARCH_DEPTH`.
pub fn find_firmware_files(base_dir: &Path) -> Vec<FirmwareCandidate> {
    let mut candidates = Vec::new();
    walk_dir(base_dir, base_dir, 0, &mut candidates);

    // Sort: lower priority first, then newest first, then shorter path first.
    candidates.sort_by(|a, b| {
        a.priority
            .cmp(&b.priority)
            .then_with(|| {
                // Newest first (reverse order of SystemTime).
                match (&b.modified, &a.modified) {
                    (Some(bm), Some(am)) => bm.cmp(am),
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    (None, None) => std::cmp::Ordering::Equal,
                }
            })
            .then_with(|| {
                // Shorter paths first (closer to root).
                a.path
                    .components()
                    .count()
                    .cmp(&b.path.components().count())
            })
    });

    candidates
}

/// Recursively walk directories looking for `.fwpkg` files.
fn walk_dir(base: &Path, dir: &Path, depth: usize, out: &mut Vec<FirmwareCandidate>) {
    if depth > MAX_SEARCH_DEPTH {
        return;
    }

    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // Skip hidden and uninteresting directories.
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.starts_with('.') && name_str != "." {
                continue;
            }
            if SKIP_DIRS.contains(&name_str.as_ref()) {
                continue;
            }
            walk_dir(base, &path, depth + 1, out);
        } else if path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("fwpkg"))
        {
            let meta = entry.metadata().ok();
            let size = meta.as_ref().map_or(0, std::fs::Metadata::len);
            let modified = meta.and_then(|m| m.modified().ok());
            let priority = compute_priority(&path, base);
            out.push(FirmwareCandidate {
                path,
                size,
                modified,
                priority,
            });
        }
    }
}

/// Compute a priority score for a firmware path.
/// Lower score = higher priority.
#[allow(clippy::cast_possible_truncation)] // PRIORITY_DIRS and path depth will never exceed u32
fn compute_priority(path: &Path, base: &Path) -> u32 {
    let rel = path.strip_prefix(base).unwrap_or(path);
    let components: Vec<_> = rel
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect();

    // Check if any path component matches a priority directory.
    for (i, dir_name) in PRIORITY_DIRS.iter().enumerate() {
        for comp in &components {
            if comp.eq_ignore_ascii_case(dir_name) {
                return i as u32;
            }
        }
    }

    // Paths containing "fwpkg" directory get slightly higher priority.
    for comp in &components {
        if comp.eq_ignore_ascii_case("fwpkg") {
            return PRIORITY_DIRS.len() as u32;
        }
    }

    // Default priority: path depth + base offset.
    PRIORITY_DIRS.len() as u32 + components.len() as u32
}

/// Resolve firmware path: if provided use directly, otherwise auto-discover.
///
/// Returns the resolved firmware `PathBuf`.
///
/// # Errors
///
/// Returns error when:
/// - No firmware specified and none found in the directory tree
/// - Non-interactive mode and multiple candidates found
/// - User cancels interactive selection
pub fn resolve_firmware(
    firmware: Option<&PathBuf>,
    non_interactive: bool,
    quiet: bool,
) -> Result<PathBuf> {
    // If explicitly provided, just return it.
    if let Some(path) = firmware {
        return Ok(path.clone());
    }

    // Auto-discover firmware files.
    let base = std::env::current_dir().context("failed to get current directory")?;
    let candidates = find_firmware_files(&base);

    if candidates.is_empty() {
        anyhow::bail!("{}", t!("flash.no_firmware_found"));
    }

    if candidates.len() == 1 {
        let chosen = &candidates[0];
        let rel = chosen
            .path
            .strip_prefix(&base)
            .unwrap_or(&chosen.path)
            .display()
            .to_string();

        if !quiet {
            eprintln!(
                "{} {}",
                style("📦").cyan(),
                t!(
                    "flash.auto_found_one",
                    path = &rel,
                    size = chosen.human_size()
                )
            );
        }

        // In non-interactive mode, use directly without confirmation.
        if non_interactive {
            return Ok(chosen.path.clone());
        }

        // Ask for confirmation.
        let confirm = dialoguer::Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt(t!("flash.confirm_firmware", path = &rel).to_string())
            .default(true)
            .interact()
            .context("firmware confirmation failed")?;

        if confirm {
            return Ok(chosen.path.clone());
        }
        anyhow::bail!("{}", t!("flash.selection_cancelled"));
    }

    // Multiple candidates.
    if non_interactive {
        // Print what we found and bail.
        for c in &candidates {
            let rel = c.path.strip_prefix(&base).unwrap_or(&c.path).display();
            eprintln!("  {rel} ({})", c.human_size());
        }
        anyhow::bail!("{}", t!("flash.multiple_firmware_non_interactive"));
    }

    if !quiet {
        eprintln!(
            "{} {}",
            style("🔍").cyan(),
            t!("flash.auto_found_multiple", count = candidates.len())
        );
    }

    // Build selection items.
    let labels: Vec<String> = candidates.iter().map(|c| c.display_label(&base)).collect();

    let selection = if use_fancy_output() {
        Select::with_theme(&ColorfulTheme::default())
            .with_prompt(t!("flash.select_firmware").to_string())
            .items(&labels)
            .default(0)
            .interact_opt()
            .context("firmware selection failed")?
    } else {
        Select::new()
            .with_prompt(t!("flash.select_firmware").to_string())
            .items(&labels)
            .default(0)
            .interact_opt()
            .context("firmware selection failed")?
    };

    match selection {
        Some(idx) => Ok(candidates[idx].path.clone()),
        None => anyhow::bail!("{}", t!("flash.selection_cancelled")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Helper to create a temporary directory structure with firmware files.
    fn create_test_tree(dir: &Path, files: &[&str]) {
        for file in files {
            let path = dir.join(file);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(&path, vec![0u8; 1024]).unwrap();
        }
    }

    #[test]
    fn test_find_no_firmware() {
        let tmp = tempfile::tempdir().unwrap();
        let result = find_firmware_files(tmp.path());
        assert!(result.is_empty());
    }

    #[test]
    fn test_find_single_firmware() {
        let tmp = tempfile::tempdir().unwrap();
        create_test_tree(tmp.path(), &["app.fwpkg"]);
        let result = find_firmware_files(tmp.path());
        assert_eq!(result.len(), 1);
        assert!(result[0].path.ends_with("app.fwpkg"));
    }

    #[test]
    fn test_find_multiple_firmware() {
        let tmp = tempfile::tempdir().unwrap();
        create_test_tree(
            tmp.path(),
            &["a.fwpkg", "sub/b.fwpkg", "deep/nested/c.fwpkg"],
        );
        let result = find_firmware_files(tmp.path());
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn test_priority_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        create_test_tree(
            tmp.path(),
            &["random/app.fwpkg", "output/release/app.fwpkg"],
        );
        let result = find_firmware_files(tmp.path());
        assert_eq!(result.len(), 2);
        // "output" dir should have higher priority (lower score).
        assert!(result[0].path.to_string_lossy().contains("output"));
    }

    #[test]
    fn test_skip_git_and_target() {
        let tmp = tempfile::tempdir().unwrap();
        create_test_tree(
            tmp.path(),
            &[
                "app.fwpkg",
                ".git/objects/fake.fwpkg",
                "target/debug/build.fwpkg",
            ],
        );
        let result = find_firmware_files(tmp.path());
        assert_eq!(result.len(), 1);
        assert!(result[0].path.ends_with("app.fwpkg"));
    }

    #[test]
    fn test_case_insensitive_extension() {
        let tmp = tempfile::tempdir().unwrap();
        create_test_tree(tmp.path(), &["app.FWPKG", "app2.FwPkg"]);
        let result = find_firmware_files(tmp.path());
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_max_depth_exceeded() {
        let tmp = tempfile::tempdir().unwrap();
        // Create file at depth MAX_SEARCH_DEPTH + 2
        let deep = "a/b/c/d/e/f/g/deep.fwpkg";
        create_test_tree(tmp.path(), &[deep]);
        let result = find_firmware_files(tmp.path());
        // Depth 7 exceeds MAX_SEARCH_DEPTH (5), should not be found.
        assert!(result.is_empty());
    }

    #[test]
    fn test_human_size() {
        let c = FirmwareCandidate {
            path: PathBuf::from("test.fwpkg"),
            size: 512,
            modified: None,
            priority: 0,
        };
        assert_eq!(c.human_size(), "512 B");

        let c = FirmwareCandidate {
            path: PathBuf::from("test.fwpkg"),
            size: 2048,
            modified: None,
            priority: 0,
        };
        assert_eq!(c.human_size(), "2.0 KB");

        let c = FirmwareCandidate {
            path: PathBuf::from("test.fwpkg"),
            size: 3 * 1024 * 1024,
            modified: None,
            priority: 0,
        };
        assert_eq!(c.human_size(), "3.0 MB");
    }

    #[test]
    fn test_resolve_with_explicit_path() {
        let p = PathBuf::from("/some/firmware.fwpkg");
        let result = resolve_firmware(Some(&p), false, false).unwrap();
        assert_eq!(result, p);
    }

    #[test]
    fn test_resolve_no_firmware_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        // Change to the temp dir for the test.
        let _guard = TempCwdGuard::new(tmp.path());
        let result = resolve_firmware(None, true, true);
        assert!(result.is_err());
    }

    #[test]
    fn test_resolve_single_non_interactive() {
        let tmp = tempfile::tempdir().unwrap();
        create_test_tree(tmp.path(), &["my_firmware.fwpkg"]);
        let _guard = TempCwdGuard::new(tmp.path());
        let result = resolve_firmware(None, true, true).unwrap();
        assert!(result.ends_with("my_firmware.fwpkg"));
    }

    #[test]
    fn test_resolve_multiple_non_interactive_fails() {
        let tmp = tempfile::tempdir().unwrap();
        create_test_tree(tmp.path(), &["a.fwpkg", "b.fwpkg"]);
        let _guard = TempCwdGuard::new(tmp.path());
        let result = resolve_firmware(None, true, true);
        assert!(result.is_err());
    }

    /// RAII guard that temporarily changes the working directory.
    struct TempCwdGuard {
        original: PathBuf,
    }

    impl TempCwdGuard {
        fn new(path: &Path) -> Self {
            let original = std::env::current_dir().unwrap();
            std::env::set_current_dir(path).unwrap();
            Self { original }
        }
    }

    impl Drop for TempCwdGuard {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.original);
        }
    }
}
