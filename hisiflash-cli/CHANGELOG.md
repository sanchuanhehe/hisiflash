# hisiflash-cli Changelog

All notable changes to `hisiflash-cli` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html)
with pre-release tags.

## [Unreleased]

### Added
- `monitor` interactive command now supports timestamp display, log file writing, and shortcut keys (`Ctrl+C`, `Ctrl+R`, `Ctrl+T`).
- `flash` supports interactive firmware auto-discovery/selection when firmware path is omitted.
- Project-level testing/validation docs were added and linked in README (`docs/testing/*`).

### Changed
- CLI now reports invalid `flash.chip` values in config explicitly instead of silently falling back.
- Interactive serial selection hints are written to `stderr` for better script compatibility.
- Multi-port serial selection behavior was tightened for non-interactive mode and clearer fallback errors.
- Chip option help text was expanded for better discoverability.
- Terminal-width aware truncation was added for firmware/port labels and prompts to reduce line wrapping.
- Error contract tightened to avoid duplicate error lines for `erase` without `--all`.
- Default runtime logging is quieter (`warn` by default, `info` with `-v`).
- `monitor` now supports clean/raw output modes (`--clean-output` default, `--raw` optional).
- `monitor` output handling was hardened with lossy UTF-8 draining and cleaner line-state management.
- `monitor` Ctrl+R now includes automatic post-reset output verification and flow-control hinting.
- `monitor` status rendering now uses synchronized status-line output to improve alignment under streaming output.
- `monitor` stream contract is now split in non-TTY mode (`stdout` for serial data, `stderr` for status), while TTY mode remains merged for alignment.

### Fixed
- Cancellation now uses a dedicated `Cancelled:` style output instead of generic error wording.

### Compatibility
- User-visible CLI behavior changed (default log verbosity and some error outputs).
- Recommended next release tag: `v1.0.0-alpha.10`.

## Historical Notes

- `v1.0.0` stable has **not** been published yet.
- Prior history was tracked in workspace root `CHANGELOG.md` and mixed CLI/lib changes.

## Release Workflow Template

When preparing a CLI release, use this consistent workflow:

1. Move finished entries from `Unreleased` into a new version section:

	```markdown
	## [1.0.0-alpha.10] - YYYY-MM-DD

	### Changed
	- ...
	```

2. Keep `Unreleased` at the top and empty (or only future items).
3. Ensure `hisiflash-cli/Cargo.toml` version matches the new section.
4. Mark behavior-affecting updates in a `Compatibility` subsection.
5. Add/refresh compare links at the bottom when tags are created.
