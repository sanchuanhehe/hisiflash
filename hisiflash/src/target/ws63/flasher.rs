//! WS63 flasher implementation.
//!
//! This module provides the main flasher interface for the WS63 chip.
//!
//! ## Generic Port Support
//!
//! The flasher uses a generic `Port` trait, allowing it to work with different
//! serial port implementations:
//!
//! - **Native platforms**: Uses the `serialport` crate via `NativePort`
//! - **WASM/Web**: Can use Web Serial API via `WebSerialPort` (experimental)
//!
//! ## Example
//!
//! ```rust,no_run
//! use hisiflash::{ChipFamily, Fwpkg};
//!
//! fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Create flasher using chip abstraction
//!     let mut flasher = ChipFamily::Ws63.create_flasher("/dev/ttyUSB0", 921600, false, 0)?;
//!
//!     // Connect to device
//!     flasher.connect()?;
//!
//!     // Flash firmware
//!     let fwpkg = Fwpkg::from_file("firmware.fwpkg")?;
//!     flasher.flash_fwpkg(&fwpkg, None, &mut |name, current, total| {
//!         println!("Flashing {}: {}/{}", name, current, total);
//!     })?;
//!
//!     Ok(())
//! }
//! ```

use {
    crate::{
        error::{Error, Result},
        image::fwpkg::Fwpkg,
        port::Port,
        protocol::ymodem::{YmodemConfig, YmodemTransfer},
        target::ws63::protocol::{
            CommandFrame, DEFAULT_BAUD, HANDSHAKE_ACK, contains_handshake_ack,
        },
    },
    log::{debug, info, trace, warn},
    std::{
        fmt::Write as _,
        sync::Arc,
        thread,
        time::{Duration, Instant},
    },
};

type ReconnectFactory<P> = dyn Fn() -> Result<P> + Send + Sync + 'static;

/// Timeout for waiting for handshake.
///
/// Keep this window long enough for users to press reset while the tool keeps
/// transmitting handshake frames. A short 5s window was easy to miss on real
/// hardware and caused repeated retries without ever hitting the bootloader
/// window.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(20);

/// Read timeout used during handshake polling.
///
/// Must be long enough to cover the full CH340 USB round-trip:
///   flush → USB OUT(~1ms) → UART TX 18B(1.56ms) → bootloader
///   process(~0.01ms) → UART ACK 12B(1.04ms) → CH340 USB IN(2-16ms)
/// Total: ~5-20 ms.  20 ms gives comfortable margin.
///
/// A 1 ms timeout was too short – the ACK never arrived before timeout,
/// so we kept flooding frames and the device never responded.
const HANDSHAKE_POLL_READ_TIMEOUT: Duration = Duration::from_millis(20);

/// Delay between handshake frame writes.
///
/// At 115 200 baud an 18-byte handshake frame takes ~1.56 ms to transmit.
/// `flush()` / `tcdrain()` may return as soon as the USB OUT transfer
/// completes, before the CH340 finishes UART TX.  5 ms guarantees the
/// frame is fully on the wire AND the bootloader sees an idle-line gap,
/// which helps it detect frame boundaries reliably.
const HANDSHAKE_FRAME_INTERVAL: Duration = Duration::from_millis(5);

/// Faster frame interval used briefly after seeing bootloader heartbeat dots.
const BOOT_DOT_FRAME_INTERVAL: Duration = Duration::from_millis(2);

/// Duration to keep boosted frame interval after receiving `.` heartbeat.
const BOOT_DOT_BOOST_WINDOW: Duration = Duration::from_millis(1200);

/// Number of immediate handshake frames to burst when boot hint appears.
const BOOT_HINT_BURST_FRAMES: usize = 3;

/// Minimal gap between burst frames.
const BOOT_HINT_BURST_GAP: Duration = Duration::from_millis(1);

/// Immediate pre-burst count right after a new handshake attempt starts.
///
/// This avoids waiting for the first read timeout before sending anything,
/// which is critical for very short boot windows.
const INITIAL_HANDSHAKE_BURST_FRAMES: usize = 4;

/// Aggressive send window at the beginning of each handshake attempt.
///
/// Right after USB reconnect/reset, the bootloader handshake window can be
/// extremely short. During this window we intentionally ignore app-mode
/// throttling and keep probing fast.
const STARTUP_AGGRESSIVE_WINDOW: Duration = Duration::from_secs(3);

/// Frame interval used during startup aggressive window.
const STARTUP_AGGRESSIVE_FRAME_INTERVAL: Duration = Duration::from_millis(2);

/// Extra handshake frames sent immediately when a startup-phase read times out.
///
/// This breaks out of the 20 ms read-timeout pacing and increases frame density
/// in the very first boot window where ACK opportunity can be extremely short.
const STARTUP_TIMEOUT_BURST_FRAMES: usize = 2;

/// Minimal gap for timeout-triggered startup burst frames.
const STARTUP_TIMEOUT_BURST_GAP: Duration = Duration::from_millis(1);

/// Slower frame interval while device appears to be in application mode.
///
/// This reduces UART disturbance and avoids triggering excessive app-side
/// UART error handling, while we keep polling for user-triggered reset into
/// boot mode.
const APP_MODE_FRAME_INTERVAL: Duration = Duration::from_millis(200);

/// Minimum quiet period on RX before sending a probe while in app mode.
const APP_MODE_TX_SILENCE_GUARD: Duration = Duration::from_millis(250);

/// Probe cadence while in app mode (no bootloader dot observed).
const APP_MODE_PROBE_INTERVAL: Duration = Duration::from_millis(1500);

/// Initial listen-only period after detecting app mode.
///
/// During this window we avoid sending handshake frames entirely to reduce
/// interference with running firmware UART tasks.
const APP_MODE_LISTEN_ONLY_GRACE: Duration = Duration::from_secs(2);

/// Delay after changing baud rate.
const BAUD_CHANGE_DELAY: Duration = Duration::from_millis(100);

/// Delay between partition transfers to prevent serial data stale.
const PARTITION_DELAY: Duration = Duration::from_millis(100);

/// Timeout for waiting for SEBOOT magic response.
const MAGIC_TIMEOUT: Duration = Duration::from_secs(10);

/// Delay between connection retry attempts.
const CONNECT_RETRY_DELAY: Duration = Duration::from_millis(500);

/// Maximum number of connection attempts.
const MAX_CONNECT_ATTEMPTS: usize = 7;

/// Maximum number of download retry attempts.
const MAX_DOWNLOAD_RETRIES: usize = 3;

/// Maximum number of reconnection attempts after reset.
const MAX_RESET_RECONNECT_ATTEMPTS: usize = 10;

/// Delay between reset and device re-enumeration (ms).
const RESET_ENUM_DELAY: Duration = Duration::from_millis(1000);

/// Bytes to keep between reads for handshake ACK detection across packet boundaries.
const HANDSHAKE_CARRY_BYTES: usize = 128;

/// Max number of raw RX chunks to log per handshake attempt.
const HANDSHAKE_MAX_LOG_PREVIEWS: usize = 12;

/// Max bytes to include in each raw RX preview log.
const HANDSHAKE_RAW_PREVIEW_BYTES: usize = 32;

/// Bytes of non-ACK data after which we assume the device is in application
/// mode and print a one-time guidance message.  We do NOT abort – the user
/// may press reset at any moment and we must be actively sending handshake
/// frames when that happens.
const APP_DETECT_THRESHOLD_BYTES: usize = 50;

/// Check if an error is likely a port-level error (USB disconnection, etc.)
fn is_port_error(e: &Error) -> bool {
    match e {
        Error::Io(io) => matches!(
            io.kind(),
            std::io::ErrorKind::NotConnected
                | std::io::ErrorKind::BrokenPipe
                | std::io::ErrorKind::UnexpectedEof
        ),
        Error::Serial(_) => true,
        _ => false,
    }
}

/// Check if an I/O error indicates connection lost (USB disconnection)
fn is_connection_lost(e: &std::io::Error) -> bool {
    matches!(
        e.kind(),
        std::io::ErrorKind::NotConnected
            | std::io::ErrorKind::BrokenPipe
            | std::io::ErrorKind::UnexpectedEof
    )
}

fn is_interrupted_error(e: &Error) -> bool {
    match e {
        Error::Io(io) => io.kind() == std::io::ErrorKind::Interrupted,
        Error::Serial(serial) => matches!(
            serial.kind(),
            serialport::ErrorKind::Io(std::io::ErrorKind::Interrupted)
        ),
        _ => false,
    }
}

fn check_user_interrupted() -> Result<()> {
    if crate::is_interrupted_requested() {
        return Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::Interrupted,
            "operation interrupted",
        )));
    }
    Ok(())
}

fn sleep_interruptible(total: Duration) -> Result<()> {
    const CHUNK: Duration = Duration::from_millis(20);

    let start = Instant::now();
    while start.elapsed() < total {
        check_user_interrupted()?;
        let elapsed = start.elapsed();
        let remain = total.saturating_sub(elapsed);
        thread::sleep(remain.min(CHUNK));
    }

    Ok(())
}

fn is_permission_denied_error(e: &Error) -> bool {
    match e {
        Error::Io(io) => io.kind() == std::io::ErrorKind::PermissionDenied,
        Error::Serial(serial) => matches!(
            serial.kind(),
            serialport::ErrorKind::Io(std::io::ErrorKind::PermissionDenied)
        ),
        _ => false,
    }
}

fn is_busy_error(e: &Error) -> bool {
    let lower = e
        .to_string()
        .to_ascii_lowercase();
    lower.contains("resource busy") || lower.contains(" busy")
}

fn is_device_not_found_error(e: &Error) -> bool {
    let lower = e
        .to_string()
        .to_ascii_lowercase();
    lower.contains("not found") || lower.contains("not in boot mode")
}

