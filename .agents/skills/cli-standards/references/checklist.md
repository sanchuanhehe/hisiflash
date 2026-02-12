# CLI Quality Checklist

## A. Invocation & Syntax

- [ ] Usage synopsis clearly separates options and operands.
- [ ] Options can appear before operands (documented and supported).
- [ ] `--` is supported for end-of-options/disambiguation where needed.
- [ ] Positional args count is minimal and justified.
- [ ] Subcommand naming is unambiguous and consistent.

## B. Standard Options

- [ ] `-h` and `--help` are supported.
- [ ] `--help` ignores unrelated args and exits with code 0.
- [ ] `--version` is supported and exits with code 0.
- [ ] Common flags use conventional names (`--verbose`, `--quiet`, `--dry-run`, `--force`, `--output`).

## C. Output Discipline

- [ ] Primary/business output is written to stdout.
- [ ] Errors/warnings/progress are written to stderr.
- [ ] Machine mode exists when needed (e.g. `--json`).
- [ ] No unstable decoration in machine mode.
- [ ] Colors/animations are disabled automatically when not in TTY.

## D. Errors & Exit Codes

- [ ] Exit code `0` means success.
- [ ] Non-zero exit codes are documented for key failure classes.
- [ ] Error text is actionable (cause + fix suggestion).
- [ ] Unknown command/flag provides suggestions when possible.

## E. Interactivity & Safety

- [ ] No prompt is required in non-interactive/script mode.
- [ ] `--no-input` (or equivalent) disables prompts.
- [ ] Destructive operations require confirmation or explicit force flag.
- [ ] `--dry-run` is provided for high-risk operations.

## F. Compatibility & Docs

- [ ] Existing scripts are not broken by default behavior changes.
- [ ] Deprecations include warnings and migration path.
- [ ] Help includes concise examples.
- [ ] Docs explain config/env precedence and non-interactive usage.
