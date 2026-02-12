# hisiflash (library) Changelog

All notable changes to the `hisiflash` library crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Host discovery APIs:
  - `discover_ports`
  - `discover_hisilicon_ports`
  - `auto_detect_port`
- Native monitor primitives:
  - `MonitorSession`
  - `split_utf8`
  - `drain_utf8_lossy`
  - `clean_monitor_text`
  - `format_monitor_output`

### Changed
- `format_monitor_output` now normalizes standalone `\r`/`\n` line endings more consistently.
- Non-timestamp formatting path now keeps `at_line_start` state aligned with emitted output.
- Monitor text-processing flow was hardened for mixed UTF-8/invalid-byte streams in long-running sessions.

### Compatibility
- Changes are additive with behavior refinements in monitor text formatting/output helpers.

## Historical Notes

- Prior history was tracked in workspace root `CHANGELOG.md` and mixed CLI/lib changes.

## Release Workflow Template

When preparing a library release, use this consistent workflow:

1. Move finished entries from `Unreleased` into a new version section:

  ```markdown
  ## [0.2.1] - YYYY-MM-DD

  ### Added
  - ...
  ```

2. Keep `Unreleased` at the top and empty (or only future items).
3. Ensure `hisiflash/Cargo.toml` version matches the new section.
4. Call out API-compatibility impact explicitly (`Compatibility` section).
5. Add/refresh compare links at the bottom when tags are created.
