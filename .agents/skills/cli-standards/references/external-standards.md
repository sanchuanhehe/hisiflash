# External CLI Standards References

This document contains links to external standards and guidelines for command-line interface design.

## Official Standards

### GNU Coding Standards - Command Line Interfaces
**URL:** https://www.gnu.org/prep/standards/standards.html#Command_002dLine-Interfaces

Key points:
- `--version` output format: first line is program name and version (e.g., "GNU hello 2.3"), followed by copyright and license info
- `--help` should output brief documentation to stdout, then exit successfully
- Use `getopt_long` for argument parsing
- Support long-named options equivalent to single-letter options
- All programs should support `--version` and `--help`

### POSIX Utility Conventions
**URL:** https://pubs.opengroup.org/onlinepubs/9699919799/basedefs/V1_chap12.html

Key points:
- Guidelines 1-14 for utility argument syntax
- Options preceded by `-` delimiter
- First `--` terminates options
- Order of operands may matter
- The `-` operand means standard input/stdout

## Industry Guidelines

### Git CLI Conventions
**URL:** https://git-scm.com/docs/gitcli

Key points:
- Options come first, then arguments
- Use `--` to disambiguate revisions from paths
- Long options can be abbreviated to unique prefix
- `-h` gives pretty printed usage
- Magic options like `--help-all`

### CLIG - Command Line Interface Guidelines
**URL:** https://clig.dev/

Key points:
- Human-first design philosophy
- stdout for primary output, stderr for messaging
- Exit codes: 0 for success, non-zero for failure
- Comprehensive help text with `-h` and `--help`
- Examples in help text
- Progress for long operations
- Configuration: flags > env vars > config files
- Validate user input early

## Summary Table

| Source | Focus | URL |
|--------|-------|-----|
| GNU Standards | `--version`, `--help` format, getopt | https://www.gnu.org/prep/standards/standards.html#Command_002dLine-Interfaces |
| POSIX | Argument syntax guidelines | https://pubs.opengroup.org/onlinepubs/9699919799/basedefs/V1_chap12.html |
| Git CLI | Options/args ordering, magic options | https://git-scm.com/docs/gitcli |
| CLIG | Human-first design, UX best practices | https://clig.dev/ |
