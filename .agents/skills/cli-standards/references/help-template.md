# Help Output Template

Use this as a baseline for top-level and subcommand help.

## Top-Level Help Template

```text
mycmd - <one-line description>

USAGE:
  mycmd <command> [options] [--] [operands...]

COMMON COMMANDS:
  init        Initialize project metadata
  build       Build artifacts
  flash       Flash firmware to device

GLOBAL OPTIONS:
  -h, --help          Show help and exit
      --version       Show version and exit
  -v, --verbose       Increase verbosity
  -q, --quiet         Reduce non-essential output
      --json          Output machine-readable JSON
      --no-input      Disable interactive prompts

EXAMPLES:
  mycmd flash -p /dev/ttyUSB0 firmware.fwpkg
  mycmd build --json
  mycmd --help

For more details:
  mycmd help <command>
```

## Subcommand Help Template

```text
mycmd flash - Flash firmware to a target device

USAGE:
  mycmd flash [options] -- <image>

OPTIONS:
  -p, --port <PORT>         Serial port path
  -b, --baud <BAUD>         Baud rate
  -n, --dry-run             Show actions without executing
  -f, --force               Skip confirmation checks
  -h, --help                Show help and exit

EXAMPLES:
  mycmd flash -p /dev/ttyUSB0 app.fwpkg
  mycmd flash --dry-run -- app.fwpkg

EXIT CODES:
  0  Success
  2  Invalid arguments
  3  Device connection failed
  4  Flash operation failed
```

## Style Notes

- Keep the first screen concise; place advanced details in dedicated docs.
- Place examples near the top for discoverability.
- If a command can be destructive, explicitly describe confirmation and force behavior.
