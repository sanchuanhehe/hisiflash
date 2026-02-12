# hisiflash-cli Changelog

All notable changes to `hisiflash-cli` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html)
with pre-release tags.

## [Unreleased]

### Changed
- CLI now reports invalid `flash.chip` values in config explicitly instead of silently falling back.
- Interactive serial selection hints are written to `stderr` for better script compatibility.
- Error contract tightened to avoid duplicate error lines for `erase` without `--all`.
- Default runtime logging is quieter (`warn` by default, `info` with `-v`).

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
