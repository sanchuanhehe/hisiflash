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

/// Format monitor output with optional timestamps.
pub fn format_monitor_output(text: &str, timestamp: bool, at_line_start: &mut bool) -> String {
    if !timestamp {
        let mut out = String::with_capacity(text.len() * 2);
        for c in text.chars() {
            match c {
                '\n' => out.push_str("\r\n"),
                '\r' => {},
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
    let hours = (total_secs / 3600) % 24;
    let minutes = (total_secs / 60) % 60;
    let seconds = total_secs % 60;

    for c in text.chars() {
        match c {
            '\r' => {},
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