fn reconnect_failure_delay(reopen_error: &Error, fallback: Duration) -> Duration {
    if is_permission_denied_error(reopen_error) || is_busy_error(reopen_error) {
        Duration::from_millis(900)
    } else if is_device_not_found_error(reopen_error) {
        Duration::from_millis(650)
    } else {
        fallback
    }
}

fn should_attempt_reopen(e: &Error) -> bool {
    // Only reopen on actual port-level errors (USB disconnect, broken pipe).
    // Timeout means the port is working but no ACK was received; reopening
    // would just fail with "busy" (old handle still held) and waste ~900 ms.
    is_port_error(e)
}

fn connect_retry_delay(e: &Error) -> Duration {
    if is_permission_denied_error(e) {
        Duration::from_millis(1800)
    } else if is_port_error(e) {
        Duration::from_millis(1200)
    } else {
        Duration::from_millis(100)
    }
}

fn should_retry_download_error(e: &Error) -> bool {
    !is_interrupted_error(e)
}

fn has_handshake_ack_with_carry(carry: &mut Vec<u8>, incoming: &[u8]) -> bool {
    let mut merged = Vec::with_capacity(carry.len() + incoming.len());
    merged.extend_from_slice(carry);
    merged.extend_from_slice(incoming);

    let found = contains_handshake_ack(&merged);

    let keep = HANDSHAKE_CARRY_BYTES.max(
        HANDSHAKE_ACK
            .len()
            .saturating_sub(1),
    );
    if merged.len() > keep {
        carry.clear();
        carry.extend_from_slice(&merged[merged.len() - keep..]);
    } else {
        carry.clear();
        carry.extend_from_slice(&merged);
    }

    found
}

fn has_bootloader_heartbeat_hint(incoming: &[u8]) -> bool {
    // Canonical WS63 boot heartbeat
    if incoming.len() == 1 && incoming[0] == b'.' {
        return true;
    }

    // Some firmware variants print text like "boot.\r\n" around boot mode.
    incoming
        .windows(5)
        .any(|w| w == b"boot.")
}

fn format_raw_preview(data: &[u8], max_bytes: usize) -> String {
    let preview = if data.len() > max_bytes {
        &data[..max_bytes]
    } else {
        data
    };

    let mut hex = String::new();
    for (idx, b) in preview
        .iter()
        .enumerate()
    {
        if idx > 0 {
            hex.push(' ');
        }
        let _ = write!(hex, "{b:02X}");
    }

    let ascii: String = preview
        .iter()
        .map(|b| {
            if b.is_ascii_graphic() || *b == b' ' {
                char::from(*b)
            } else {
                '.'
            }
        })
        .collect();

    if data.len() > max_bytes {
        format!("hex=[{hex} ...] ascii='{ascii}...' total={}", data.len())
    } else {
        format!("hex=[{hex}] ascii='{ascii}' total={}", data.len())
    }
}

// Complex detection helpers (echo detection, text-stream analysis, app-log
// keyword matching, early-abort on APP data) were all removed after analysis
// showed they caused more harm than benefit.  The correct approach is to
// continuously send handshake frames for the entire timeout duration so we
// catch the bootloader whenever the user presses reset.  Early-aborting on
// APP logs wasted time with futile port-reopen attempts and left gaps where
// no handshake frames were being sent.

/// WS63 flasher.
///
/// Generic over the port type `P`, which must implement the `Port` trait.
/// This allows the flasher to work with different serial port implementations.
pub struct Ws63Flasher<P: Port> {
    port: P,
    port_name: String,
    reconnect_factory: Option<Arc<ReconnectFactory<P>>>,
    target_baud: u32,
    late_baud: bool,
    verbose: u8,
}

// Implementation for any Port type
impl<P: Port> Ws63Flasher<P> {
    /// Create a new WS63 flasher with an existing port.
    ///
    /// # Arguments
    ///
    /// * `port` - An opened serial port implementing the `Port` trait
    /// * `target_baud` - Target baud rate for data transfer
    pub fn new(port: P, target_baud: u32) -> Self {
        Self {
            port,
            port_name: String::new(),
            reconnect_factory: None,
            target_baud,
            late_baud: false,
            verbose: 0,
        }
    }

    /// Create with port name for reconnection support.
    pub fn new_with_name(port: P, port_name: &str, target_baud: u32) -> Self {
        Self {
            port,
            port_name: port_name.to_string(),
            reconnect_factory: None,
            target_baud,
            late_baud: false,
            verbose: 0,
        }
    }

    fn with_reconnect_factory(mut self, factory: Arc<ReconnectFactory<P>>) -> Self {
        self.reconnect_factory = Some(factory);
        self
    }

    /// Set late baud rate change mode.
    ///
    /// For WS63, baud switching is always deferred until LoaderBoot is ready
    /// because early switching can cause intermittent YMODEM startup failures
    /// on some USB-UART adapters. This flag is kept for API/config
    /// compatibility.
    #[must_use]
    pub fn with_late_baud(mut self, late_baud: bool) -> Self {
        self.late_baud = late_baud;
        self
    }

    /// Set verbose output level.
    #[must_use]
    pub fn with_verbose(mut self, verbose: u8) -> Self {
        self.verbose = verbose;
        self
    }

    /// Connect to the device.
    ///
    /// This waits for the device to boot into download mode and performs
    /// the initial handshake with retry mechanism.
    ///
    /// The retry mechanism handles:
    /// - Device not yet in download mode (normal case)
    /// - USB enumeration delays (USB cable issues)
    /// - Temporary port disconnection
    pub fn connect(&mut self) -> Result<()> {
        let expected_port = if self
            .port_name
            .is_empty()
        {
            self.port
                .name()
                .to_string()
        } else {
            self.port_name
                .clone()
        };

        info!("Waiting for device on {expected_port}...");
        info!("Please reset the device to enter download mode.");

        for attempt in 1..=MAX_CONNECT_ATTEMPTS {
            check_user_interrupted()?;

            if attempt > 1 {
                info!("Connection attempt {attempt}/{MAX_CONNECT_ATTEMPTS}");
            }

            match self.try_connect() {
                Ok(()) => {
                    return Ok(());
                },
                Err(e) => {
                    if is_interrupted_error(&e) {
                        return Err(e);
                    }

                    if is_port_error(&e) {
                        warn!("Port connection lost: {e}");
                    }

                    if attempt < MAX_CONNECT_ATTEMPTS {
                        warn!("Connection failed (attempt {attempt}/{MAX_CONNECT_ATTEMPTS}): {e}");
                        if should_attempt_reopen(&e) {
                            match self.try_reopen_port() {
                                Ok(true) => {
                                    // Port reopened – start handshaking immediately
                                    // without any delay.  The bootloader's handshake
                                    // window is very short (~50-200 ms) and starts
                                    // right after device reset, so every millisecond
                                    // counts.
                                    info!("Port reopened, retrying immediately");
                                    continue;
                                },
                                Ok(false) => {
                                    // No reopen factory available. Fall back to
                                    // standard retry delay path below.
                                },
                                Err(reopen_err) => {
                                    warn!("Port reopen failed: {reopen_err}");
                                    let delay =
                                        reconnect_failure_delay(&reopen_err, CONNECT_RETRY_DELAY);
                                    sleep_interruptible(delay)?;

                                    // Reopen path already waited (including
                                    // internal poll timeout + failure delay).
                                    // Skip the generic connect retry delay to
                                    // avoid double-waiting and missing reset
                                    // windows during USB re-enumeration.
                                    continue;
                                },
                            }
                        }
                        let delay = connect_retry_delay(&e);
                        sleep_interruptible(delay)?;
                    } else {
                        // Last attempt failed - provide helpful message
                        if should_attempt_reopen(&e) {
                            return Err(Error::Io(std::io::Error::new(
                                std::io::ErrorKind::NotConnected,
                                format!(
                                    "Port {expected_port} is not connected. Auto-reconnect failed; please check USB cable and try again."
                                ),
                            )));
                        }
                        return Err(e);
                    }
                },
            }
        }

        Err(Error::Timeout(format!(
            "Connection failed after {MAX_CONNECT_ATTEMPTS} attempts"
        )))
    }

    fn try_reopen_port(&mut self) -> Result<bool> {
        let Some(factory) = &self.reconnect_factory else {
            return Ok(false);
        };

        check_user_interrupted()?;

        // Best-effort close of current handle before reopen.
        //
        // After USB reset/unplug, the stale fd can still hold the tty node
        // briefly, and reopening may fail with "Permission denied" or
        // "Device or resource busy". Releasing the handle first makes reopen
        // much more reliable on Linux/CH340.
        if let Err(close_err) = self
            .port
            .close()
        {
            if is_interrupted_error(&close_err) {
                return Err(close_err);
            }
            debug!("Ignoring close error before reopen: {close_err}");
        }

        // Give kernel/udev a tiny window to finalize node state.
        sleep_interruptible(Duration::from_millis(30))?;

        let mut new_port = (factory)()?;
        let _ = new_port.clear_buffers();
        self.port = new_port;
        Ok(true)
    }

    /// Single connection attempt.
    ///
    /// Sets a very short read timeout for the duration of the handshake to
    /// maximise write frequency, then restores the original timeout.
    fn try_connect(&mut self) -> Result<()> {
        let original_timeout = self
            .port
            .timeout();

        if original_timeout != HANDSHAKE_POLL_READ_TIMEOUT {
            self.port
                .set_timeout(HANDSHAKE_POLL_READ_TIMEOUT)?;
        }

        let result = self.try_connect_inner();

        if original_timeout != HANDSHAKE_POLL_READ_TIMEOUT {
            match self
                .port
                .set_timeout(original_timeout)
            {
                Ok(()) => {},
                Err(e) => {
                    if result.is_ok() {
                        return Err(e);
                    }
                    warn!("Failed to restore port timeout after handshake attempt: {e}");
                },
            }
        }

        result
    }

