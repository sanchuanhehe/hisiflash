# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Initial release of hisiflash
- WS63 chip support with SEBOOT protocol
- FWPKG firmware package parsing and flashing
- USB VID/PID based serial port auto-detection
- Support for CH340, CP210x, FTDI, and HiSilicon USB devices
- YMODEM-1K file transfer protocol
- CLI commands: `flash`, `write`, `write-program`, `erase`, `info`, `list-ports`
- Multi-chip support framework (WS63, BS2X, BS25)
- Progress bar display during flashing
- Environment variable configuration
- Cross-platform support (Linux, macOS, Windows)

### Documentation
- SEBOOT protocol specification
- Architecture documentation
- Contributing guidelines
- Agent instructions (AGENTS.md)

## [0.1.0] - 2026-01-27

### Added
- Initial implementation
- WS63 flashing support
- FWPKG parsing
- Serial port communication
- YMODEM protocol
- Basic CLI interface

[Unreleased]: https://github.com/example/hisiflash/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/example/hisiflash/releases/tag/v0.1.0
