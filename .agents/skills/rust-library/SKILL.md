---
name: rust-library
description: Define, implement, and review Rust library design. Use when designing library crates, modules, traits, error handling, public APIs, feature flags, and cross-platform abstractions.
compatibility: Generic skill for Rust library projects; applies to embedded/flash tooling, CLI libraries, protocol implementations, and system-level libraries.
metadata:
  author: sanchuanhehe
  version: "1.0"
---

# Rust Library Design Skill

## Purpose

Use this skill to design and maintain high-quality Rust libraries.

Primary references:
- Rust API Guidelines (idiomatic Rust patterns)
- Rust Design Patterns (official patterns catalog)
- Embedded/Rust best practices (no_std, embedded systems)
- External References (compiled resources in references/ directory)

## When to activate

Activate when the task includes any of the following:
- Design or change library public API
- Add or modify modules, traits, or error types
- Add or change feature flags
- Add or change Cargo.toml dependencies
- Design protocol implementations (binary protocols, serial communication)
- Add cross-platform support (native, wasm, embedded)
- Change error handling patterns

## Architecture Principles

### 1. Workspace Structure (Multi-Crate)

```
project/
├── library-crate/
│   ├── src/
│   │   ├── lib.rs
│   │   ├── error.rs
│   │   └── ...
│   └── Cargo.toml
├── cli-crate/ (optional)
│   └── src/
│       └── main.rs
└── Cargo.toml
```

### 2. Module Organization

- **Clear separation**: Each module has a single responsibility
- **Hierarchical**: Core → Protocols → Implementations → Platform-specific
- **Feature-gated**: Platform-specific code behind feature flags

## Public API Design

### 1. Re-exports Pattern

```rust
// lib.rs - Clean public API
pub use target::{Target, Handler, Config};
pub use error::{Error, Result};
pub use image::{Image, ImageInfo};

// Keep internal modules accessible but not cluttered
pub mod device;
pub mod protocol;
```

### 2. Trait-Based Abstraction

```rust
// Core trait for operations
pub trait Handler: Send + Sync {
    fn connect(&mut self) -> Result<()>;
    fn read(&mut self, data: &mut [u8]) -> Result<usize>;
    fn write(&mut self, data: &[u8]) -> Result<usize>;
    // ...
}

// Target family creates handler
pub enum Target {
    VariantA,
    VariantB,
}

impl Target {
    pub fn create_handler(&self, port: &str, config: &Config) -> Result<Box<dyn Handler>> {
        match self {
            Target::VariantA => Ok(Box::new(HandlerA::new(port, config)?)),
            Target::VariantB => todo!(),
        }
    }
}
```

### 3. Configuration Pattern

```rust
#[derive(Debug, Clone)]
pub struct Config {
    pub timeout_ms: u32,
    pub retries: u32,
    pub buffer_size: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            timeout_ms: 1000,
            retries: 3,
            buffer_size: 4096,
        }
    }
}
```

## Error Handling

### 1. Error Enum with thiserror

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("Parse error: {0}")]
    Parse(String),
    
    #[error("Device not found: {0}")]
    NotFound(String),
    
    #[error("Connection failed: {0}")]
    Connection(String),
    
    #[error("Timeout after {0} attempts")]
    Timeout(u32),
}
```

### 2. Result Type Alias

```rust
pub type Result<T> = std::result::Result<T, Error>;
```

### 3. Context-Rich Errors

```rust
impl Error {
    pub fn with_context(self, msg: impl Into<String>) -> Error {
        Error::Context(msg.into(), Box::new(self))
    }
}
```

## Feature Flags

### 1. Standard Features

```toml
[features]
default = ["std"]
std = []
native = ["dep:some-native-crate"]
wasm = ["dep:wasm-bindgen"]
serde = ["dep:serde", "dep:toml"]
```

### 2. Usage in Code

```rust
#[cfg(feature = "native")]
use some_crate::{NativeType, NativeConfig};

#[cfg(feature = "wasm")]
use wasm_bindgen::JsValue;

pub trait Handler: Send + Sync {
    #[cfg(feature = "native")]
    fn open_native(config: &NativeConfig) -> Result<Self>
    where Self: Sized;
    
    #[cfg(feature = "wasm")]
    fn open_web() -> Result<Self>
    where Self: Sized;
}
```

## Protocol Implementation

### 1. Binary Protocol Pattern

```rust
use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};