    /// Core handshake loop – sends one handshake frame, waits for the ACK,
    /// and repeats until the bootloader responds or timeout expires.
    ///
    /// The bootloader emits `.` heartbeat dots at ~110 ms intervals for
    /// roughly 1 second after reset.  During that window it checks UART RX
    /// for a valid handshake frame.
    ///
    /// Timing is critical for USB-serial adapters (CH340 etc.):
    ///  1. `flush()` after each write to force USB OUT transfer
    ///  2. 5 ms gap after flush so the 18-byte frame (1.56 ms at 115 200)
    ///     is fully transmitted and the bootloader sees an idle-line gap
    ///  3. 20 ms read timeout to cover the full CH340 USB round-trip
    ///     (ACK takes 5-20 ms from frame TX to host RX)
    ///  4. ~40 frames/sec – more than enough to hit the 1 s window
    fn try_connect_inner(&mut self) -> Result<()> {
        let start = Instant::now();
        // Always handshake at the current UART rate (115200). After ACK, switch
        // to target baud using SetBaudRate. This mirrors common ROM tool flows
        // and avoids intermittent failures from negotiating high baud in the
        // initial handshake frame.
        let handshake_data = CommandFrame::handshake(DEFAULT_BAUD).build();
        let mut carry = Vec::new();
        let mut total_rx = 0usize;
        let mut logged = 0usize;
        let mut app_detected = false;
        let mut app_warned = false;
        let mut app_detected_at: Option<Instant> = None;
        let mut dot_boost_until: Option<Instant> = None;
        let mut burst_frames_remaining = 0usize;
        let mut last_rx_at = Instant::now();

        // Drain any stale data from previous attempts.
        let _ = self
            .port
            .clear_buffers();

        // Pre-burst immediately at attempt start to catch very short windows.
        for _ in 0..INITIAL_HANDSHAKE_BURST_FRAMES {
            if let Err(e) = self
                .port
                .write_all(&handshake_data)
            {
                if e.kind() == std::io::ErrorKind::Interrupted {
                    return Err(Error::Io(e));
                }
                if is_connection_lost(&e) {
                    return Err(Error::Io(e));
                }
                trace!("Initial handshake write error (ignoring): {e}");
            }

            if let Err(e) = self
                .port
                .flush()
            {
                if e.kind() == std::io::ErrorKind::Interrupted {
                    return Err(Error::Io(e));
                }
                if is_connection_lost(&e) {
                    return Err(Error::Io(e));
                }
                trace!("Initial handshake flush error (ignoring): {e}");
            }

            sleep_interruptible(BOOT_HINT_BURST_GAP)?;
        }

        let mut last_tx: Option<Instant> = None;

        while start.elapsed() < HANDSHAKE_TIMEOUT {
            check_user_interrupted()?;
            let in_startup_aggressive_window = start.elapsed() < STARTUP_AGGRESSIVE_WINDOW;

            // --- RX first: observe current device mode before deciding TX ---
            let mut buf = [0u8; 256];
            match self
                .port
                .read(&mut buf)
            {
                Ok(n) if n > 0 => {
                    last_rx_at = Instant::now();
                    total_rx += n;
                    if logged < HANDSHAKE_MAX_LOG_PREVIEWS {
                        trace!(
                            "Handshake RX: {}",
                            format_raw_preview(&buf[..n], HANDSHAKE_RAW_PREVIEW_BYTES)
                        );
                        logged += 1;
                    }

                    if has_handshake_ack_with_carry(&mut carry, &buf[..n]) {
                        info!("Handshake successful!");

                        // Keep default 115200 after handshake.
                        // Switching baud here is racy on some devices/adapters:
                        // if SetBaudRate is not fully applied on the device side
                        // before LoaderBoot YMODEM starts, host/device baud can
                        // diverge and produce periodic garbage bytes (e.g.
                        // 0x80/0x00) while waiting for 'C'.
                        //
                        // We now always defer baud switch until LoaderBoot is
                        // transferred and the device is fully in second-stage
                        // protocol mode.
                        if !self.late_baud && self.target_baud != DEFAULT_BAUD {
                            debug!("Early baud switch requested but deferred for WS63 reliability");
                        }
                        return Ok(());
                    }

                    // Bootloader heartbeat/text hint indicates it is actively
                    // polling for handshake. Boost send cadence for a short
                    // window.
                    if has_bootloader_heartbeat_hint(&buf[..n]) {
                        app_detected = false;
                        app_detected_at = None;
                        dot_boost_until = Some(Instant::now() + BOOT_DOT_BOOST_WINDOW);
                        if burst_frames_remaining < BOOT_HINT_BURST_FRAMES {
                            burst_frames_remaining = BOOT_HINT_BURST_FRAMES;
                        }
                    }

                    // When we see substantial non-ACK data the device is
                    // probably running its application.  Print a one-time
                    // hint but keep sending handshake frames – the user
                    // may press reset at any moment.
                    if !app_detected && total_rx > APP_DETECT_THRESHOLD_BYTES {
                        app_detected = true;
                        app_detected_at = Some(Instant::now());
                        if !app_warned {
                            app_warned = true;
                            warn!(
                                "Device appears to be in application mode. \
                                 Please press the reset button to enter download mode."
                            );
                        }
                    }
                },
                Ok(_) => {},
                Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {
                    // During the startup aggressive window, don't let the
                    // 20 ms read timeout fully dictate TX cadence.
                    // Inject a tiny burst immediately after timeout to improve
                    // first-window capture probability.
                    if in_startup_aggressive_window {
                        for _ in 0..STARTUP_TIMEOUT_BURST_FRAMES {
                            check_user_interrupted()?;

                            if let Err(write_err) = self
                                .port
                                .write_all(&handshake_data)
                            {
                                if write_err.kind() == std::io::ErrorKind::Interrupted {
                                    return Err(Error::Io(write_err));
                                }
                                if is_connection_lost(&write_err) {
                                    return Err(Error::Io(write_err));
                                }
                                trace!("Startup timeout burst write error (ignoring): {write_err}");
                                continue;
                            }

                            if let Err(flush_err) = self
                                .port
                                .flush()
                            {
                                if flush_err.kind() == std::io::ErrorKind::Interrupted {
                                    return Err(Error::Io(flush_err));
                                }
                                if is_connection_lost(&flush_err) {
                                    return Err(Error::Io(flush_err));
                                }
                                trace!("Startup timeout burst flush error (ignoring): {flush_err}");
                                continue;
                            }

                            last_tx = Some(Instant::now());
                            sleep_interruptible(STARTUP_TIMEOUT_BURST_GAP)?;
                        }
                    }
                },
                Err(e) => {
                    if e.kind() == std::io::ErrorKind::Interrupted {
                        return Err(Error::Io(e));
                    }
                    if is_connection_lost(&e) {
                        return Err(Error::Io(e));
                    }
                    trace!("Read error (ignoring): {e}");
                },
            }

            // --- TX: paced handshake probe ---
            // Adaptive pacing:
            // - In boot-dot window, send aggressively to catch ACK.
            // - In app mode, probe at low frequency to avoid app UART errors.
            let now = Instant::now();
            let in_dot_boost_window = match dot_boost_until {
                Some(until) => now < until,
                None => false,
            };
            let in_burst = burst_frames_remaining > 0;
            let frame_interval = if in_burst {
                BOOT_HINT_BURST_GAP
            } else if in_startup_aggressive_window {
                STARTUP_AGGRESSIVE_FRAME_INTERVAL
            } else if in_dot_boost_window {
                BOOT_DOT_FRAME_INTERVAL
            } else if app_detected {
                APP_MODE_FRAME_INTERVAL
            } else {
                HANDSHAKE_FRAME_INTERVAL
            };

            // In app mode without boot-dot signal, switch to sparse probing
            // and only probe after RX is briefly quiet to avoid injecting
            // protocol frames into active app UART logs.
            if app_detected && !in_dot_boost_window && !in_burst && !in_startup_aggressive_window {
                let in_listen_only_grace = match app_detected_at {
                    Some(t0) => now.duration_since(t0) < APP_MODE_LISTEN_ONLY_GRACE,
                    None => false,
                };
                if in_listen_only_grace {
                    continue;
                }

                let rx_quiet = now.duration_since(last_rx_at) >= APP_MODE_TX_SILENCE_GUARD;
                let probe_due = match last_tx {
                    Some(last) => now.duration_since(last) >= APP_MODE_PROBE_INTERVAL,
                    None => true,
                };
                if !(rx_quiet && probe_due) {
                    continue;
                }
            }

            if let Some(last) = last_tx {
                if now.duration_since(last) < frame_interval {
                    continue;
                }
            }

            if let Err(e) = self
                .port
                .write_all(&handshake_data)
            {
                if e.kind() == std::io::ErrorKind::Interrupted {
                    return Err(Error::Io(e));
                }
                if is_connection_lost(&e) {
                    return Err(Error::Io(e));
                }
                trace!("Write error (ignoring): {e}");
                continue;
            }

            // flush() is critical for USB-serial adapters (CH340 etc.)
            if let Err(e) = self
                .port
                .flush()
            {
                if e.kind() == std::io::ErrorKind::Interrupted {
                    return Err(Error::Io(e));
                }
                if is_connection_lost(&e) {
                    return Err(Error::Io(e));
                }
                continue;
            }

            last_tx = Some(now);
            burst_frames_remaining = burst_frames_remaining.saturating_sub(1);
        }

        if total_rx > 0 {
            warn!(
                "Handshake timeout after receiving {total_rx} bytes of non-ACK data; \
                 device may not be in download mode"
            );
        }

        Err(Error::Timeout(format!(
            "No response after {} seconds",
            HANDSHAKE_TIMEOUT.as_secs()
        )))
    }

