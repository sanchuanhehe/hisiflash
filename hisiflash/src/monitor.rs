//! Native serial monitor primitives.

#[cfg(feature = "native")]
use std::io::Write as _;

/// A native monitor session wrapping a serial port connection.
#[cfg(feature = "native")]
pub struct MonitorSession {
    port: Box<dyn serialport::SerialPort>,
}

#[cfg(feature = "native")]
impl MonitorSession {
    /// Open a monitor session on the specified port and baud rate.
    pub fn open(port_name: &str, baud_rate: u32) -> crate::Result<Self> {
        let port = serialport::new(port_name, baud_rate)
            .timeout(std::time::Duration::from_millis(50))
            .open()?;
        Ok(Self { port })
    }

    /// Create a cloned reader handle for a background read loop.
    pub fn try_clone_reader(&self) -> crate::Result<Box<dyn serialport::SerialPort>> {
        Ok(self.port.try_clone()?)
    }

    /// Write raw bytes to the serial connection.
    pub fn write_bytes(&mut self, data: &[u8]) -> crate::Result<()> {
        self.port.write_all(data)?;
        Ok(())
    }

    /// Set DTR line state.
    pub fn set_data_terminal_ready(&mut self, enabled: bool) -> crate::Result<()> {
        self.port.write_data_terminal_ready(enabled)?;
        Ok(())
    }

    /// Set RTS line state.
    pub fn set_request_to_send(&mut self, enabled: bool) -> crate::Result<()> {
        self.port.write_request_to_send(enabled)?;
        Ok(())
    }
}

#[cfg(not(feature = "native"))]
/// A placeholder monitor session for non-native targets.
pub struct MonitorSession;

#[cfg(not(feature = "native"))]
impl MonitorSession {
    /// Open monitor session on non-native targets.
    pub fn open(_port_name: &str, _baud_rate: u32) -> crate::Result<Self> {
        Err(crate::Error::Unsupported(
            "Serial monitor is only available with native feature".to_string(),
        ))
    }
}

/// Split a byte slice into a valid UTF-8 prefix and the remaining bytes.
pub fn split_utf8(bytes: &[u8]) -> (&str, &[u8]) {
    match std::str::from_utf8(bytes) {
        Ok(s) => (s, &[]),
        Err(e) => {
            let valid_up_to = e.valid_up_to();
            let valid = std::str::from_utf8(&bytes[..valid_up_to]).unwrap_or_default();
            (valid, &bytes[valid_up_to..])
        },
    }
}

/// Drain buffered bytes into displayable UTF-8 text without stalling on invalid bytes.
///
/// - Valid UTF-8 is emitted as-is.
/// - Invalid byte sequences emit the replacement char `�` and continue.
/// - Incomplete UTF-8 suffix is kept in `buffer` for the next read.
pub fn drain_utf8_lossy(buffer: &mut Vec<u8>) -> String {
    let mut output = String::new();

    loop {
        match std::str::from_utf8(buffer) {
            Ok(valid) => {
                output.push_str(valid);
                buffer.clear();
                break;
            },
            Err(err) => {
                let valid_up_to = err.valid_up_to();
                if valid_up_to > 0 {
                    if let Ok(valid) = std::str::from_utf8(&buffer[..valid_up_to]) {
                        output.push_str(valid);
                    }
                }

                match err.error_len() {
                    Some(invalid_len) => {
                        output.push('�');
                        let drain_to = valid_up_to.saturating_add(invalid_len).min(buffer.len());
                        buffer.drain(..drain_to);
                    },
                    None => {
                        if valid_up_to > 0 {
                            buffer.drain(..valid_up_to);
                        }
                        break;
                    },
                }
            },
        }
    }

    output
}

/// Filter non-printable control characters for cleaner monitor output.
///
/// Keeps:\n, \t and printable Unicode chars.
/// Converts carriage returns (\r) to newlines (\n).
/// Drops other control characters.
pub fn clean_monitor_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '\n' | '\t' => out.push(ch),
            '\r' => out.push('\n'),
            _ if ch.is_control() => {},
            _ => out.push(ch),
        }
    }
    out
}

/// Format monitor output with optional timestamps.
pub fn format_monitor_output(text: &str, timestamp: bool, at_line_start: &mut bool) -> String {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");

    if !timestamp {
        let mut out = String::with_capacity(normalized.len() * 2);
        for c in normalized.chars() {
            match c {
                '\n' => {
                    out.push_str("\r\n");
                    *at_line_start = true;
                },
                _ => {
                    out.push(c);
                    *at_line_start = false;
                },
            }
        }
        return out;
    }

    let mut out = String::with_capacity(normalized.len() + 128);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let total_secs = now.as_secs();
    let millis = now.subsec_millis();
    let hours = (total_secs / 3600) % 24;
    let minutes = (total_secs / 60) % 60;
    let seconds = total_secs % 60;

    for c in normalized.chars() {
        match c {
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
    use super::{clean_monitor_text, drain_utf8_lossy, format_monitor_output};

    #[test]
    fn test_drain_utf8_lossy_replaces_invalid_bytes_and_continues() {
        let mut buf = vec![0xFF, b'A', 0xFE, b'B'];
        let out = drain_utf8_lossy(&mut buf);
        assert_eq!(out, "�A�B");
        assert!(buf.is_empty());
    }

    #[test]
    fn test_drain_utf8_lossy_keeps_incomplete_suffix() {
        let mut buf = vec![0xE4, 0xBD]; // incomplete UTF-8 for '你'
        let out = drain_utf8_lossy(&mut buf);
        assert_eq!(out, "");
        assert_eq!(buf, vec![0xE4, 0xBD]);

        buf.push(0xA0);
        let out2 = drain_utf8_lossy(&mut buf);
        assert_eq!(out2, "你");
        assert!(buf.is_empty());
    }

    #[test]
    fn test_clean_monitor_text_filters_control_chars() {
        let text = "A\x07B\x1BC\tD\nE\rF";
        let cleaned = clean_monitor_text(text);
        assert_eq!(cleaned, "ABC\tD\nE\nF");
    }

    #[test]
    fn test_format_output_normalizes_standalone_cr_to_newline() {
        let mut at_line_start = true;
        let result = format_monitor_output("abc\rdef", false, &mut at_line_start);
        assert_eq!(result, "abc\r\ndef");
    }

    #[test]
    fn test_format_output_no_timestamp_updates_line_state() {
        let mut at_line_start = true;
        let result = format_monitor_output("abc", false, &mut at_line_start);
        assert_eq!(result, "abc");
        assert!(!at_line_start);

        let result2 = format_monitor_output("\n", false, &mut at_line_start);
        assert_eq!(result2, "\r\n");
        assert!(at_line_start);
    }
}