pub struct Frame {
    pub magic: u32,
    pub length: u16,
    pub command: u8,
    pub data: Vec<u8>,
    pub crc: u16,
}

impl Frame {
    pub fn parse(data: &[u8]) -> Result<Self> {
        // Implement parsing with byteorder
    }
    
    pub fn serialize(&self) -> Vec<u8> {
        // Implement serialization
    }
}
```

### 2. CRC Calculation

```rust
pub fn calculate_crc(data: &[u8]) -> u16 {
    // Use crc16 crate or custom implementation
    crc16::xmodem(data)
}
```

## Testing Strategy

### 1. Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_frame_parse() {
        // Test frame parsing
    }
    
    #[test]
    fn test_crc() {
        // Test CRC calculation
    }
}
```

### 2. Integration Tests

```rust
#[cfg(test)]
mod integration {
    #[test]
    fn test_image_parse() {
        // Test firmware/image parsing
    }
}
```

## Documentation Requirements

### 1. Module Documentation

```rust
//! # library-name
//!
//! A library for doing something useful.
//!
//! ## Supported Platforms
//!
//! - Native (Linux, macOS, Windows)
//! - WASM (Web browsers)
//!
//! ## Features
//!
//! - `native`: Native system support
//! - `wasm`: WebAssembly support
//! - `serde`: Serialization support
```

### 2. Public API Documentation

```rust
/// Creates a new handler for the specified device.
///
/// # Arguments
///
/// * `port` - Device path (e.g., "/dev/ttyUSB0")
/// * `config` - Configuration options
///
/// # Errors
///
/// Returns [`Error::NotFound`] if device is not found.
///
/// # Example
///
/// ```rust,no_run
/// use library_name::{Target, Config};
/// let handler = Target::VariantA.create_handler("/dev/ttyUSB0", &Config::default())?;
/// ```
pub fn create_handler(port: &str, config: &Config) -> Result<Box<dyn Handler>> {
    // Implementation
}
```

## Cross-Platform Considerations

### 1. Device/Port Abstraction

```rust
pub trait Port: Send + Sync {
    fn write(&mut self, data: &[u8]) -> Result<usize>;
    fn read(&mut self, data: &mut [u8]) -> Result<usize>;
    fn set_timeout(&mut self, timeout: Duration) -> Result<()>;
}

#[cfg(feature = "native")]
mod native {
    pub struct NativePort { /* ... */ }
}

#[cfg(feature = "wasm")]
mod wasm {
    pub struct WasmPort { /* ... */ }
}
```

### 2. No-STD Support (Future)

```rust
#![no_std]

#[cfg(not(feature = "std"))]
extern crate alloc;
```

## Code Quality

### 1. Lints

```rust
#![warn(missing_docs)]
#![warn(clippy::all)]
#![deny(unsafe_code)] // If possible
```

### 2. Required Commands

```bash
# Check code
cargo clippy --all-targets --all-features -- -D warnings

# Format
cargo fmt --all

# Test
cargo test

# Audit
cargo audit
```

## Dependencies Guidelines

### 1. Minimal Dependencies

- Use std library when possible
- Prefer `no_std` compatible crates
- Avoid heavy dependencies (tokio unless needed)

### 2. Common Dependencies

| Category | Crate | Purpose |
|----------|-------|---------|
| Error | thiserror | Error enum derive |
| Serial | serialport | Cross-platform serial |
| Binary | byteorder | Read/write binary |
| CRC | crc16 | CRC calculation |
| Logging | log | Logging facade |
| Time | portable-atomic | Atomic time |

## Anti-Patterns to Avoid

1. **Exposing internal types**: Keep implementation details private
2. **Panic on errors**: Return `Result` instead
3. **Undocumented public API**: All public items need docs
4. **Breaking changes**: Use versioning, deprecation warnings
5. **Missing feature gates**: Platform-specific code behind features

## Design Workflow

### Step 1: Identify Responsibility
- Single Responsibility Principle per module
- Clear boundaries between layers

### Step 2: Define Public API
- What needs to be public?
- What can stay private?

### Step 3: Choose Abstractions
- Use traits for polymorphism
- Use enums for variants

### Step 4: Handle Errors
- Define error enum
- Add context where needed

### Step 5: Document
- Module-level docs
- Public API docs
- Examples

### Step 6: Test
- Unit tests for logic
- Integration tests for workflows

---

*For general Rust patterns, see [Rust Design Patterns](https://github.com/rust-unofficial/patterns).*