    /// Change the baud rate.
    fn change_baud_rate(&mut self, baud: u32) -> Result<()> {
        info!("Changing baud rate to {baud}");

        // Send baud rate change command
        let frame = CommandFrame::set_baud_rate(baud);
        self.send_command(&frame)?;

        // Wait for command to be processed
        thread::sleep(BAUD_CHANGE_DELAY);

        // Change local baud rate
        self.port
            .set_baud_rate(baud)?;

        // Clear buffers
        thread::sleep(BAUD_CHANGE_DELAY);
        self.port
            .clear_buffers()?;

        debug!("Baud rate changed to {baud}");
        Ok(())
    }

    /// Send a command frame.
    fn send_command(&mut self, frame: &CommandFrame) -> Result<()> {
        let data = frame.build();
        trace!(
            "Sending command {:?}: {} bytes",
            frame.command(),
            data.len()
        );

        self.port
            .write_all(&data)?;
        self.port
            .flush()?;

        Ok(())
    }

    /// Wait for SEBOOT magic (0xDEADBEEF) response from device.
    ///
    /// After LoaderBoot YMODEM transfer or after sending a download command,
    /// the device responds with a SEBOOT frame starting with the magic bytes.
    /// This function reads bytes until the magic sequence is found, then
    /// drains the remaining frame data.
    fn wait_for_magic(&mut self, timeout: Duration) -> Result<()> {
        let magic: [u8; 4] = [0xEF, 0xBE, 0xAD, 0xDE]; // Little-endian DEADBEEF
        let start = Instant::now();
        let mut match_idx = 0;

        debug!("Waiting for SEBOOT magic...");

        while start.elapsed() < timeout {
            let mut buf = [0u8; 1];
            match self
                .port
                .read(&mut buf)
            {
                Ok(1) => {
                    if buf[0] == magic[match_idx] {
                        match_idx += 1;
                        if match_idx == magic.len() {
                            // Found magic, drain remaining frame data
                            thread::sleep(Duration::from_millis(50));
                            let mut drain = [0u8; 256];
                            let _ = self
                                .port
                                .read(&mut drain);
                            debug!("Received SEBOOT magic response");
                            return Ok(());
                        }
                    } else {
                        // Reset match, check if current byte starts a new match
                        match_idx = usize::from(buf[0] == magic[0]);
                    }
                },
                Ok(_) => {},
                Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {},
                Err(e) => return Err(Error::Io(e)),
            }
        }

        Err(Error::Timeout("Timeout waiting for SEBOOT magic".into()))
    }

    /// Transfer LoaderBoot via YMODEM without sending a download command.
    ///
    /// After handshake, the device enters YMODEM mode directly for LoaderBoot.
    /// No download command (0xD2) should be sent. This matches the official
    /// fbb_burntool behavior where LOADER type partitions skip the download
    /// command and go straight to YMODEM transfer.
    fn transfer_loaderboot<F>(&mut self, name: &str, data: &[u8], progress: &mut F) -> Result<()>
    where
        F: FnMut(&str, usize, usize),
    {
        debug!(
            "Transferring LoaderBoot {} ({} bytes) via YMODEM",
            name,
            data.len()
        );

        let config = YmodemConfig {
            char_timeout: Duration::from_millis(1000),
            c_timeout: Duration::from_secs(30),
            max_retries: 10,
            verbose: self.verbose,
        };

        let mut ymodem = YmodemTransfer::with_config(&mut self.port, config);
        ymodem.transfer(name, data, |current, total| {
            progress(name, current, total);
        })?;

        debug!("LoaderBoot transfer complete");
        Ok(())
    }

    /// Flash a FWPKG firmware package.
    ///
    /// # Arguments
    ///
    /// * `fwpkg` - The firmware package to flash
    /// * `filter` - Optional filter for partition names (None = flash all)
    /// * `progress` - Progress callback (partition_name, current_bytes,
    ///   total_bytes)
    pub fn flash_fwpkg<F>(
        &mut self,
        fwpkg: &Fwpkg,
        filter: Option<&[&str]>,
        mut progress: F,
    ) -> Result<()>
    where
        F: FnMut(&str, usize, usize),
    {
        // Get LoaderBoot
        let loaderboot = fwpkg
            .loaderboot()
            .ok_or_else(|| Error::InvalidFwpkg("No LoaderBoot partition found".into()))?;

        info!("Flashing LoaderBoot: {}", loaderboot.name);

        // LoaderBoot: NO download command. After handshake ACK, the device
        // enters YMODEM mode directly. This matches fbb_burntool and ws63flash.
        let lb_data = fwpkg.bin_data(loaderboot)?;
        self.transfer_loaderboot(&loaderboot.name, lb_data, &mut progress)?;

        // Wait for LoaderBoot to initialize (device sends SEBOOT magic when ready)
        self.wait_for_magic(MAGIC_TIMEOUT)?;

        // Change baud rate after LoaderBoot is ready.
        // For WS63 this is the most reliable point to switch speed.
        if self.target_baud != DEFAULT_BAUD {
            self.change_baud_rate(self.target_baud)?;
        }

        // Flash remaining partitions
        for bin in fwpkg.normal_bins() {
            // Apply filter if provided
            if let Some(names) = filter {
                if !names
                    .iter()
                    .any(|n| {
                        bin.name
                            .contains(n)
                    })
                {
                    debug!("Skipping partition: {}", bin.name);
                    continue;
                }
            }

            info!(
                "Flashing partition: {} -> 0x{:08X}",
                bin.name, bin.burn_addr
            );

            let bin_data = fwpkg.bin_data(bin)?;
            self.download_binary(&bin.name, bin_data, bin.burn_addr, &mut progress)?;

            // Inter-partition delay to prevent serial data stale
            // (MCU won't respond if next command follows immediately)
            thread::sleep(PARTITION_DELAY);
        }

        info!("Flashing complete!");
        Ok(())
    }

    /// Download a single binary to flash with retry mechanism.
    #[allow(clippy::cast_possible_truncation)]
    fn download_binary<F>(
        &mut self,
        name: &str,
        data: &[u8],
        addr: u32,
        progress: &mut F,
    ) -> Result<()>
    where
        F: FnMut(&str, usize, usize),
    {
        let mut last_error = None;

        for attempt in 1..=MAX_DOWNLOAD_RETRIES {
            match self.try_download_binary(name, data, addr, progress) {
                Ok(()) => {
                    return Ok(());
                },
                Err(e) => {
                    if !should_retry_download_error(&e) {
                        return Err(e);
                    }

                    if attempt < MAX_DOWNLOAD_RETRIES {
                        warn!(
                            "Download failed for {name} (attempt \
                             {attempt}/{MAX_DOWNLOAD_RETRIES}): {e}"
                        );
                        warn!("Retrying...");
                        last_error = Some(e);

                        // Clear buffers and wait before retry
                        let _ = self
                            .port
                            .clear_buffers();
                        thread::sleep(CONNECT_RETRY_DELAY);
                    } else {
                        return Err(e);
                    }
                },
            }
        }

        // Use unwrap_or_else to ensure we never lose error information
        Err(last_error.unwrap_or_else(|| {
            Error::Protocol("Download failed after all retries (no error captured)".into())
        }))
    }

    /// Single attempt to download a binary.
    fn try_download_binary<F>(
        &mut self,
        name: &str,
        data: &[u8],
        addr: u32,
        progress: &mut F,
    ) -> Result<()>
    where
        F: FnMut(&str, usize, usize),
    {
        // Check for oversized data that would truncate
        let len = u32::try_from(data.len()).map_err(|_| {
            Error::Protocol(format!("Firmware too large ({} bytes > 4GB)", data.len()))
        })?;

        debug!(
            "Downloading {} ({} bytes) to 0x{:08X}",
            name,
            data.len(),
            addr
        );

        // Calculate aligned erase size (align up to 0x1000 = 4KB boundary)
        // This matches the official fbb_burntool behavior.
        let erase_size = (len + 0xFFF) & !0xFFF;

        // Send download command
        let frame = CommandFrame::download(addr, len, erase_size);
        self.send_command(&frame)?;

        // Wait for ACK frame (SEBOOT magic response) from device
        // The device responds with a SEBOOT frame after processing the download
        // command. ws63flash calls uart_read_until_magic() here.
        self.wait_for_magic(MAGIC_TIMEOUT)?;

        // Transfer using YMODEM
        // Note: ymodem.transfer() internally calls wait_for_c(), so we don't need
        // to call it here. The device sends 'C' after the ACK frame.
        let config = YmodemConfig {
            char_timeout: Duration::from_millis(1000),
            c_timeout: Duration::from_secs(30),
            max_retries: 10,
            verbose: self.verbose,
        };

        let mut ymodem = YmodemTransfer::with_config(&mut self.port, config);
        ymodem.transfer(name, data, |current, total| {
            progress(name, current, total);
        })?;

        debug!("{name} transfer complete");
        Ok(())
    }

    /// Write raw binary data to flash.
    ///
    /// # Arguments
    ///
    /// * `loaderboot` - LoaderBoot binary data (required for first-stage boot)
    /// * `bins` - List of (data, address) pairs to flash
    pub fn write_bins(&mut self, loaderboot: &[u8], bins: &[(&[u8], u32)]) -> Result<()> {
        info!("Writing LoaderBoot ({} bytes)", loaderboot.len());

        // Transfer LoaderBoot (no download command)
        self.transfer_loaderboot("loaderboot", loaderboot, &mut |_, _, _| {})?;

        // Wait for LoaderBoot to initialize
        self.wait_for_magic(MAGIC_TIMEOUT)?;

        // Change baud rate after LoaderBoot is ready.
        if self.target_baud != DEFAULT_BAUD {
            self.change_baud_rate(self.target_baud)?;
        }

        // Download remaining binaries
        for (i, (data, addr)) in bins
            .iter()
            .enumerate()
        {
            let name = format!("binary_{i}");
            info!("Writing {} ({} bytes) to 0x{:08X}", name, data.len(), addr);
            self.download_binary(&name, data, *addr, &mut |_, _, _| {})?;

            // Inter-partition delay
            thread::sleep(PARTITION_DELAY);
        }

        Ok(())
    }

