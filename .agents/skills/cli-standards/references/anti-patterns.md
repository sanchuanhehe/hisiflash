# CLI Anti-Patterns

This file lists common anti-patterns in CLI design and implementation, with minimal corrective actions.

## 1) Arguments and parsing

### Anti-pattern: Inconsistent option/operand ordering
- **Symptom**: The same command expects different argument order in different contexts.
- **Risk**: Script instability and behavior drift after upgrades.
- **Fix**: Standardize on `command [options] [--] [operands...]`; keep all docs options-first.

### Anti-pattern: Missing `--` end-of-options support
- **Symptom**: Filenames/operands beginning with `-` cannot be passed reliably.
- **Risk**: Misparsed as flags, causing errors or destructive behavior.
- **Fix**: Support and document `--`; include disambiguation examples in help.

### Anti-pattern: Optional option-argument ambiguity
- **Symptom**: Boundary between `--opt value` and operands is unclear.
- **Risk**: Behavior depends on parser implementation details.
- **Fix**: Avoid optional option-arguments when possible; if required, allow only attached form and document clearly.

## 2) Option naming

### Anti-pattern: Conflicting meaning for `-v`
- **Symptom**: Some subcommands use `-v=verbose`, others `-v=version`.
- **Risk**: Frequent misuse in both scripts and manual usage.
- **Fix**: Standardize globally on `-v/--verbose`; use `--version` (optional `-V`) for version.

### Anti-pattern: Same option name, different meaning across subcommands
- **Symptom**: `--force` means different things in different subcommands.
- **Risk**: Higher cognitive load and increased accidental misuse.
- **Fix**: Keep same-name/same-meaning; rename when semantics must differ and document explicitly.

### Anti-pattern: Accepting arbitrary abbreviated subcommands by default
- **Symptom**: `ins` auto-maps to `install`.
- **Risk**: Blocks adding future commands like `inspect`.
- **Fix**: Support only explicit, stable aliases; disallow arbitrary prefix matching.

## 3) Output and composability

### Anti-pattern: Logs mixed into stdout
- **Symptom**: Business data mixed with `INFO/DEBUG` lines.
- **Risk**: Pipe/script parsing breaks.
- **Fix**: Data only on stdout; logs/warnings/progress only on stderr.

### Anti-pattern: Human-only output format, no machine mode
- **Symptom**: Table/color output cannot be parsed stably.
- **Risk**: Automation integration becomes fragile.
- **Fix**: Provide `--json` (or stable plain mode) with a compatibility policy for fields.

### Anti-pattern: Color/animation still enabled on non-TTY
- **Symptom**: Garbled CI logs and noisy progress artifacts.
- **Risk**: Poor readability and false failure perception.
- **Fix**: Auto-disable on non-TTY; honor `NO_COLOR` and `--no-color`.

## 4) Errors and exit codes

### Anti-pattern: Non-actionable error messages
- **Symptom**: Only “failed/invalid” without context or fix suggestions.
- **Risk**: Higher debugging and support costs.
- **Fix**: Include failure point + cause + next command/action.

### Anti-pattern: Exit code semantics drift
- **Symptom**: Same failure returns different codes across versions.
- **Risk**: Upstream scripts break.
- **Fix**: Stabilize key exit-code semantics and document primary codes.

### Anti-pattern: Returning success for business failures
- **Symptom**: Failure paths still return `0`.
- **Risk**: Automation falsely treats runs as successful.
- **Fix**: Enforce `0=success`; non-zero for all failures, ideally categorized.

## 5) Interactivity and safety

### Anti-pattern: Interactive input required to run
- **Symptom**: Missing args trigger prompts and CI hangs.
- **Risk**: No automation path.
- **Fix**: Provide complete non-interactive args/flags and support `--no-input`.

### Anti-pattern: No confirmation for high-risk operations
- **Symptom**: Delete/overwrite/remote mutations execute immediately.
- **Risk**: Irreversible incidents.
- **Fix**: Confirm in interactive mode; require `--force` or `--confirm=<target>` in scripts.

### Anti-pattern: Reading secrets directly from flags/env
- **Symptom**: `--password xxx` or plaintext env-var credentials.
- **Risk**: Leaks via shell history, process list, and logs.
- **Fix**: Use `--password-file`, stdin, or dedicated secret providers.

## 6) Compatibility and evolution

### Anti-pattern: Silent changes to released behavior
- **Symptom**: Same option name changes meaning without warning.
- **Risk**: Production scripts fail silently.
- **Fix**: Deprecate first, provide migration window, then remove; keep changes visible.

### Anti-pattern: Treating human-readable text as stable API
- **Symptom**: Scripts depend on colored/natural-language output.
- **Risk**: Minor wording changes break integrations.
- **Fix**: Recommend `--json`/`--plain` for scripts and keep machine interfaces stable.

## 7) Quick PR review questions

- Does it support `-h/--help`, `--version`, and `--`?
- Are stdout and stderr strictly separated?
- Is there a `--json` (or equivalent machine mode)?
- Do high-risk commands provide `--dry-run` + `--force/--confirm`?
- Will non-interactive environments avoid hanging on prompts?
- Does this change affect existing scripts, and is migration guidance provided?

## 8) Related references

- Option map: [option-map.md](option-map.md)
- Validation checklist: [checklist.md](checklist.md)
- Help template: [help-template.md](help-template.md)
