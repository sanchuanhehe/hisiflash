//! Serial monitor command implementation.
//!
//! Dual-threaded serial monitor with keyboard input, timestamps, and log file support.

use anyhow::{Context, Result};
use console::style;
use hisiflash::MonitorSession;
use rust_i18n::t;
use std::io;
use std::io::IsTerminal;
use std::io::Write as _;
use std::path::PathBuf;

use crate::config::Config;
use crate::{Cli, clear_interrupted_flag, get_port, was_interrupted};

pub(crate) use hisiflash::{clean_monitor_text, drain_utf8_lossy, format_monitor_output};

fn contains_reset_evidence(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("boot.")
        || lower.contains("flash init")
        || lower.contains("verify_")
        || lower.contains("reset cause")
        || lower.contains("bootrom")
}

/// Run the serial monitor.
///
/// - Reader thread: serial → terminal (with optional timestamps and ANSI passthrough)
/// - Main thread: keyboard (crossterm raw mode) → serial
/// - Ctrl+C: graceful exit
/// - Ctrl+R: reset device (DTR/RTS toggle)
/// - Ctrl+T: toggle timestamp display
pub(crate) fn cmd_monitor(
    cli: &Cli,
    config: &mut Config,
    monitor_port_override: Option<&str>,
    monitor_baud: u32,
    timestamp: bool,
    clean_output: bool,
    log_file: Option<&PathBuf>,
) -> Result<()> {
    use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
    use crossterm::terminal;
    use std::io::Read as _;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    fn now_millis() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
    }

    // [Sensitive] Terminal alignment helper:
    // Serial devices may emit partial lines without trailing '\n'.
    // Before printing status/hint lines, always clear current terminal line and
    // write atomically under the same lock to avoid cursor-column drift.
    //
    // Channel policy note:
    // - Status lines are always written to stderr.
    // - In TTY mode, monitor stream is also written to stderr to keep one
    //   synchronized terminal channel and avoid inter-stream reordering.
    // - In non-TTY mode, monitor stream is written to stdout (separated), while
    //   status/hint lines remain on stderr for script-friendly contracts.
    fn print_status_line(term_lock: &Arc<Mutex<()>>, message: &str, tty_mode: bool) {
        if let Ok(_guard) = term_lock.lock() {
            if tty_mode {
                eprint!("\r\x1b[2K{message}\r\n");
            } else {
                eprintln!("{message}");
            }
            io::stderr().flush().ok();
        }
    }

    let port_name = if let Some(port) = monitor_port_override {
        port.to_string()
    } else {
        get_port(cli, config)?
    };
    let tty_mode = io::stdout().is_terminal() && io::stderr().is_terminal();
    // Design trade-off (explicit):
    // - TTY mode: prioritize alignment/readability by coalescing monitor data and
    //   status lines onto one channel (stderr).
    // - non-TTY mode: prioritize CLI stream contract by splitting channels:
    //   monitor data -> stdout, status/hints -> stderr.
    let term_lock = Arc::new(Mutex::new(()));

    print_status_line(
        &term_lock,
        &format!(
            "{} {}",
            style("📡").cyan(),
            t!(
                "monitor.opening",
                port = style(&port_name).green().to_string(),
                baud = monitor_baud
            )
        ),
        tty_mode,
    );
    print_status_line(
        &term_lock,
        &style(t!("monitor.exit_hint")).dim().to_string(),
        tty_mode,
    );

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
    let force_line_start = Arc::new(AtomicBool::new(false));
    let force_line_start_reader = force_line_start.clone();
    let term_lock_reader = term_lock.clone();
    let tty_mode_reader = tty_mode;
    let clean_output_reader = clean_output;
    let last_rx_millis = Arc::new(AtomicU64::new(0));
    let last_rx_millis_reader = last_rx_millis.clone();
    let reset_evidence_hits = Arc::new(AtomicU64::new(0));
    let reset_evidence_hits_reader = reset_evidence_hits.clone();
    let mut signal_interrupted = false;
    let mut user_requested_exit = false;

    // Open log file if specified
    let log_writer: Option<std::sync::Mutex<std::fs::File>> = if let Some(path) = log_file {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .with_context(|| format!("Failed to open log file: {}", path.display()))?;
        print_status_line(
            &term_lock,
            &format!(
                "{} {}",
                style("📝").cyan(),
                t!("monitor.logging", path = path.display().to_string())
            ),
            tty_mode,
        );
        Some(std::sync::Mutex::new(file))
    } else {
        None
    };

    // Reader thread: serial → terminal
    let reader_handle = std::thread::spawn(move || {
        let mut buf = [0u8; 1024];
        // [Sensitive] Must match actual terminal cursor state.
        // If this drifts from reality, Ctrl+R/Ctrl+T alignment will break.
        // Kept in sync by format_monitor_output() and force_line_start handling.
        let mut at_line_start = true;
        // Buffer for partial UTF-8 sequences that span read boundaries
        let mut utf8_buf: Vec<u8> = Vec::new();

        while running_reader.load(Ordering::Relaxed) {
            match serial_reader.read(&mut buf) {
                Ok(0) => {},
                Ok(n) => {
                    let data = &buf[..n];
                    last_rx_millis_reader.store(now_millis(), Ordering::Relaxed);

                    // Append to UTF-8 buffer for handling partial sequences
                    utf8_buf.extend_from_slice(data);

                    let decoded = drain_utf8_lossy(&mut utf8_buf);

                    if contains_reset_evidence(&decoded) {
                        reset_evidence_hits_reader.fetch_add(1, Ordering::Relaxed);
                    }

                    let display_text = if clean_output_reader {
                        clean_monitor_text(&decoded)
                    } else {
                        decoded
                    };

                    if !display_text.is_empty() {
                        // [Sensitive] Explicitly force next serial chunk to start at new line
                        // after status/hint output, regardless of device chunk boundaries.
                        if force_line_start_reader.swap(false, Ordering::Relaxed) {
                            if let Ok(_guard) = term_lock_reader.lock() {
                                if tty_mode_reader {
                                    eprint!("\r\n");
                                    io::stderr().flush().ok();
                                } else {
                                    print!("\r\n");
                                    io::stdout().flush().ok();
                                }
                            }
                            at_line_start = true;
                        }

                        // Write to log file (raw, no timestamps)
                        if let Some(ref log) = log_writer {
                            if let Ok(mut f) = log.lock() {
                                let _ = f.write_all(display_text.as_bytes());
                            }
                        }

                        // Process output with optional timestamps
                        let ts_enabled = show_timestamp_reader.load(Ordering::Relaxed);
                        let output =
                            format_monitor_output(&display_text, ts_enabled, &mut at_line_start);
                        if let Ok(_guard) = term_lock_reader.lock() {
                            if tty_mode_reader {
                                eprint!("{output}");
                                io::stderr().flush().ok();
                            } else {
                                print!("{output}");
                                io::stdout().flush().ok();
                            }
                        }
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
                        // [Sensitive] Request reader-side line realignment before next chunk.
                        force_line_start.store(true, Ordering::Relaxed);
                        print_status_line(
                            &term_lock,
                            &format!("{} {}", style("🔄").cyan(), t!("monitor.resetting")),
                            tty_mode,
                        );

                        let before_rx = last_rx_millis.load(Ordering::Relaxed);
                        let before_evidence_hits = reset_evidence_hits.load(Ordering::Relaxed);
                        let reset_result = (|| -> Result<()> {
                            serial_writer.set_data_terminal_ready(false)?;
                            serial_writer.set_request_to_send(false)?;
                            std::thread::sleep(Duration::from_millis(100));
                            serial_writer.set_data_terminal_ready(true)?;
                            serial_writer.set_request_to_send(true)?;
                            std::thread::sleep(Duration::from_millis(100));
                            serial_writer.set_data_terminal_ready(false)?;
                            Ok(())
                        })();

                        match reset_result {
                            Ok(()) => {
                                const VERIFY_TIMEOUT_MS: u64 = 2000;
                                const SILENCE_GAP_MS: u64 = 120;
                                print_status_line(
                                    &term_lock,
                                    &format!(
                                        "{} {}",
                                        style("✓").green(),
                                        t!("monitor.reset_signal_sent")
                                    ),
                                    tty_mode,
                                );

                                let start = now_millis();
                                let mut evidence_observed = false;
                                let mut weak_observed = false;
                                let mut saw_silence_gap = false;
                                let mut last_seen_rx = before_rx;

                                while now_millis().saturating_sub(start) < VERIFY_TIMEOUT_MS {
                                    let current_hits = reset_evidence_hits.load(Ordering::Relaxed);
                                    if current_hits > before_evidence_hits {
                                        evidence_observed = true;
                                        break;
                                    }

                                    let current_rx = last_rx_millis.load(Ordering::Relaxed);
                                    if now_millis().saturating_sub(current_rx) >= SILENCE_GAP_MS {
                                        saw_silence_gap = true;
                                    }

                                    if current_rx > last_seen_rx {
                                        if saw_silence_gap {
                                            weak_observed = true;
                                            break;
                                        }
                                        last_seen_rx = current_rx;
                                    }

                                    std::thread::sleep(Duration::from_millis(50));
                                }

                                let mut show_flow_control_hint = false;
                                if evidence_observed {
                                    print_status_line(
                                        &term_lock,
                                        &format!(
                                            "{} {}",
                                            style("✓").green(),
                                            t!("monitor.reset_evidence_observed")
                                        ),
                                        tty_mode,
                                    );
                                } else if weak_observed {
                                    show_flow_control_hint = true;
                                    print_status_line(
                                        &term_lock,
                                        &format!(
                                            "{} {}",
                                            style("⚠").yellow(),
                                            t!(
                                                "monitor.reset_evidence_weak",
                                                timeout_ms = VERIFY_TIMEOUT_MS
                                            )
                                        ),
                                        tty_mode,
                                    );
                                } else {
                                    show_flow_control_hint = true;
                                    print_status_line(
                                        &term_lock,
                                        &format!(
                                            "{} {}",
                                            style("⚠").yellow(),
                                            t!(
                                                "monitor.reset_evidence_unconfirmed",
                                                timeout_ms = VERIFY_TIMEOUT_MS
                                            )
                                        ),
                                        tty_mode,
                                    );
                                }
                                if show_flow_control_hint {
                                    print_status_line(
                                        &term_lock,
                                        t!("monitor.reset_flow_control_hint").as_ref(),
                                        tty_mode,
                                    );
                                }
                                // [Sensitive] Ensure subsequent serial bytes start on clean line.
                                force_line_start.store(true, Ordering::Relaxed);
                            },
                            Err(err) => {
                                print_status_line(
                                    &term_lock,
                                    &format!(
                                        "{} {}",
                                        style("⚠").yellow(),
                                        t!("monitor.reset_failed", error = err.to_string())
                                    ),
                                    tty_mode,
                                );
                                print_status_line(
                                    &term_lock,
                                    t!("monitor.reset_flow_control_hint").as_ref(),
                                    tty_mode,
                                );
                                force_line_start.store(true, Ordering::Relaxed);
                            },
                        }
                    },
                    // Ctrl+T: toggle timestamp
                    (KeyCode::Char('t'), KeyModifiers::CONTROL) => {
                        let current = show_timestamp.load(Ordering::Relaxed);
                        show_timestamp.store(!current, Ordering::Relaxed);
                        force_line_start.store(true, Ordering::Relaxed);
                        let state = if current {
                            t!("monitor.timestamp_off")
                        } else {
                            t!("monitor.timestamp_on")
                        };
                        print_status_line(
                            &term_lock,
                            &format!("{} {state}", style("⏱").cyan()),
                            tty_mode,
                        );
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
    print_status_line(
        &term_lock,
        &format!("{} {}", style("👋").cyan(), t!("monitor.closed")),
        tty_mode,
    );

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
        // Standalone \r should become newline
        let result = format_monitor_output("abc\rdef", false, &mut at_line_start);
        assert_eq!(result, "abc\r\ndef");
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

    #[test]
    fn test_contains_reset_evidence_boot_pattern() {
        assert!(contains_reset_evidence("boot.\n"));
    }

    #[test]
    fn test_contains_reset_evidence_flash_init_pattern() {
        assert!(contains_reset_evidence("Flash Init Fail! ret = 0x80001341"));
    }

    #[test]
    fn test_contains_reset_evidence_verify_pattern() {
        assert!(contains_reset_evidence(
            "verify_public_rootkey secure verify disable!"
        ));
    }

    #[test]
    fn test_contains_reset_evidence_negative_case() {
        assert!(!contains_reset_evidence("normal runtime log line"));
    }
}
