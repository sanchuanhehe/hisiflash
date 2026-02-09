//! Serial monitor command implementation.
//!
//! Dual-threaded serial monitor with keyboard input, timestamps, and log file support.

use anyhow::{Context, Result};
use console::style;
use rust_i18n::t;
use std::io;
use std::io::Write as _;
use std::path::PathBuf;

use crate::config::Config;
use crate::{Cli, get_port};

/// Run the serial monitor.
///
/// - Reader thread: serial → stdout (with optional timestamps and ANSI passthrough)
/// - Main thread: keyboard (crossterm raw mode) → serial
/// - Ctrl+C: graceful exit
/// - Ctrl+R: reset device (DTR/RTS toggle)
/// - Ctrl+T: toggle timestamp display
pub(crate) fn cmd_monitor(
    cli: &Cli,
    config: &mut Config,
    monitor_baud: u32,
    timestamp: bool,
    log_file: Option<&PathBuf>,
) -> Result<()> {
    use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
    use crossterm::terminal;
    use std::io::Read as _;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    let port_name = get_port(cli, config)?;

    eprintln!(
        "{} {}",
        style("📡").cyan(),
        t!(
            "monitor.opening",
            port = style(&port_name).green().to_string(),
            baud = monitor_baud
        )
    );
    eprintln!("{}", style(t!("monitor.exit_hint")).dim());

    // Open serial port
    let serial = serialport::new(&port_name, monitor_baud)
        .timeout(Duration::from_millis(50))
        .open()
        .with_context(|| t!("error.open_port", port = port_name.clone()))?;

    // Clone for the reader thread
    let mut serial_reader = serial
        .try_clone()
        .context(t!("error.serial_error").to_string())?;
    let mut serial_writer = serial;

    // Shared state
    let running = Arc::new(AtomicBool::new(true));
    let running_reader = running.clone();
    let show_timestamp = Arc::new(AtomicBool::new(timestamp));
    let show_timestamp_reader = show_timestamp.clone();

    // Open log file if specified
    let log_writer: Option<std::sync::Mutex<std::fs::File>> = log_file.map(|path| {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .with_context(|| format!("Failed to open log file: {}", path.display()))
            .unwrap();
        eprintln!(
            "{} {}",
            style("📝").cyan(),
            t!("monitor.logging", path = path.display().to_string())
        );
        std::sync::Mutex::new(file)
    });

    // Reader thread: serial → stdout
    let reader_handle = std::thread::spawn(move || {
        let mut buf = [0u8; 1024];
        // Track whether we're at the beginning of a new line (for timestamp insertion)
        let mut at_line_start = true;
        // Buffer for partial UTF-8 sequences that span read boundaries
        let mut utf8_buf: Vec<u8> = Vec::new();

        while running_reader.load(Ordering::Relaxed) {
            match serial_reader.read(&mut buf) {
                Ok(0) => {},
                Ok(n) => {
                    let data = &buf[..n];

                    // Append to UTF-8 buffer for handling partial sequences
                    utf8_buf.extend_from_slice(data);

                    // Find the longest valid UTF-8 prefix
                    let (valid, remainder) = split_utf8(&utf8_buf);

                    if !valid.is_empty() {
                        // Write to log file (raw, no timestamps)
                        if let Some(ref log) = log_writer {
                            if let Ok(mut f) = log.lock() {
                                let _ = f.write_all(valid.as_bytes());
                            }
                        }

                        // Process output with optional timestamps
                        let ts_enabled = show_timestamp_reader.load(Ordering::Relaxed);
                        let output = format_monitor_output(valid, ts_enabled, &mut at_line_start);
                        print!("{output}");
                        io::stdout().flush().ok();
                    }

                    // Keep remainder for next iteration
                    let remainder_bytes = remainder.to_vec();
                    utf8_buf.clear();
                    utf8_buf.extend_from_slice(&remainder_bytes);
                },
                Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => {},
                Err(ref e) if e.kind() == std::io::ErrorKind::BrokenPipe => break,
                Err(_) => {
                    if running_reader.load(Ordering::Relaxed) {
                        // Only report if we haven't been asked to stop
                        break;
                    }
                },
            }
        }
    });

    // Enter raw mode for keyboard input
    terminal::enable_raw_mode().context("Failed to enable raw terminal mode")?;

    // Ensure we restore terminal on exit (even on panic)
    let _raw_guard = RawModeGuard;

    // Main thread: keyboard → serial
    while running.load(Ordering::Relaxed) {
        // Poll for keyboard events with timeout
        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(KeyEvent {
                code, modifiers, ..
            }) = event::read()?
            {
                match (code, modifiers) {
                    // Ctrl+C: exit
                    (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                        running.store(false, Ordering::Relaxed);
                        break;
                    },
                    // Ctrl+R: reset device via DTR/RTS toggle
                    (KeyCode::Char('r'), KeyModifiers::CONTROL) => {
                        eprintln!("\r\n{} {}", style("🔄").cyan(), t!("monitor.resetting"));
                        let _ = serial_writer.write_data_terminal_ready(false);
                        let _ = serial_writer.write_request_to_send(false);
                        std::thread::sleep(Duration::from_millis(100));
                        let _ = serial_writer.write_data_terminal_ready(true);
                        let _ = serial_writer.write_request_to_send(true);
                        std::thread::sleep(Duration::from_millis(100));
                        let _ = serial_writer.write_data_terminal_ready(false);
                    },
                    // Ctrl+T: toggle timestamp
                    (KeyCode::Char('t'), KeyModifiers::CONTROL) => {
                        let current = show_timestamp.load(Ordering::Relaxed);
                        show_timestamp.store(!current, Ordering::Relaxed);
                        let state = if current {
                            t!("monitor.timestamp_off")
                        } else {
                            t!("monitor.timestamp_on")
                        };
                        eprintln!("\r\n{} {state}", style("⏱").cyan());
                    },
                    // Enter: send \r\n (works with both \n and \r\n devices)
                    (KeyCode::Enter, _) => {
                        let _ = serial_writer.write_all(b"\r\n");
                    },
                    // Regular character
                    (KeyCode::Char(c), KeyModifiers::NONE | KeyModifiers::SHIFT) => {
                        let mut buf = [0u8; 4];
                        let bytes = c.encode_utf8(&mut buf);
                        let _ = serial_writer.write_all(bytes.as_bytes());
                    },
                    // Backspace
                    (KeyCode::Backspace, _) => {
                        let _ = serial_writer.write_all(&[0x08]); // BS
                    },
                    // Tab
                    (KeyCode::Tab, _) => {
                        let _ = serial_writer.write_all(&[0x09]); // HT
                    },
                    // Escape
                    (KeyCode::Esc, _) => {
                        let _ = serial_writer.write_all(&[0x1B]);
                    },
                    _ => {},
                }
            }
        }
    }

    // Wait for reader thread to finish
    let _ = reader_handle.join();
    eprintln!("\r\n{} {}", style("👋").cyan(), t!("monitor.closed"));

    Ok(())
}

