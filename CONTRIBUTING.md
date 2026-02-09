# Contributing to hisiflash

Thank you for your interest in contributing to hisiflash! This document provides guidelines and information for contributors.

## Getting Started

### Prerequisites

- Rust 1.85 or later
- Git
- Linux: `libudev-dev` package
- A HiSilicon development board (optional, for hardware testing)

### Setting Up Development Environment

```bash
# Clone the repository
git clone https://github.com/example/hisiflash.git
cd hisiflash

# Build the project
cargo build

# Run tests
cargo test

# Run linter
cargo clippy --all-targets --all-features -- -D warnings

# Format code
cargo fmt --all
```

## Development Workflow

### Branch Naming

- `feature/description` - New features
- `fix/description` - Bug fixes
- `docs/description` - Documentation changes
- `refactor/description` - Code refactoring

### Commit Messages

We follow the [Conventional Commits](https://www.conventionalcommits.org/) specification:

```
<type>(<scope>): <description>

[optional body]

[optional footer(s)]
```

Types:
- `feat`: New feature
- `fix`: Bug fix
- `docs`: Documentation only
- `style`: Code style (formatting, etc.)
- `refactor`: Code refactoring
- `perf`: Performance improvement
- `test`: Adding or updating tests
- `chore`: Maintenance tasks

Examples:
```
feat(protocol): add SEBOOT frame encryption support
fix(serial): handle timeout on Windows correctly
docs: update SEBOOT protocol specification
```

### Pull Request Process

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Ensure all tests pass: `cargo test`
5. Ensure code is formatted: `cargo fmt --all`
6. Ensure no clippy warnings: `cargo clippy --all-targets -- -D warnings`
7. Submit a pull request

### Code Review

All submissions require review. We use GitHub pull requests for this purpose.

## Code Style

### Rust Guidelines

- Follow the [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)
- Use `rustfmt` for formatting (configuration in `rustfmt.toml`)
- Use `clippy` for linting
- Write documentation for all public APIs
- Add tests for new functionality

### Documentation

- Use `///` for public item documentation
- Use `//!` for module-level documentation
- Include examples in doc comments where appropriate
- Keep comments up-to-date with code changes

### Error Handling

- Use `thiserror` for library errors
- Use `anyhow` for CLI errors
- Avoid `unwrap()` and `expect()` in library code
- Provide meaningful error messages

## Testing

### Running Tests

```bash
# Run all tests
cargo test

# Run specific test
cargo test test_handshake

# Run tests with output
cargo test -- --nocapture

# Run tests with logging
RUST_LOG=debug cargo test
```

### Writing Tests

- Place unit tests in the same file as the code
- Place integration tests in `tests/` directory
- Use descriptive test names
- Test both success and error cases

### Hardware Testing

**WARNING:** Hardware tests can modify device firmware. Only use dedicated test hardware.

```bash
# Run with real hardware (requires device)
cargo test --features hardware_test -- --ignored
```

## Architecture

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for detailed architecture documentation.

### Key Modules

| Module | Description |
|--------|-------------|
| `hisiflash::protocol` | Communication protocols (SEBOOT, YMODEM) |
| `hisiflash::connection` | Serial port handling and detection |
| `hisiflash::target` | Chip-specific implementations |
| `hisiflash::image` | Firmware image parsing |

## Adding New Features

### Adding a New Chip

1. Add chip variant to `ChipFamily` in `target/chip.rs`
2. Create chip-specific module in `target/` if needed
3. Update CLI chip selection in `main.rs`
4. Add documentation and tests
5. Update README and AGENTS.md

### Adding a New Command

1. Add command to `Commands` enum in `main.rs`
2. Implement command handler function
3. Add to match statement in `main()`
4. Add help text and documentation
5. Add tests

### Modifying Protocol

1. Update implementation in `protocol/seboot.rs`
2. Update protocol documentation in `docs/protocols/`
3. Add/update tests
4. Consider backward compatibility

## Release Process

The library (`hisiflash`) and CLI (`hisiflash-cli`) have independent version numbers and release cycles.

### Releasing hisiflash-cli (CLI tool)

1. Update version in `hisiflash-cli/Cargo.toml`
2. Update `CHANGELOG.md` with release notes
3. Commit changes: `git commit -am "chore: release hisiflash-cli v1.0.0"`
4. Create a CLI tag: `git tag cli-v1.0.0`
5. Push tag: `git push origin cli-v1.0.0`
6. GitHub Actions will:
   - Build binaries for all platforms (Linux, macOS, Windows)
   - Create a GitHub Release with binaries attached
   - Publish to crates.io (for stable releases)

### Releasing hisiflash (library)

1. Update version in `hisiflash/Cargo.toml`
2. Update `CHANGELOG.md` with release notes
3. Commit changes: `git commit -am "chore: release hisiflash v0.2.0"`
4. Create a library tag: `git tag lib-v0.2.0`
5. Push tag: `git push origin lib-v0.2.0`
6. GitHub Actions will:
   - Run tests
   - Publish to crates.io (for stable releases)
   - Create a GitHub Release

### Tag Format

| Tag Pattern | Target | Example |
|-------------|--------|---------|
| `cli-v*` | hisiflash-cli (binaries + crates.io) | `cli-v1.0.0`, `cli-v1.1.0-beta.1` |
| `lib-v*` | hisiflash library (crates.io only) | `lib-v0.2.0`, `lib-v0.3.0-rc.1` |

Pre-release tags (containing `-alpha`, `-beta`, `-rc` etc.) will create draft releases and skip crates.io publishing.

## Getting Help

- Open an issue for bugs or feature requests
- Start a discussion for questions
- Check existing issues and discussions first

## License

By contributing, you agree that your contributions will be licensed under the MIT OR Apache-2.0 license.
