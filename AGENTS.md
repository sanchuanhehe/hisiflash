# Copilot Instructions for hisiflash

This document provides essential information for coding agents working on the hisiflash repository.

## Repository Overview

**What hisiflash does:** hisiflash is a Rust-based, cross-platform tool for flashing HiSilicon chips (WS63, BS2X series). Inspired by [espflash](https://github.com/esp-rs/espflash) and [esptool](https://github.com/espressif/esptool), it provides:
- `hisiflash` CLI tool for flashing, erasing, and managing HiSilicon devices
- `hisiflash` library crate for programmatic access

**Project type:** Rust workspace with two crates:
- `hisiflash/` - Core library crate
- `hisiflash-cli/` - CLI application crate

**Supported chips:** WS63 (WiFi+BLE), BS2X series (BS21, BS25 - BLE only), and other HiSilicon chips using the SEBOOT protocol.

## Build and Development Setup

### Essential Commands

1. **Build the project:**
   ```bash
   cargo build --release
   ```

2. **Run tests:**
   ```bash
   cargo test
   ```

3. **Run linter:**
   ```bash
   cargo clippy --all-targets --all-features -- -D warnings
   ```

4. **Format code:**
   ```bash
   cargo fmt --all
   ```

5. **Check without building:**
   ```bash
   cargo check --all-targets
   ```

### Running the CLI

```bash
# List available serial ports
./target/release/hisiflash list-ports

# Flash firmware
./target/release/hisiflash flash -p /dev/ttyUSB0 firmware.fwpkg

# Show help
./target/release/hisiflash --help
```

## Project Architecture

### Directory Structure

```
hisiflash/
├── Cargo.toml                    # Workspace configuration
├── README.md                     # Project documentation
├── AGENTS.md                     # This file (for AI agents)
├── docs/                         # Documentation
│   ├── ARCHITECTURE.md           # Architecture design
│   ├── REQUIREMENTS.md           # Requirements spec
│   └── protocols/                # Protocol documentation
│       ├── SEBOOT_PROTOCOL.md    # SEBOOT protocol spec
│       └── WS63_PROTOCOL.md      # WS63 specifics
│
├── hisiflash/                    # Core library crate
│   └── src/
│       ├── lib.rs                # Library entry point
│       ├── error.rs              # Error definitions
│       ├── connection/           # Serial port handling
│       │   ├── serial.rs         # Serial port abstraction
│       │   └── detect.rs         # USB VID/PID auto-detection
│       ├── protocol/             # Communication protocols
│       │   ├── seboot.rs         # HiSilicon SEBOOT protocol
│       │   ├── ymodem.rs         # YMODEM file transfer
│       │   └── crc.rs            # CRC16-XMODEM
│       ├── target/               # Chip-specific implementations
│       │   ├── chip.rs           # Chip family abstraction
│       │   └── ws63/             # WS63 implementation
│       └── image/                # Firmware image handling
│           └── fwpkg.rs          # FWPKG format parser
│
└── hisiflash-cli/                # CLI application crate
    └── src/
        ├── main.rs               # CLI entry point
        └── commands/             # Subcommand implementations
```

### Key Modules

| Module | Purpose |
|--------|---------|
| `protocol::seboot` | Official HiSilicon SEBOOT protocol (0xDEADBEEF frames) |
| `protocol::ymodem` | YMODEM-1K file transfer protocol |
| `connection::detect` | USB VID/PID based port auto-detection |
| `target::chip` | Chip family abstraction (WS63, BS2X, etc.) |
| `image::fwpkg` | FWPKG firmware package parser |

### SEBOOT Protocol Overview

All commands use the same frame format:
```
+------------+--------+------+-------+---------------+--------+
|   Magic    | Length | Type | ~Type |     Data      | CRC16  |
+------------+--------+------+-------+---------------+--------+
| 0xDEADBEEF | 2 bytes| 1    | 1     |   variable    | 2 bytes|
+------------+--------+------+-------+---------------+--------+
```

Key command types:
- `0xF0` - Handshake
- `0xD2` - Download Flash Image
- `0x87` - Reset
- `0x4B` - Download NV
- `0xE1` - ACK (response)

## Testing

**Run all tests:**
```bash
cargo test
```

**Run specific test:**
```bash
cargo test test_handshake_frame
```

**Run tests with output:**
```bash
cargo test -- --nocapture
```

## Code Style

- **Formatter:** rustfmt (default settings)
- **Linter:** clippy with `-D warnings`
- **Documentation:** All public APIs must have doc comments

### Naming Conventions

- Snake_case for functions and variables
- PascalCase for types and traits
- SCREAMING_SNAKE_CASE for constants
- Prefix private modules with underscore if needed

## Common Tasks

### Adding a New Chip

1. Add variant to `ChipFamily` enum in `target/chip.rs`
2. Implement chip-specific logic if needed
3. Add CLI option in `hisiflash-cli/src/main.rs`
4. Update documentation

### Adding a New Command

1. Add command to `Commands` enum in `main.rs`
2. Implement `cmd_<name>()` function
3. Wire up in `main()` match statement
4. Add tests if applicable

### Modifying Protocol

1. Update `protocol/seboot.rs` for frame changes
2. Update `docs/protocols/SEBOOT_PROTOCOL.md`
3. Add/update tests in the same file

## Dependencies

**Runtime dependencies:**
- `serialport` - Serial port communication
- `byteorder` - Byte order handling
- `thiserror` - Error definitions
- `log` - Logging facade

**CLI dependencies:**
- `clap` - Command line parsing
- `indicatif` - Progress bars
- `console` - Terminal styling
- `anyhow` - Error handling

## Environment Variables

| Variable | Description |
|----------|-------------|
| `HISIFLASH_PORT` | Default serial port |
| `HISIFLASH_BAUD` | Default baud rate |
| `HISIFLASH_CHIP` | Default chip type |
| `RUST_LOG` | Logging level (debug, info, warn, error) |

## Debugging Tips

1. **Enable debug logging:**
   ```bash
   RUST_LOG=debug ./target/release/hisiflash list-ports
   ```

2. **Check serial port permissions:**
   ```bash
   sudo usermod -a -G dialout $USER
   # Then log out and back in
   ```

3. **Monitor serial traffic:**
   ```bash
   # Use another terminal to monitor
   cat /dev/ttyUSB0 | hexdump -C
   ```

## Reference Projects

- [espflash](https://github.com/esp-rs/espflash) - Rust ESP32 flashing tool (architecture reference)
- [esptool](https://github.com/espressif/esptool) - Python ESP flashing tool (protocol reference)
- [fbb_burntool](https://github.com/example/fbb_burntool) - Official HiSilicon burning tool (SEBOOT protocol source)

## Trust These Instructions

These instructions are validated against the current codebase. When in doubt:
1. Run `cargo check` to verify compilation
2. Run `cargo test` to verify functionality
3. Run `cargo clippy` to check code quality
