# CLI Option Map

This file defines **recommended semantics**, **suggested short options**, and **ambiguities to avoid** for common CLI flags.

## 1) Global Common Options (Preferred)

| Meaning | Long option (recommended) | Short option (suggested) | Notes |
|---|---|---|---|
| Help | `--help` | `-h` | Must be supported; show help and exit 0 |
| Version | `--version` | none or `-V` | Must be supported; print version and exit 0 |
| Verbose logging | `--verbose` | `-v` | If repeatable, document behavior (e.g. `-vv`) |
| Quiet mode | `--quiet` / `--silent` | `-q` | `--quiet` and `--silent` should be synonyms |
| Debug mode | `--debug` | `-d` | Send debug output to stderr |
| Dry run | `--dry-run` | `-n` | No state changes; show intended actions only |
| Force execution | `--force` | `-f` | Skip confirmations/safeguards; required for risky ops |
| Non-interactive | `--no-input` | none | Disable prompts; CI/script friendly |
| Output file | `--output <FILE>` | `-o <FILE>` | Redirect output to a file |
| JSON output | `--json` | none | Stable machine-readable output on stdout |
| Disable color | `--no-color` | none | Also honor `NO_COLOR` |

## 2) Domain-Common Options (Use as Needed)

| Meaning | Long option (recommended) | Short option (suggested) | Typical usage |
|---|---|---|---|
| All objects | `--all` | `-a` | list/show/search |
| Recursive | `--recursive` | `-r` | file/resource trees |
| User | `--user <USER>` | `-u <USER>` | credentials/target account |
| Port | `--port <PORT>` | `-p <PORT>` | serial/TCP port |
| Timeout | `--timeout <DURATION>` | none | network/device operations |
| Config file | `--config <FILE>` | `-c <FILE>` | select config source |
| Staged/index only | `--staged` / `--cached` | none | Git-like workflows |

## 3) Recommended and Avoided Patterns

### Recommended

- Use full long options in docs/script examples for robustness.
- Provide both `--dry-run` and `--force` for potentially destructive operations.
- Support `--` to end option parsing and remove `-`-prefixed operand ambiguity.
- Separate machine data from human messaging: data on stdout, diagnostics on stderr.
- If color/animation exists, auto-disable on non-TTY outputs.

### Avoid

- Do not assign `-v` to both `version` and `verbose`.
- Do not reuse short flags with conflicting meanings across subcommands.
- Do not require interactive input only; always provide non-interactive flags/args.
- Do not mix log prefixes (`INFO`/`DEBUG`) into stdout machine output.
- Do not allow arbitrary subcommand abbreviations (blocks future expansion).

## 4) Parameter Order and Formatting

- Preferred form: `command [options] [--] [operands...]`.
- Put options first, operands after options.
- For required option values:
  - Prefer documenting long options as `--option=value`
  - Also accept `--option value` unless parser/project constraints forbid it
- For short options, prioritize readability in scripts over heavy aggregation.

## 5) Compatibility Strategy

- Do not silently change semantics of released options; prefer additive options.
- If a change is unavoidable:
  1. Emit a deprecation warning first.
  2. Provide migration examples.
  3. Define and publish a removal window in release notes.

## 6) Standards Alignment

- POSIX: argument syntax, `--` termination, option ordering conventions.
- GNU: `--help`/`--version`, long-option consistency.
- gitcli: ambiguity disambiguation and options-first discipline.
- CLIG: human-first output, actionable errors, script stability.
