---
name: cli-standards
description: Define, implement, and review command-line interface behavior for CLI tools. Use when designing commands/subcommands, flags/args, help text, stdout/stderr contracts, exit codes, interactivity, and automation compatibility.
compatibility: Generic skill for coding agents and CLI projects; references POSIX utility conventions, GNU CLI standards, gitcli conventions, and clig.dev best practices.
metadata:
  author: sanchuanhehe
  version: "1.0"
---

# CLI Standards Skill

## Purpose

Use this skill to ensure a CLI is predictable, script-friendly, and human-friendly.

Primary references:
- POSIX Utility Conventions (argument syntax and ordering baseline)
- GNU CLI standards (`--help`, `--version`, long options)
- Git CLI conventions (disambiguation with `--`, options-before-args discipline)
- CLIG (human-first UX, errors, output, discoverability)

## When to activate

Activate when the task includes any of the following:
- Add or change command/subcommand structure
- Add or rename flags/options/arguments
- Change help text, usage, examples, or docs
- Change stdout/stderr format, logs, progress, or JSON output
- Change exit codes or error handling
- Add interactive prompts, confirmations, or non-interactive mode

## Non-negotiable defaults

1. **Options before operands** by default.
2. Support `-h` and `--help`; both must show help and exit success.
3. Support `--version`; print version info to stdout and exit success.
4. Primary output to stdout; diagnostics/errors/progress to stderr.
5. Exit code `0` for success, non-zero for failure.
6. Support `--` to end option parsing when operands may start with `-`.
7. Avoid breaking existing flags/subcommands; prefer additive changes.

## Design workflow

### Step 1: Command model
- Prefer explicit subcommands for complex workflows.
- Keep naming consistent across subcommands.
- Avoid ambiguous command names (`update` vs `upgrade`) unless clearly distinct.

### Step 2: Args and flags
- Prefer flags over too many positional args.
- Provide both short and long forms for high-frequency options.
- Use standard names where possible:
  - `-h`, `--help`
  - `--version`
  - `-v`/`--verbose` (pick one semantics and stay consistent)
  - `-q`, `--quiet`
  - `-n`, `--dry-run`
  - `-f`, `--force`
  - `-o`, `--output`
- For scripts, require full long option names in docs/examples.

### Step 3: Output contract
- Human-readable default output.
- If machine integration is needed, provide `--json` (or equivalent stable format).
- Do not mix parseable data with log lines on stdout.
- If output is long, consider pager only for interactive TTY.

### Step 4: Errors and UX
- Error messages should be actionable: what failed + why + next fix.
- If likely typo, suggest closest command/flag.
- For destructive actions, confirm in interactive mode; require explicit force/confirm flag for non-interactive mode.
- Never require prompts in CI/script mode.

### Step 5: Compatibility and evolution
- Keep existing behavior stable unless a major version migration is explicit.
- If deprecating, warn clearly and offer replacement.
- Preserve automation paths (`--no-input`, `--json`, stable exit semantics).

## Priority when standards conflict

1. **Project-specific compatibility requirements** (existing CLI contract)
2. **POSIX conventions** (syntax/order/disambiguation baseline)
3. **GNU conventions** (`--help`, `--version`, long options)
4. **Git CLI and CLIG practices** (usability/script robustness)

## Deliverables expected from agent

For each CLI change, produce:
1. Updated command/flag design.
2. Updated help output/examples.
3. Explicit stdout/stderr and exit code behavior.
4. Backward-compatibility note.
5. Validation checklist result using [references/checklist.md](references/checklist.md).

## Quick references

- Quality checklist: [references/checklist.md](references/checklist.md)
- Help text template: [references/help-template.md](references/help-template.md)
- Option naming map: [references/option-map.md](references/option-map.md)
- Review anti-patterns: [references/anti-patterns.md](references/anti-patterns.md)
- PR scoring rubric: [references/review-rubric.md](references/review-rubric.md)