/// RAII guard to restore terminal mode on drop.
struct RawModeGuard;

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        let _ = crossterm::terminal::disable_raw_mode();
    }
}

/// Split a byte slice into a valid UTF-8 prefix string and the remaining bytes.
///
/// This handles the case where a multi-byte UTF-8 sequence is split across
/// two serial reads. The valid prefix is returned as a `&str`, and the
/// remaining incomplete bytes are returned for the next read.
pub(crate) fn split_utf8(bytes: &[u8]) -> (&str, &[u8]) {
    match std::str::from_utf8(bytes) {
        Ok(s) => (s, &[]),
        Err(e) => {
            let valid_up_to = e.valid_up_to();
            let valid = std::str::from_utf8(&bytes[..valid_up_to]).unwrap_or_default();
            (valid, &bytes[valid_up_to..])
        },
    }
}

/// Format monitor output with optional timestamps.
///
/// Handles `\r\n`, `\n`, and `\r` line endings uniformly.
/// Inserts `[HH:MM:SS.mmm]` at the beginning of each new line when enabled.
pub(crate) fn format_monitor_output(
    text: &str,
    timestamp: bool,
    at_line_start: &mut bool,
) -> String {
    if !timestamp {
        // Even without timestamps, normalize \r\n handling:
        // Raw mode requires \r\n for proper line feed + carriage return
        let mut out = String::with_capacity(text.len() * 2);
        for c in text.chars() {
            match c {
                '\n' => out.push_str("\r\n"),
                '\r' => {}, // Skip \r, we handle it via \n → \r\n
                _ => out.push(c),
            }
        }
        return out;
    }

    let mut out = String::with_capacity(text.len() + 128);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let total_secs = now.as_secs();
    let millis = now.subsec_millis();
    // Convert to HH:MM:SS (UTC-based, good enough for relative timing)
    let hours = (total_secs / 3600) % 24;
    let minutes = (total_secs / 60) % 60;
    let seconds = total_secs % 60;

    for c in text.chars() {
        match c {
            '\r' => {}, // Skip \r, we handle it via \n → \r\n
            '\n' => {
                out.push_str("\r\n");
                *at_line_start = true;
            },
            _ => {
                if *at_line_start {
                    use std::fmt::Write;
                    let _ = write!(
                        out,
                        "\x1b[90m[{hours:02}:{minutes:02}:{seconds:02}.{millis:03}]\x1b[0m "
                    );
                    *at_line_start = false;
                }
                out.push(c);
            },
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- split_utf8 ----

    #[test]
    fn test_split_utf8_valid_ascii() {
        let (valid, remainder) = split_utf8(b"hello world");
        assert_eq!(valid, "hello world");
        assert!(remainder.is_empty());
    }

    #[test]
    fn test_split_utf8_valid_multibyte() {
        let input = "你好世界".as_bytes();
        let (valid, remainder) = split_utf8(input);
        assert_eq!(valid, "你好世界");
        assert!(remainder.is_empty());
    }

    #[test]
    fn test_split_utf8_partial_multibyte() {
        // '你' is 3 bytes: 0xE4, 0xBD, 0xA0
        // Chop after first 2 bytes → incomplete
        let input = &[0xE4, 0xBD];
        let (valid, remainder) = split_utf8(input);
        assert_eq!(valid, "");
        assert_eq!(remainder, &[0xE4, 0xBD]);
    }

    #[test]
    fn test_split_utf8_mixed_valid_and_partial() {
        // "AB" + partial 3-byte char → valid: "AB", remainder: partial
        let mut input = Vec::new();
        input.extend_from_slice(b"AB");
        input.push(0xE4);
        input.push(0xBD);
        let (valid, remainder) = split_utf8(&input);
        assert_eq!(valid, "AB");
        assert_eq!(remainder, &[0xE4, 0xBD]);
    }

    #[test]
    fn test_split_utf8_empty() {
        let (valid, remainder) = split_utf8(b"");
        assert_eq!(valid, "");
        assert!(remainder.is_empty());
    }

    #[test]
    fn test_split_utf8_single_invalid_byte() {
        let input = &[0xFF];
        let (valid, remainder) = split_utf8(input);
        assert_eq!(valid, "");
        assert_eq!(remainder, &[0xFF]);
    }

    // ---- format_monitor_output ----

    #[test]
    fn test_format_output_no_timestamp_plain() {
        let mut at_line_start = true;
        let result = format_monitor_output("hello", false, &mut at_line_start);
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_format_output_no_timestamp_newline_normalization() {
        let mut at_line_start = true;
        let result = format_monitor_output("line1\nline2", false, &mut at_line_start);
        assert_eq!(result, "line1\r\nline2");
    }

    #[test]
    fn test_format_output_no_timestamp_crlf_normalization() {
        let mut at_line_start = true;
        // \r\n input → should emit single \r\n (not \r\r\n)
        let result = format_monitor_output("line1\r\nline2", false, &mut at_line_start);
        assert_eq!(result, "line1\r\nline2");
    }

    #[test]
    fn test_format_output_no_timestamp_standalone_cr() {
        let mut at_line_start = true;
        // Standalone \r should be stripped
        let result = format_monitor_output("abc\rdef", false, &mut at_line_start);
        assert_eq!(result, "abcdef");
    }

    #[test]
    fn test_format_output_with_timestamp_inserts_prefix() {
        let mut at_line_start = true;
        let result = format_monitor_output("hello", true, &mut at_line_start);
        // Should start with ANSI grey timestamp
        assert!(result.contains("\x1b[90m["));
        assert!(result.contains("]\x1b[0m hello"));
        assert!(!at_line_start);
    }

    #[test]
    fn test_format_output_with_timestamp_only_at_line_start() {
        let mut at_line_start = false;
        let result = format_monitor_output("continuation", true, &mut at_line_start);
        // No timestamp — we're mid-line
        assert!(!result.contains("\x1b[90m"));
        assert_eq!(result, "continuation");
    }

    #[test]
    fn test_format_output_with_timestamp_after_newline() {
        let mut at_line_start = true;
        let result = format_monitor_output("line1\nline2", true, &mut at_line_start);
        // Should have timestamp before line1 and set up for line2
        assert!(result.contains("line1\r\n"));
        // line2 should also get a timestamp since at_line_start was reset
        let parts: Vec<&str> = result.split("\r\n").collect();
        assert_eq!(parts.len(), 2);
        assert!(parts[0].contains("line1")); // First line has timestamp + text
        assert!(parts[1].contains("line2")); // Second line has timestamp + text
    }

    #[test]
    fn test_format_output_empty_string() {
        let mut at_line_start = true;
        let result = format_monitor_output("", false, &mut at_line_start);
        assert_eq!(result, "");
    }

    #[test]
    fn test_format_output_only_newlines() {
        let mut at_line_start = true;
        let result = format_monitor_output("\n\n", false, &mut at_line_start);
        assert_eq!(result, "\r\n\r\n");
    }
}