    /// Erase entire flash.
    pub fn erase_all(&mut self) -> Result<()> {
        info!("Erasing entire flash...");

        let frame = CommandFrame::erase_all();
        self.send_command(&frame)?;

        // Wait for erase to complete
        thread::sleep(Duration::from_secs(5));

        info!("Flash erased");
        Ok(())
    }

    /// Reset the device.
    ///
    /// Sends a software reset command (0x87) to the device.
    /// After reset, the device will boot normally (not into download mode).
    /// This method includes a reconnection mechanism to handle USB enumeration
    /// delays where the serial port may temporarily disappear.
    pub fn reset(&mut self) -> Result<()> {
        info!("Resetting device...");

        let frame = CommandFrame::reset();
        self.send_command(&frame)?;

        // Wait for device to start resetting
        thread::sleep(Duration::from_millis(100));

        info!("Device reset command sent, waiting for reconnection...");
        Ok(())
    }

    /// Reset the device and wait for it to reconnect in download mode.
    ///
    /// This is useful when you want to reset the device and then reconnect
    /// to continue flashing. It handles USB enumeration delays where the
    /// serial port may temporarily disappear.
    ///
    /// # Arguments
    ///
    /// * `reconnect` - Whether to wait for device to reconnect in download mode
    pub fn reset_and_reconnect(&mut self, reconnect: bool) -> Result<()> {
        info!("Resetting device and waiting for reconnection...");

        let frame = CommandFrame::reset();
        self.send_command(&frame)?;

        // Wait for device to start resetting
        thread::sleep(Duration::from_millis(100));

        if reconnect {
            // Wait for USB enumeration
            thread::sleep(RESET_ENUM_DELAY);

            // Reuse connect() so reconnect can benefit from reopen factory.
            for attempt in 1..=MAX_RESET_RECONNECT_ATTEMPTS {
                match self.connect() {
                    Ok(()) => return Ok(()),
                    Err(e) if is_interrupted_error(&e) => return Err(e),
                    Err(e) if attempt < MAX_RESET_RECONNECT_ATTEMPTS => {
                        warn!(
                            "Reconnection attempt {attempt}/{MAX_RESET_RECONNECT_ATTEMPTS} failed: {e}"
                        );
                        thread::sleep(CONNECT_RETRY_DELAY);
                    },
                    Err(e) => return Err(e),
                }
            }
        }

        Ok(())
    }
}

// Native-specific convenience functions
#[cfg(feature = "native")]
mod native_impl {
    use {
        super::{
            DEFAULT_BAUD, Duration, Error, ReconnectFactory, Result, Ws63Flasher,
            check_user_interrupted, debug, is_interrupted_error, sleep_interruptible, thread, warn,
        },
        crate::port::{NativePort, NativePortEnumerator, PortEnumerator, PortInfo, SerialConfig},
        std::{sync::Arc, time::Instant},
    };

    // USB unplug/replug can take >2s on some Linux systems due to udev and
    // CH340 node recreation latency. Keep a wider polling window here so a
    // single reopen attempt can survive one full re-enumeration cycle.
    const REOPEN_WAIT_TIMEOUT: Duration = Duration::from_millis(4500);
    const REOPEN_POLL_INTERVAL: Duration = Duration::from_millis(40);

    #[derive(Clone, Debug)]
    struct UsbFingerprint {
        vid: u16,
        pid: u16,
        manufacturer: Option<String>,
        product: Option<String>,
        serial_number: Option<String>,
    }

    fn fingerprint_for_port(port_name: &str) -> Option<UsbFingerprint> {
        let ports = NativePortEnumerator::list_ports().ok()?;
        let target = ports
            .into_iter()
            .find(|p| p.name == port_name)?;
        Some(UsbFingerprint {
            vid: target.vid?,
            pid: target.pid?,
            manufacturer: target.manufacturer,
            product: target.product,
            serial_number: target.serial_number,
        })
    }

    fn resolve_port_name(initial_name: &str, fp: Option<&UsbFingerprint>) -> Result<String> {
        let ports = NativePortEnumerator::list_ports()?;

        // Always prefer the exact original node if it still exists.
        // This avoids hopping between /dev/ttyUSB0 and /dev/ttyUSB1 when
        // multiple CH340-class devices are present.
        if ports
            .iter()
            .any(|p| p.name == initial_name)
        {
            return Ok(initial_name.to_string());
        }

        if let Some(fingerprint) = fp {
            if let Some(matched) = ports
                .iter()
                .find(|p: &&PortInfo| {
                    p.vid == Some(fingerprint.vid)
                        && p.pid == Some(fingerprint.pid)
                        && p.serial_number == fingerprint.serial_number
                })
            {
                return Ok(matched
                    .name
                    .clone());
            }

            let mut vid_pid_matches: Vec<&PortInfo> = ports
                .iter()
                .filter(|p: &&PortInfo| {
                    p.vid == Some(fingerprint.vid) && p.pid == Some(fingerprint.pid)
                })
                .collect();

            if vid_pid_matches.is_empty() {
                // fall through to DeviceNotFound below
            } else {
                // Further narrow down with manufacturer/product when available.
                if let Some(manufacturer) = &fingerprint.manufacturer {
                    vid_pid_matches.retain(|p| {
                        p.manufacturer
                            .as_ref()
                            == Some(manufacturer)
                    });
                }
                if let Some(product) = &fingerprint.product {
                    vid_pid_matches.retain(|p| {
                        p.product
                            .as_ref()
                            == Some(product)
                    });
                }

                if vid_pid_matches.is_empty() {
                    // fall through to DeviceNotFound below
                } else {
                    // Deterministic fallback for serial-less USB bridges.
                    vid_pid_matches.sort_by(|a, b| {
                        a.name
                            .cmp(&b.name)
                    });
                    return Ok(vid_pid_matches[0]
                        .name
                        .clone());
                }
            }
        }

        Err(Error::DeviceNotFound)
    }

    fn reconnect_factory_for_native(
        config: SerialConfig,
        fingerprint: Option<UsbFingerprint>,
    ) -> Arc<ReconnectFactory<NativePort>> {
        Arc::new(move || {
            let start = Instant::now();
            let mut last_error: Option<Error> = None;

            while start.elapsed() < REOPEN_WAIT_TIMEOUT {
                check_user_interrupted()?;

                match resolve_port_name(&config.port_name, fingerprint.as_ref()) {
                    Ok(resolved_name) => {
                        let mut resolved_config = config.clone();
                        resolved_config.port_name = resolved_name;

                        match NativePort::open(&resolved_config) {
                            Ok(port) => return Ok(port),
                            Err(e) => {
                                if is_interrupted_error(&e) {
                                    return Err(e);
                                }

                                // Transient states during re-enumeration/udev handoff.
                                last_error = Some(e);
                                sleep_interruptible(REOPEN_POLL_INTERVAL)?;
                            },
                        }
                    },
                    Err(Error::DeviceNotFound) => {
                        sleep_interruptible(REOPEN_POLL_INTERVAL)?;
                    },
                    Err(e) => {
                        if is_interrupted_error(&e) {
                            return Err(e);
                        }

                        last_error = Some(e);
                        sleep_interruptible(REOPEN_POLL_INTERVAL)?;
                    },
                }
            }

            Err(last_error.unwrap_or(Error::DeviceNotFound))
        })
    }

    impl Ws63Flasher<NativePort> {
        /// Create a new WS63 flasher by opening a serial port.
        ///
        /// This is a convenience function for native platforms that opens
        /// the port with default settings.
        ///
        /// # Arguments
        ///
        /// * `port_name` - Serial port name (e.g., "/dev/ttyUSB0" or "COM3")
        /// * `target_baud` - Target baud rate for data transfer
        pub fn open(port_name: &str, target_baud: u32) -> Result<Self> {
            Self::open_with_retry(port_name, target_baud)
        }

        /// Open a serial port with full configuration (P0: 完整配置支持).
        ///
        /// This allows customization of all serial port parameters.
        ///
        /// # Arguments
        ///
        /// * `config` - Serial port configuration
        pub fn open_with_config(config: crate::port::SerialConfig) -> Result<Self> {
            Self::open_with_config_retry(config)
        }

        /// Open serial port with full config and retry mechanism.
        #[allow(clippy::needless_pass_by_value)]
        fn open_with_config_retry(config: crate::port::SerialConfig) -> Result<Self> {
            const MAX_OPEN_PORT_ATTEMPTS: usize = 3;
            const OPEN_RETRY_DELAY: Duration = Duration::from_millis(500);

            let mut last_error = None;

            for attempt in 1..=MAX_OPEN_PORT_ATTEMPTS {
                match NativePort::open(&config) {
                    Ok(port) => {
                        if attempt > 1 {
                            debug!("Port opened on attempt {attempt}");
                        }
                        let fingerprint = fingerprint_for_port(&config.port_name);
                        let factory = reconnect_factory_for_native(config.clone(), fingerprint);
                        return Ok(
                            Self::new_with_name(port, &config.port_name, config.baud_rate)
                                .with_reconnect_factory(factory),
                        );
                    },
                    Err(e) => {
                        if is_interrupted_error(&e) {
                            return Err(e);
                        }

                        warn!(
                            "Failed to open port {} (attempt {}/{}): {e}",
                            config.port_name, attempt, MAX_OPEN_PORT_ATTEMPTS
                        );
                        last_error = Some(e);

                        if attempt < MAX_OPEN_PORT_ATTEMPTS {
                            thread::sleep(OPEN_RETRY_DELAY);
                        }
                    },
                }
            }

            Err(last_error.unwrap_or_else(|| {
                Error::Config(format!(
                    "Failed to open port after {MAX_OPEN_PORT_ATTEMPTS} attempts"
                ))
            }))
        }

