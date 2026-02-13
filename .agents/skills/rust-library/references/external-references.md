# Rust Library Design - External References

This document contains curated links to external resources for Rust library design best practices.

## Official Resources

### Rust Language Reference
- [The Rust Book](https://doc.rust-lang.org/book/) - Official Rust documentation
- [Rust Reference](https://doc.rust-lang.org/reference/) - Language reference
- [API Guidelines](https://rust-lang.github.io/api-guidelines/) - Official API design guidelines

### Rust Standard Library
- [std Library Docs](https://doc.rust-lang.org/std/) - Standard library documentation
- [Cargo Book](https://doc.rust-lang.org/cargo/) - Package manager documentation
- [Rust by Example](https://doc.rust-lang.org/rust-by-example/) - Learning by examples

## Design Patterns

### Official Patterns Catalog
- [Rust Design Patterns](https://github.com/rust-unofficial/patterns) - Official patterns collection
- [Anti-Patterns](https://github.com/rust-unofficial/patterns/blob/main/anti-patterns/README.md) - What to avoid

### Specific Patterns
- [Builder Pattern](https://rust-unofficial.github.io/patterns/patterns/creational/builder.html)
- [Newtype Pattern](https://rust-unofficial.github.io/patterns/patterns/structural/newtype.html)
- [Type State Pattern](https://rust-unofficial.github.io/patterns/patterns/type-state.html)
- [RAII Pattern](https://rust-unofficial.github.io/patterns/patterns/idiomatic/raii.html)

## API Design

### Guidelines
- [Rust API Guidelines (mirror)](https://corrode.dev/blog/rust-api-design-guidelines/) - API design best practices
- [Idiomatic Rust](https://github.com/mre/idiomatic-rust) - Writing idiomatic Rust

### Error Handling
- [Error Handling Book Chapter](https://doc.rust-lang.org/book/ch09-00-error-handling.html)
- [thiserror crate](https://docs.rs/thiserror/latest/thiserror/) - Error enum derive
- [anyhow crate](https://docs.rs/anyhow/latest/anyhow/) - Application error handling

## Embedded Rust

### No-Std Development
- [Embedded Rust Book](https://doc.rust-lang.org/embedded-book/) - Official embedded guide
- [Awesome Embedded Rust](https://github.com/rust-embedded/awesome-embedded-rust) - Curated embedded resources
- [No-Std](https://rust-unofficial.github.io/the-rust-programming-language/unsupported.html) - No-std overview

### Embedded Working Group
- [embedded WG](https://github.com/rust-embedded/wg) - Embedded working group
- [HALs](https://github.com/rust-embedded/awesome-embedded-rust#hal-implementations) - Hardware Abstraction Layers

## Communication Libraries

> Note: These are examples of communication libraries that may be useful depending on your project needs.

### Serial/Network Ports
- [serialport crate](https://docs.rs/serialport/latest/serialport/) - Cross-platform serial port
- [tokio-serial](https://docs.rs/tokio-serial/latest/tokio_serial/) - Async serial ports

### Binary Protocol Parsing
- [byteorder crate](https://docs.rs/byteorder/latest/byteorder/) - Byte order handling
- [bytes crate](https://docs.rs/bytes/latest/bytes/) - Efficient byte handling
- [nom parser](https://github.com/rust-bakery/nom) - Parser combinators

### Checksum/CRC
- [crc crate](https://docs.rs/crc/latest/crc/) - CRC calculations
- [crc16 crate](https://docs.rs/crc16/latest/crc16/) - CRC16 implementations

## Testing

### Testing in Rust
- [Testing Book Chapter](https://doc.rust-lang.org/book/ch11-00-testing.html)
- [rstest](https://docs.rs/rstest/latest/rstest/) - Parametric testing
- [proptest](https://docs.rs/proptest/latest/proptest/) - Property-based testing
- [criterion](https://docs.rs/criterion/latest/criterion/) - Benchmarking

### Fuzzing
- [cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz) - Fuzzing
- [AFL.rs](https://github.com/rust-fuzz/afl.rs) - American Fuzzy Lop

## Performance

### Optimizations
- [Rust Performance Book](https://nnethercote.github.io/perf-book/) - Performance guide
- [Compiler Performance](https://perf.rust-lang.org/) - Rust compiler performance

### Memory
- [ allocations](https://doc.rust-lang.org/nightly/unstable-book/compiler-flags/sanitizer.html) - Memory tools
- [dhat](https://docs.rs/dhat/latest/dhat/) - Heap profiler

## Security

### Security in Rust
- [Rust Security Handbook](https://github.com/yevh/rust-security-handbook) - Security best practices
- [Cargo Audit](https://github.com/rustsec/rustsec) - Dependency vulnerability scanning
- [Rust Sec](https://rustsec.org/) - Security advisories

## Tooling

### Development Tools
- [rustfmt](https://github.com/rust-lang/rustfmt) - Code formatter
- [clippy](https://github.com/rust-lang/rust-clippy) - Linter
- [rust-analyzer](https://github.com/rust-lang/rust-analyzer) - Language server
- [cargo-expand](https://github.com/dtolnay/cargo-expand) - Macro expansion

### Documentation
- [mdBook](https://rust-lang.github.io/mdBook/) - Documentation generation
- [docs.rs](https://docs.rs/) - Package documentation hosting

## Learning Resources

### Books
- [Programming Rust](https://www.oreilly.com/library/view/programming-rust-2nd/9781492052586/) - Comprehensive Rust book
- [Rust in Action](https://www.manning.com/books/rust-in-action) - Hands-on systems programming
- [Hands-on Rust](https://pragprog.com/titles/hwrust/hands-on-rust/) - Learning by building games

### Courses
- [Comprehensive Rust](https://google.github.io/comprehensive-rust/) - Google's Rust course
- [Rustlings](https://github.com/rust-lang/rustlings) - Small exercises
- [Exercism Rust Track](https://exercism.org/tracks/rust) - Programming exercises

---

*Last updated: 2026-02-13*
