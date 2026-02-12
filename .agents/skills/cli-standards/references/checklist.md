# CLI Quality Checklist

## A. Invocation and Syntax

- [ ] Ensure the usage synopsis clearly separates options and operands.
- [ ] Ensure options can appear before operands (documented and supported).
- [ ] Ensure `--` is supported for end-of-options disambiguation where needed.
- [ ] Ensure positional argument count is minimal and justified.
- [ ] Ensure subcommand naming is unambiguous and consistent.

## B. Standard Options

- [ ] Ensure `-h` and `--help` are supported.
- [ ] Ensure `--help` ignores unrelated args and exits with code 0.
- [ ] Ensure `--version` is supported and exits with code 0.
- [ ] Ensure common flags use conventional names (`--verbose`, `--quiet`, `--dry-run`, `--force`, `--output`).

## C. Output Discipline

- [ ] Ensure primary output is written to stdout.
- [ ] Ensure errors, warnings, and progress are written to stderr.
- [ ] Ensure machine mode exists when needed (e.g. `--json`).
- [ ] Ensure machine mode does not include unstable decoration.
- [ ] Ensure colors and animations are disabled automatically on non-TTY.

## D. Errors and Exit Codes

- [ ] Ensure exit code `0` means success.
- [ ] Ensure non-zero exit codes are documented for key failure classes.
- [ ] Ensure error text is actionable (cause + fix suggestion).
- [ ] Ensure unknown commands and flags provide suggestions when possible.

## E. Interactivity and Safety

- [ ] Ensure no prompt is required in non-interactive script mode.
- [ ] Ensure `--no-input` (or equivalent) disables prompts.
- [ ] Ensure destructive operations require confirmation or an explicit force flag.
- [ ] Ensure `--dry-run` is available for high-risk operations.

## F. Compatibility and Docs

- [ ] Ensure existing scripts are not broken by default behavior changes.
- [ ] Ensure deprecations include warnings and a migration path.
- [ ] Ensure help includes concise examples.
- [ ] Ensure docs explain config/env precedence and non-interactive usage.