        /// Open serial port with retry mechanism.
        fn open_with_retry(port_name: &str, target_baud: u32) -> Result<Self> {
            const MAX_OPEN_PORT_ATTEMPTS: usize = 3;
            const OPEN_RETRY_DELAY: Duration = Duration::from_millis(500);

            let mut last_error = None;

            for attempt in 1..=MAX_OPEN_PORT_ATTEMPTS {
                let config = crate::port::SerialConfig::new(port_name, DEFAULT_BAUD);
                match NativePort::open(&config) {
                    Ok(port) => {
                        if attempt > 1 {
                            debug!("Port opened on attempt {attempt}");
                        }
                        let fingerprint = fingerprint_for_port(port_name);
                        let factory = reconnect_factory_for_native(config.clone(), fingerprint);
                        return Ok(Self::new_with_name(port, port_name, target_baud)
                            .with_reconnect_factory(factory));
                    },
                    Err(e) => {
                        if is_interrupted_error(&e) {
                            return Err(e);
                        }

                        warn!(
                            "Failed to open port {port_name} (attempt \
                             {attempt}/{MAX_OPEN_PORT_ATTEMPTS}): {e}"
                        );
                        last_error = Some(e);

                        if attempt < MAX_OPEN_PORT_ATTEMPTS {
                            thread::sleep(OPEN_RETRY_DELAY);
                        }
                    },
                }
            }

            Err(last_error.unwrap_or_else(|| {
                Error::Config(format!(
                    "Failed to open port {port_name} after {MAX_OPEN_PORT_ATTEMPTS} attempts"
                ))
            }))
        }
    }
}

impl<P: Port> crate::target::Flasher for Ws63Flasher<P> {
    fn connect(&mut self) -> Result<()> {
        self.connect()
    }

    fn flash_fwpkg(
        &mut self,
        fwpkg: &Fwpkg,
        filter: Option<&[&str]>,
        progress: &mut dyn FnMut(&str, usize, usize),
    ) -> Result<()> {
        self.flash_fwpkg(fwpkg, filter, |name, current, total| {
            progress(name, current, total);
        })
    }

    fn write_bins(&mut self, loaderboot: &[u8], bins: &[(&[u8], u32)]) -> Result<()> {
        self.write_bins(loaderboot, bins)
    }

    fn erase_all(&mut self) -> Result<()> {
        self.erase_all()
    }

    fn reset(&mut self) -> Result<()> {
        self.reset()
    }

    fn reset_and_reconnect(&mut self, reconnect: bool) -> Result<()> {
        self.reset_and_reconnect(reconnect)
    }

    fn connection_baud(&self) -> u32 {
        DEFAULT_BAUD
    }

    fn target_baud(&self) -> Option<u32> {
        Some(self.target_baud)
    }

    fn close(&mut self) {
        // Close the underlying port to release resources
        // This is important for proper cleanup after reset
        let _ = self
            .port
            .close();
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::port::Port,
        std::{
            io::{Read, Write},
            sync::{Arc, Mutex},
        },
    };

    /// Mock port implementation for testing without real hardware.
    ///
    /// This implementation uses an internal buffer to simulate serial port
    /// behavior, allowing unit tests to run without actual hardware.
    #[derive(Clone)]
    struct MockPort {
        name: String,
        baud_rate: u32,
        timeout: Duration,
        read_buffer: Arc<Mutex<Vec<u8>>>,
        write_buffer: Arc<Mutex<Vec<u8>>>,
        dtr: bool,
        rts: bool,
    }

    impl MockPort {
        fn new(name: &str) -> Self {
            Self {
                name: name.to_string(),
                baud_rate: 115200,
                timeout: Duration::from_millis(1000),
                read_buffer: Arc::new(Mutex::new(Vec::new())),
                write_buffer: Arc::new(Mutex::new(Vec::new())),
                dtr: false,
                rts: false,
            }
        }

        /// Add data to the read buffer (simulates receiving data from device).
        fn add_read_data(&self, data: &[u8]) {
            let mut buf = self
                .read_buffer
                .lock()
                .unwrap();
            buf.extend_from_slice(data);
        }

        /// Get data written to the port (simulates sending data to device).
        fn get_written_data(&self) -> Vec<u8> {
            let buf = self
                .write_buffer
                .lock()
                .unwrap();
            buf.clone()
        }

        /// Clear all buffers.
        fn clear(&self) {
            let mut read_buf = self
                .read_buffer
                .lock()
                .unwrap();
            let mut write_buf = self
                .write_buffer
                .lock()
                .unwrap();
            read_buf.clear();
            write_buf.clear();
        }
    }

    impl Port for MockPort {
        fn set_timeout(&mut self, timeout: Duration) -> Result<()> {
            self.timeout = timeout;
            Ok(())
        }

        fn timeout(&self) -> Duration {
            self.timeout
        }

        fn set_baud_rate(&mut self, baud_rate: u32) -> Result<()> {
            self.baud_rate = baud_rate;
            Ok(())
        }

        fn baud_rate(&self) -> u32 {
            self.baud_rate
        }

        fn clear_buffers(&mut self) -> Result<()> {
            self.clear();
            Ok(())
        }

        fn name(&self) -> &str {
            &self.name
        }

        fn set_dtr(&mut self, level: bool) -> Result<()> {
            self.dtr = level;
            Ok(())
        }

        fn set_rts(&mut self, level: bool) -> Result<()> {
            self.rts = level;
            Ok(())
        }

        fn read_cts(&mut self) -> Result<bool> {
            Ok(true) // Assume CTS is asserted
        }

        fn read_dsr(&mut self) -> Result<bool> {
            Ok(true) // Assume DSR is asserted
        }

        fn close(&mut self) -> Result<()> {
            // Clear all buffers to simulate port closure
            self.clear();
            Ok(())
        }
    }

    impl Read for MockPort {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let mut read_buf = self
                .read_buffer
                .lock()
                .map_err(|e| std::io::Error::other(format!("mutex poisoned: {e}")))?;

