# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.0.0] - 2026-02-09

### Added
- WS63 chip support with SEBOOT protocol — full flash/write/erase workflow
- FWPKG firmware package parsing (V1/V2 format) and flashing
- USB VID/PID based serial port auto-detection (CH340, CP210x, FTDI, PL2303, HiSilicon)
- YMODEM-1K file transfer protocol
- CLI commands: `flash`, `write`, `write-program`, `erase`, `info`, `list-ports`, `monitor`, `completions`
- `flash --monitor` — automatically open serial monitor after flashing
- Interactive serial port selection with device auto-detection and memory
- TOML configuration file support (local `hisiflash.toml` + global `~/.config/hisiflash/`)
- Environment variable configuration (`HISIFLASH_PORT`, `HISIFLASH_BAUD`, `HISIFLASH_CHIP`, etc.)
- `--json` output for `info` and `list-ports` commands
- `-q/--quiet` silent mode, `-v/-vv/-vvv` verbose levels
- `--non-interactive` mode for CI/CD environments
- Shell completions for Bash, Zsh, Fish, PowerShell, Elvish
- Full internationalization (English + 简体中文) via `--lang` / system locale
- Colorful progress bar display during flashing
- Cross-platform support (Linux, macOS, Windows)
- Comprehensive test suite (200+ tests across library and CLI)
- Multi-chip support framework (BS2X, BS25 planned for future releases)

### Documentation
- SEBOOT protocol specification
- Architecture documentation
- Contributing guidelines
- Agent instructions (AGENTS.md)
- CLI help fully localized (en + zh-CN)

## [0.1.0] - 2026-01-27

### Added
- Initial implementation
- WS63 flashing support
- FWPKG parsing
- Serial port communication
- YMODEM protocol
- Basic CLI interface

[Unreleased]: https://github.com/sanchuanhehe/hisiflash/compare/v1.0.0...HEAD
[1.0.0]: https://github.com/sanchuanhehe/hisiflash/compare/v0.1.0...v1.0.0
[0.1.0]: https://github.com/sanchuanhehe/hisiflash/releases/tag/v0.1.0
