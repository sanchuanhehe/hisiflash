//! Serial monitor command implementation.
//!
//! Dual-threaded serial monitor with keyboard input, timestamps, and log file support.

use anyhow::{Context, Result};
use console::style;
use hisiflash::MonitorSession;
use rust_i18n::t;
use std::io;
use std::io::Write as _;
use std::path::PathBuf;

use crate::config::Config;
use crate::{Cli, clear_interrupted_flag, get_port, was_interrupted};

pub(crate) use hisiflash::{drain_utf8_lossy, format_monitor_output};

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
    let serial = MonitorSession::open(&port_name, monitor_baud)
        .with_context(|| t!("error.open_port", port = port_name.clone()))?;

    // Clone for the reader thread
    let mut serial_reader = serial
        .try_clone_reader()
        .context(t!("error.serial_error").to_string())?;
    let mut serial_writer = serial;

    // Shared state
    let running = Arc::new(AtomicBool::new(true));
    let running_reader = running.clone();
    let show_timestamp = Arc::new(AtomicBool::new(timestamp));
    let show_timestamp_reader = show_timestamp.clone();
    let mut signal_interrupted = false;
    let mut user_requested_exit = false;

    // Open log file if specified
    let log_writer: Option<std::sync::Mutex<std::fs::File>> = if let Some(path) = log_file {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .with_context(|| format!("Failed to open log file: {}", path.display()))?;
        eprintln!(
            "{} {}",
            style("📝").cyan(),
            t!("monitor.logging", path = path.display().to_string())
        );
        Some(std::sync::Mutex::new(file))
    } else {
        None
    };

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

                    let decoded = drain_utf8_lossy(&mut utf8_buf);

                    if !decoded.is_empty() {
                        // Write to log file (raw, no timestamps)
                        if let Some(ref log) = log_writer {
                            if let Ok(mut f) = log.lock() {
                                let _ = f.write_all(decoded.as_bytes());
                            }
                        }

                        // Process output with optional timestamps
                        let ts_enabled = show_timestamp_reader.load(Ordering::Relaxed);
                        let output =
                            format_monitor_output(&decoded, ts_enabled, &mut at_line_start);
                        print!("{output}");
                        io::stdout().flush().ok();
                    }
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
        if was_interrupted() {
            signal_interrupted = true;
            running.store(false, Ordering::Relaxed);
            break;
        }

        // Poll for keyboard events with timeout
        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(KeyEvent {
                code, modifiers, ..
            }) = event::read()?
            {
                match (code, modifiers) {
                    // Ctrl+C: exit
                    (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                        user_requested_exit = true;
                        running.store(false, Ordering::Relaxed);
                        break;
                    },
                    // Ctrl+R: reset device via DTR/RTS toggle
                    (KeyCode::Char('r'), KeyModifiers::CONTROL) => {
                        eprintln!("\r\n{} {}", style("🔄").cyan(), t!("monitor.resetting"));
                        let _ = serial_writer.set_data_terminal_ready(false);
                        let _ = serial_writer.set_request_to_send(false);
                        std::thread::sleep(Duration::from_millis(100));
                        let _ = serial_writer.set_data_terminal_ready(true);
                        let _ = serial_writer.set_request_to_send(true);
                        std::thread::sleep(Duration::from_millis(100));
                        let _ = serial_writer.set_data_terminal_ready(false);
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
                        let _ = serial_writer.write_bytes(b"\r\n");
                    },
                    // Regular character
                    (KeyCode::Char(c), KeyModifiers::NONE | KeyModifiers::SHIFT) => {
                        let mut buf = [0u8; 4];
                        let bytes = c.encode_utf8(&mut buf);
                        let _ = serial_writer.write_bytes(bytes.as_bytes());
                    },
                    // Backspace
                    (KeyCode::Backspace, _) => {
                        let _ = serial_writer.write_bytes(&[0x08]);
                    },
                    // Tab
                    (KeyCode::Tab, _) => {
                        let _ = serial_writer.write_bytes(&[0x09]);
                    },
                    // Escape
                    (KeyCode::Esc, _) => {
                        let _ = serial_writer.write_bytes(&[0x1B]);
                    },
                    _ => {},
                }
            }
        }
    }

    // Wait for reader thread to finish
    let _ = reader_handle.join();
    eprintln!("\r\n{} {}", style("👋").cyan(), t!("monitor.closed"));

    if signal_interrupted || was_interrupted() || user_requested_exit {
        clear_interrupted_flag();
        Ok(())
    } else {
        Ok(())
    }
}

/// RAII guard to restore terminal mode on drop.
struct RawModeGuard;

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        let _ = crossterm::terminal::disable_raw_mode();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hisiflash::split_utf8;

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