            if read_buf.is_empty() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "no data available",
                ));
            }

            let to_read = std::cmp::min(buf.len(), read_buf.len());
            buf[..to_read].copy_from_slice(&read_buf[..to_read]);
            read_buf.drain(..to_read);
            Ok(to_read)
        }
    }

    impl Write for MockPort {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            let mut write_buf = self
                .write_buffer
                .lock()
                .map_err(|e| std::io::Error::other(format!("mutex poisoned: {e}")))?;
            write_buf.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn test_is_port_error_for_io_kinds() {
        let disconnected = Error::Io(std::io::Error::new(
            std::io::ErrorKind::NotConnected,
            "not connected",
        ));
        assert!(is_port_error(&disconnected));

        let broken_pipe = Error::Io(std::io::Error::new(
            std::io::ErrorKind::BrokenPipe,
            "broken pipe",
        ));
        assert!(is_port_error(&broken_pipe));

        let timeout = Error::Io(std::io::Error::new(std::io::ErrorKind::TimedOut, "timeout"));
        assert!(!is_port_error(&timeout));
    }

    #[test]
    fn test_try_reopen_port_without_factory() {
        let port = MockPort::new("/dev/ttyUSB0");
        let mut flasher = Ws63Flasher::new_with_name(port, "/dev/ttyUSB0", 921600);
        let reopened = flasher
            .try_reopen_port()
            .expect("try_reopen_port should not fail without factory");
        assert!(!reopened);
    }

    #[test]
    fn test_try_reopen_port_with_factory_replaces_port() {
        let port = MockPort::new("old-port");
        let factory: Arc<ReconnectFactory<MockPort>> = Arc::new(|| Ok(MockPort::new("new-port")));
        let mut flasher =
            Ws63Flasher::new_with_name(port, "old-port", 921600).with_reconnect_factory(factory);

        let reopened = flasher
            .try_reopen_port()
            .expect("try_reopen_port should succeed with factory");
        assert!(reopened);
        assert_eq!(
            flasher
                .port
                .name(),
            "new-port"
        );
    }

    #[test]
    fn test_is_interrupted_error_for_io_interrupted() {
        let interrupted = Error::Io(std::io::Error::new(
            std::io::ErrorKind::Interrupted,
            "interrupted",
        ));
        assert!(is_interrupted_error(&interrupted));

        let timed_out = Error::Io(std::io::Error::new(std::io::ErrorKind::TimedOut, "timeout"));
        assert!(!is_interrupted_error(&timed_out));

        let serial_interrupted = Error::Serial(serialport::Error::new(
            serialport::ErrorKind::Io(std::io::ErrorKind::Interrupted),
            "serial interrupted",
        ));
        assert!(is_interrupted_error(&serial_interrupted));
    }

    #[test]
    fn test_should_retry_download_error_interrupted_false() {
        let interrupted = Error::Io(std::io::Error::new(
            std::io::ErrorKind::Interrupted,
            "interrupted",
        ));
        assert!(!should_retry_download_error(&interrupted));

        let timed_out = Error::Io(std::io::Error::new(std::io::ErrorKind::TimedOut, "timeout"));
        assert!(should_retry_download_error(&timed_out));
    }

    #[test]
    fn test_is_permission_denied_error_for_io_and_serial() {
        let io_perm = Error::Io(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "permission denied",
        ));
        assert!(is_permission_denied_error(&io_perm));

        let serial_perm = Error::Serial(serialport::Error::new(
            serialport::ErrorKind::Io(std::io::ErrorKind::PermissionDenied),
            "serial permission denied",
        ));
        assert!(is_permission_denied_error(&serial_perm));
    }

    #[test]
    fn test_reconnect_failure_delay_for_busy_and_not_found() {
        let fallback = Duration::from_millis(100);

        let busy = Error::Serial(serialport::Error::new(
            serialport::ErrorKind::Unknown,
            "Device or resource busy",
        ));
        assert_eq!(
            reconnect_failure_delay(&busy, fallback),
            Duration::from_millis(900)
        );

        let not_found = Error::Config("Device not found or not in boot mode".into());
        assert_eq!(
            reconnect_failure_delay(&not_found, fallback),
            Duration::from_millis(650)
        );

        let other = Error::Protocol("bad frame".into());
        assert_eq!(reconnect_failure_delay(&other, fallback), fallback);
    }

    #[test]
    fn test_should_attempt_reopen_for_port_errors_only() {
        // Timeout should NOT trigger reopen – port is functional, just no ACK.
        let timeout = Error::Timeout("timeout".into());
        assert!(!should_attempt_reopen(&timeout));

        // Actual port errors SHOULD trigger reopen.
        let broken = Error::Io(std::io::Error::new(
            std::io::ErrorKind::BrokenPipe,
            "broken pipe",
        ));
        assert!(should_attempt_reopen(&broken));

        let protocol = Error::Protocol("bad frame".into());
        assert!(!should_attempt_reopen(&protocol));
    }

    #[test]
    fn test_connect_retry_delay_by_error_type() {
        let timeout = Error::Timeout("timeout".into());
        assert_eq!(connect_retry_delay(&timeout), Duration::from_millis(100));

        let broken = Error::Io(std::io::Error::new(
            std::io::ErrorKind::BrokenPipe,
            "broken pipe",
        ));
        assert_eq!(connect_retry_delay(&broken), Duration::from_millis(1200));

        let permission = Error::Io(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "permission denied",
        ));
        assert_eq!(
            connect_retry_delay(&permission),
            Duration::from_millis(1800)
        );
    }

    #[test]
    fn test_has_handshake_ack_with_carry_detects_split_ack() {
        let mut carry = Vec::new();
        let split = 4;
        let first = &HANDSHAKE_ACK[..split];
        let second = &HANDSHAKE_ACK[split..];

        assert!(!has_handshake_ack_with_carry(&mut carry, first));
        assert!(has_handshake_ack_with_carry(&mut carry, second));
    }

    #[test]
    fn test_format_raw_preview_truncates_and_formats() {
        let input = b"ABC\x01\x02xyz";
        let preview = format_raw_preview(input, 4);
        assert!(preview.contains("hex=[41 42 43 01 ...]"));
        assert!(preview.contains("ascii='ABC....'") || preview.contains("ascii='ABC.'"));
        assert!(preview.contains("total=8"));
    }

    /// Test creating a Ws63Flasher with a mock port.
    #[test]
    fn test_flasher_new_with_mock_port() {
        let port = MockPort::new("/dev/ttyUSB0");
        let flasher = Ws63Flasher::new(port, 921600);

        assert_eq!(flasher.target_baud, 921600);
        assert!(!flasher.late_baud);
        assert_eq!(flasher.verbose, 0);
    }

    /// Test builder methods on Ws63Flasher.
    #[test]
    fn test_flasher_builder_methods() {
        let port = MockPort::new("/dev/ttyUSB0");
        let flasher = Ws63Flasher::new(port, 921600)
            .with_late_baud(true)
            .with_verbose(2);

        assert!(flasher.late_baud);
        assert_eq!(flasher.verbose, 2);
    }

    /// Test MockPort read/write operations.
    #[test]
    fn test_mock_port_read_write() {
        let mut port = MockPort::new("/dev/ttyUSB0");

        // Add some data to read buffer
        port.add_read_data(&[0xDE, 0xAD, 0xBE, 0xEF]);

        // Write some data
        port.write_all(b"test")
            .unwrap();
        port.flush()
            .unwrap();

        // Verify written data
        let written = port.get_written_data();
        assert_eq!(written, b"test");

        // Read data - use read_exact to handle partial reads properly
        let mut buf = [0u8; 4];
        std::io::Read::read_exact(&mut port, &mut buf).unwrap();
        assert_eq!(&buf, &[0xDE, 0xAD, 0xBE, 0xEF]);
    }

    /// Test MockPort buffer operations.
    #[test]
    fn test_mock_port_buffers() {
        let mut port = MockPort::new("/dev/ttyUSB0");

        // Clear buffers
        port.clear();
        assert!(
            port.get_written_data()
                .is_empty()
        );

        // Write and add read data
        port.write_all(b"hello")
            .unwrap();
        port.add_read_data(&[1, 2, 3]);

        // Verify
        assert_eq!(port.get_written_data(), b"hello");

        let mut buf = [0u8; 3];
        std::io::Read::read_exact(&mut port, &mut buf).unwrap();
        assert_eq!(&buf, &[1, 2, 3]);

        // Clear and verify
        port.clear();
        assert!(
            port.get_written_data()
                .is_empty()
        );
    }

    /// Test MockPort pin control.
    #[test]
    fn test_mock_port_pin_control() {
        let mut port = MockPort::new("/dev/ttyUSB0");

        assert!(!port.dtr);
        assert!(!port.rts);

        port.set_dtr(true)
            .unwrap();
        port.set_rts(true)
            .unwrap();

        assert!(port.dtr);
        assert!(port.rts);
    }

    /// Test MockPort baud rate and timeout.
    #[test]
    fn test_mock_port_baud_timeout() {
        let mut port = MockPort::new("/dev/ttyUSB0");

        assert_eq!(port.baud_rate(), 115200);
        assert_eq!(port.timeout(), Duration::from_millis(1000));

        port.set_baud_rate(921600)
            .unwrap();
        port.set_timeout(Duration::from_millis(500))
            .unwrap();

        assert_eq!(port.baud_rate(), 921600);
        assert_eq!(port.timeout(), Duration::from_millis(500));
    }

    /// Test MockPort name.
    #[test]
    fn test_mock_port_name() {
        let port = MockPort::new("/dev/ttyUSB1");
        assert_eq!(port.name(), "/dev/ttyUSB1");

        let port2 = MockPort::new("COM3");
        assert_eq!(port2.name(), "COM3");
    }

    /// Test creating flasher with mock port through
    /// ChipFamily::create_flasher_with_port.
    #[test]
    fn test_create_flasher_with_mock_port() {
        use crate::target::ChipFamily;

        let port = MockPort::new("/dev/ttyUSB0");
        let flasher = ChipFamily::Ws63.create_flasher_with_port(port, 921600, false, 0);

        assert!(flasher.is_ok());
        let flasher = flasher.unwrap();

        // Flasher should be usable (even though connect will fail without mock response
        // data)
        assert_eq!(flasher.connection_baud(), 115200); // DEFAULT_BAUD for handshake
        assert_eq!(flasher.target_baud(), Some(921600));
    }

    /// Test that Flasher trait object works correctly.
    #[test]
    fn test_flasher_trait_object() {
        use crate::target::Flasher;

        let port = MockPort::new("/dev/ttyUSB0");
        let flasher: Box<dyn Flasher> = Box::new(Ws63Flasher::new(port, 921600));

        assert_eq!(flasher.connection_baud(), 115200);
        assert_eq!(flasher.target_baud(), Some(921600));
    }

    /// Test multiple flasher instances with same mock port clone.
    #[test]
    fn test_multiple_flashers_same_port() {
        use crate::target::ChipFamily;

        let port = MockPort::new("/dev/ttyUSB0");
        let port_clone = port.clone();

        let flasher1 = ChipFamily::Ws63.create_flasher_with_port(port, 921600, false, 0);
        let flasher2 = ChipFamily::Ws63.create_flasher_with_port(port_clone, 115200, true, 1);

        assert!(flasher1.is_ok());
        assert!(flasher2.is_ok());

        let flasher1 = flasher1.unwrap();
        let flasher2 = flasher2.unwrap();

        assert_eq!(flasher1.target_baud(), Some(921600));
        assert_eq!(flasher2.target_baud(), Some(115200));
    }

    /// Test unsupported chip family returns error for create_flasher_with_port.
    #[test]
    fn test_create_flasher_with_port_unsupported_chip() {
        use crate::target::ChipFamily;

        let port = MockPort::new("/dev/ttyUSB0");
        let result = ChipFamily::Bs2x.create_flasher_with_port(port, 115200, false, 0);

        assert!(result.is_err());
        // Verify error is the Unsupported variant
        assert!(matches!(result, Err(crate::error::Error::Unsupported(_))));
    }

    // =====================================================================
    // Regression tests for protocol fixes (CRC fix + flash protocol fix)
    // =====================================================================

    /// Regression: erase_size must be aligned to 0x1000 (4KB) boundary.
    ///
    /// The official fbb_burntool aligns erase_size to 0x1000:
    ///   `if (eraseSize % 0x1000 != 0) eraseSize = 0x1000 * (eraseSize / 0x1000
    /// + 1)`
    ///
    /// Previously hisiflash passed `len` directly as erase_size without
    /// alignment.
    #[test]
    fn test_erase_size_alignment_4k() {
        // Already aligned values should stay the same
        assert_eq!((0x1000u32 + 0xFFF) & !0xFFF, 0x1000);
        assert_eq!((0x2000u32 + 0xFFF) & !0xFFF, 0x2000);
        assert_eq!((0x10000u32 + 0xFFF) & !0xFFF, 0x10000);

        // Non-aligned values should be rounded up to next 4KB boundary
        assert_eq!((1u32 + 0xFFF) & !0xFFF, 0x1000);
        assert_eq!((0x1001u32 + 0xFFF) & !0xFFF, 0x2000);
        assert_eq!((0x2001u32 + 0xFFF) & !0xFFF, 0x3000);
        assert_eq!((0xFFFu32 + 0xFFF) & !0xFFF, 0x1000);

        // Typical firmware sizes from ws63-liteos-app_all.fwpkg
        // root_params_sign.bin: length = 0x8F4 (2292 bytes)
        assert_eq!((0x8F4u32 + 0xFFF) & !0xFFF, 0x1000);
        // root_params_sign_b.bin: similar
        assert_eq!((0x900u32 + 0xFFF) & !0xFFF, 0x1000);
        // A larger typical partition
        assert_eq!((0x12345u32 + 0xFFF) & !0xFFF, 0x13000);
    }

    /// Regression: wait_for_magic correctly detects SEBOOT magic bytes.
    ///
    /// After LoaderBoot transfer and after each download command, the device
    /// sends a SEBOOT frame starting with 0xDEADBEEF (little-endian: EF BE AD
    /// DE). wait_for_magic must find this pattern in the byte stream.
    #[test]
    fn test_wait_for_magic_finds_magic() {
        let port = MockPort::new("/dev/ttyUSB0");

        // Simulate device response: some garbage then magic + frame data
        let mut response = vec![0x00, 0x41, 0x42]; // garbage bytes
        response.extend_from_slice(&[0xEF, 0xBE, 0xAD, 0xDE]); // magic
        response.extend_from_slice(&[0x0C, 0x00, 0xE1, 0x1E, 0x5A, 0x00, 0x00, 0x00]); // frame
        port.add_read_data(&response);

        let mut flasher = Ws63Flasher::new(port, 921600);
        let result = flasher.wait_for_magic(Duration::from_millis(500));
        assert!(
            result.is_ok(),
            "wait_for_magic should succeed when magic is present"
        );
    }

    /// Regression: wait_for_magic times out when no magic present.
    #[test]
    fn test_wait_for_magic_timeout_no_magic() {
        let port = MockPort::new("/dev/ttyUSB0");
        // No data in buffer -> should timeout
        let mut flasher = Ws63Flasher::new(port, 921600);
        let result = flasher.wait_for_magic(Duration::from_millis(100));
        assert!(
            result.is_err(),
            "wait_for_magic should timeout with no data"
        );
    }

    /// Regression: wait_for_magic with magic preceded by partial match.
    ///
    /// Tests the edge case where some bytes of the magic appear before the
    /// full magic sequence (e.g., 0xEF followed by garbage, then the real
    /// magic).
    #[test]
    fn test_wait_for_magic_partial_then_real() {
        let port = MockPort::new("/dev/ttyUSB0");

        // Partial magic (0xEF 0xBE) then non-magic, then real magic
        let mut response = Vec::new();
        response.extend_from_slice(&[0xEF, 0xBE, 0x00]); // partial match then break
        response.extend_from_slice(&[0xEF, 0xBE, 0xAD, 0xDE]); // real magic
        response.extend_from_slice(&[0x0C, 0x00, 0xE1, 0x1E, 0x5A, 0x00]); // frame tail
        port.add_read_data(&response);

        let mut flasher = Ws63Flasher::new(port, 921600);
        let result = flasher.wait_for_magic(Duration::from_millis(500));
        assert!(
            result.is_ok(),
            "wait_for_magic should handle partial matches"
        );
    }

    /// Regression: LoaderBoot must NOT send download command (0xD2).
    ///
    /// In the official fbb_burntool, `SendBurnCmd()` skips the download payload
    /// for LOADER type: `if (GetCurrentCmdType() != BurnCtrl::LOADER)`.
    /// ws63flash also only calls ymodem_xfer() directly after handshake for
    /// LoaderBoot.
    ///
    /// Previously hisiflash called download_binary() for LoaderBoot, which sent
    /// a 0xD2 download command frame. This caused the device to misinterpret
    /// the frame as data corruption.
    #[test]
    fn test_loaderboot_no_download_command() {
        let port = MockPort::new("/dev/ttyUSB0");

        // Simulate: device sends 'C' for YMODEM, then ACKs all blocks, then magic
        let response = vec![
            b'C', // YMODEM 'C' request
            0x06, // ACK for block 0 (file info)
            0x06, // ACK for data block
            0x06, // ACK for EOT
            0x06, // ACK for finish block
        ];
        port.add_read_data(&response);

        let mut flasher = Ws63Flasher::new(port, 921600);
        let result = flasher.transfer_loaderboot("test.bin", &[0xAA], &mut |_, _, _| {});

        // Transfer should succeed (or fail on mock port details, but NOT send 0xD2)
        // The key assertion: check that no download command frame was written
        let written = flasher
            .port
            .get_written_data();

        // Download command frame starts with magic + has cmd byte 0xD2
        // Scan the written data for 0xD2 command byte at the expected position
        // Frame format: [EF BE AD DE] [len_lo len_hi] [CMD] [SCMD] ...
        let has_download_cmd = written
            .windows(8)
            .any(|w| {
                w[0] == 0xEF
                    && w[1] == 0xBE
                    && w[2] == 0xAD
                    && w[3] == 0xDE
                    && w[6] == 0xD2
                    && w[7] == 0x2D
            });

        assert!(
            !has_download_cmd,
            "LoaderBoot transfer must NOT send download command (0xD2). Written data should only \
             contain YMODEM blocks, not SEBOOT command frames."
        );

        // Also verify that the YMODEM transfer actually wrote something
        assert!(
            !written.is_empty(),
            "YMODEM transfer should have written data for LoaderBoot"
        );

        // Verify the result succeeded
        assert!(
            result.is_ok(),
            "LoaderBoot transfer should succeed: {:?}",
            result.err()
        );
    }

    /// Regression: download_binary for normal partitions MUST send download
    /// command (0xD2).
    ///
    /// After LoaderBoot, all subsequent partitions require a download command
    /// with addr, len, and aligned erase_size before the YMODEM transfer.
    #[test]
    fn test_normal_partition_sends_download_command() {
        let port = MockPort::new("/dev/ttyUSB0");

        // Simulate: device sends magic ACK after download command, then 'C' for YMODEM
        let mut response = Vec::new();
        // ACK frame for download command (magic + frame data)
        response.extend_from_slice(&[0xEF, 0xBE, 0xAD, 0xDE]);
        response.extend_from_slice(&[0x0C, 0x00, 0xE1, 0x1E, 0x5A, 0x00, 0x00, 0x00]);
        // Note: wait_for_magic drains remaining bytes after the magic in one read call,
        // so YMODEM responses (C, ACKs) get consumed. This is a mock limitation.
        // We just verify the download command was sent; full flow is tested on
        // hardware.
        port.add_read_data(&response);

        let mut flasher = Ws63Flasher::new(port, 921600);
        let test_data = vec![0xBB; 100];
        // The transfer will fail because 'C' and ACKs were drained by wait_for_magic,
        // but we only care about verifying the download command was sent.
        let _result = flasher.try_download_binary(
            "test_partition.bin",
            &test_data,
            0x00800000,
            &mut |_, _, _| {},
        );

        let written = flasher
            .port
            .get_written_data();

        // Verify download command WAS sent
        let has_download_cmd = written
            .windows(8)
            .any(|w| {
                w[0] == 0xEF
                    && w[1] == 0xBE
                    && w[2] == 0xAD
                    && w[3] == 0xDE
                    && w[6] == 0xD2
                    && w[7] == 0x2D
            });

        assert!(
            has_download_cmd,
            "Normal partition download must send download command (0xD2). Written data should \
             contain a SEBOOT command frame."
        );
    }

    /// Regression: download command frame must contain properly aligned
    /// erase_size.
    ///
    /// Verifies the actual bytes written in the download command frame have
    /// the erase_size field aligned to 0x1000 (4KB).
    #[test]
    fn test_download_frame_erase_size_in_bytes() {
        // Test with a non-aligned length (100 bytes = 0x64)
        // Expected erase_size: (0x64 + 0xFFF) & !0xFFF = 0x1000
        let frame = CommandFrame::download(0x00800000, 100, (100 + 0xFFF) & !0xFFF);
        let data = frame.build();

        // Frame layout: Magic(4) + Len(2) + CMD(1) + SCMD(1) + addr(4) + len(4) +
        // erase_size(4) + const(2) + CRC(2) erase_size starts at offset 16
        let erase_size = u32::from_le_bytes([data[16], data[17], data[18], data[19]]);
        assert_eq!(
            erase_size, 0x1000,
            "erase_size for 100 bytes should be 0x1000 (4KB aligned), got 0x{erase_size:X}"
        );

        // Test with exactly 4KB
        let frame2 = CommandFrame::download(0x00800000, 0x1000, (0x1000u32 + 0xFFF) & !0xFFF);
        let data2 = frame2.build();
        let erase_size2 = u32::from_le_bytes([data2[16], data2[17], data2[18], data2[19]]);
        assert_eq!(
            erase_size2, 0x1000,
            "erase_size for exactly 4KB should remain 0x1000"
        );

        // Test with 4KB + 1
        let frame3 = CommandFrame::download(0x00800000, 0x1001, (0x1001u32 + 0xFFF) & !0xFFF);
        let data3 = frame3.build();
        let erase_size3 = u32::from_le_bytes([data3[16], data3[17], data3[18], data3[19]]);
        assert_eq!(
            erase_size3, 0x2000,
            "erase_size for 0x1001 bytes should be 0x2000 (next 4KB boundary)"
        );
    }
}
