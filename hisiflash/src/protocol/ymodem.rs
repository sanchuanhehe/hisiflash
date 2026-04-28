//! YMODEM-1K file transfer protocol implementation.
//!
//! This module implements the YMODEM-1K protocol for file transfer,
//! which is used by HiSilicon boot loaders for firmware transfer.
//!
//! ## Protocol Overview
//!
//! YMODEM-1K uses 1024-byte data blocks with CRC16 error detection:
//!
//! ```text
//! Block format:
//! +-----+-----+------+---------------+--------+
//! | STX | SEQ | ~SEQ |   DATA (1024) | CRC16  |
//! +-----+-----+------+---------------+--------+
//! | 1   | 1   | 1    |     1024      | 2      |
//! +-----+-----+------+---------------+--------+
//! ```

use {
    crate::{
        CancelContext,
        error::{Error, Result},
        protocol::crc::crc16_xmodem,
    },
    log::{debug, trace},
    std::{
        io::{Read, Write},
        time::{Duration, Instant},
    },
};

/// YMODEM control characters.
pub mod control {
    /// Start of Header (128-byte block).
    pub const SOH: u8 = 0x01;
    /// Start of Text (1024-byte block).
    pub const STX: u8 = 0x02;
    /// End of Transmission.
    pub const EOT: u8 = 0x04;
    /// Acknowledge.
    pub const ACK: u8 = 0x06;
    /// Not Acknowledge.
    pub const NAK: u8 = 0x15;
    /// Cancel.
    pub const CAN: u8 = 0x18;
    /// CRC mode request character.
    pub const C: u8 = b'C';
}

/// Block size for SOH packets.
pub const SOH_BLOCK_SIZE: usize = 128;

/// Block size for STX packets (YMODEM-1K).
pub const STX_BLOCK_SIZE: usize = 1024;

/// Grace period before treating a standalone 'C' as a retransmission request.
///
/// fbb_burntool does not immediately resend a data block when it sees a lone
/// 'C'. It keeps waiting briefly for an ACK first, which avoids spuriously
/// resending when the boot ROM emits repeated readiness characters while still
/// processing the current block.
const RETRY_REQUEST_GRACE: Duration = Duration::from_millis(300);
const SEBOOT_MAGIC: [u8; 4] = [0xEF, 0xBE, 0xAD, 0xDE];

/// BurnTool waits noticeably longer for the post-EOT 'C' before concluding the
/// session should end without a finish block.
const POST_EOT_C_TIMEOUT: Duration = Duration::from_millis(2500);

/// YMODEM configuration options.
#[derive(Debug, Clone)]
pub struct YmodemConfig {
    /// Timeout for waiting for a character.
    pub char_timeout: Duration,
    /// Timeout for waiting for 'C' character.
    pub c_timeout: Duration,
    /// Maximum retries for sending a block.
    pub max_retries: u32,
    /// Whether to send the finish block even if EOT is ACKed without a trailing
    /// 'C' request.
    pub finish_without_c: bool,
    /// Verbose output level.
    pub verbose: u8,
}

impl Default for YmodemConfig {
    fn default() -> Self {
        Self {
            char_timeout: Duration::from_secs(1),
            c_timeout: Duration::from_secs(60),
            max_retries: 10,
            finish_without_c: true,
            verbose: 0,
        }
    }
}

/// YMODEM transfer handler.
pub struct YmodemTransfer<'a, P: Read + Write> {
    port: &'a mut P,
    config: YmodemConfig,
    cancel: &'a CancelContext,
    prefetched_input: Vec<u8>,
    trailing_data: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ControlResponse {
    Ack,
    Nak,
    Cancel,
    RetryRequested,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EotOutcome {
    SendFinish,
    Complete,
}

impl<'a, P: Read + Write> YmodemTransfer<'a, P> {
    fn check_interrupted(&self) -> Result<()> {
        self.cancel
            .check()
    }

    /// Create a new YMODEM transfer handler.
    pub fn new(port: &'a mut P, cancel: &'a CancelContext) -> Self {
        Self {
            port,
            config: YmodemConfig::default(),
            cancel,
            prefetched_input: Vec::new(),
            trailing_data: Vec::new(),
        }
    }

    /// Create a new YMODEM transfer handler with custom configuration.
    pub fn with_config(port: &'a mut P, config: YmodemConfig, cancel: &'a CancelContext) -> Self {
        Self {
            port,
            config,
            cancel,
            prefetched_input: Vec::new(),
            trailing_data: Vec::new(),
        }
    }

    /// Seed the transfer with bytes that were already read by a previous stage.
    #[must_use]
    pub fn with_prefetched_input(mut self, prefetched_input: Vec<u8>) -> Self {
        self.prefetched_input = prefetched_input;
        self
    }

    /// Return bytes that were already received while the session was
    /// transitioning out of YMODEM.
    pub fn take_trailing_data(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.trailing_data)
    }

    fn extend_trailing_data(&mut self, data: &[u8], debug_message: &str) {
        if data.is_empty() {
            return;
        }

        let had_magic = self
            .trailing_data
            .windows(SEBOOT_MAGIC.len())
            .any(|window| window == SEBOOT_MAGIC);
        self.trailing_data
            .extend_from_slice(data);

        if !had_magic
            && self
                .trailing_data
                .windows(SEBOOT_MAGIC.len())
                .any(|window| window == SEBOOT_MAGIC)
        {
            debug!("{debug_message}");
        }
    }

    fn read_input(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self
            .prefetched_input
            .is_empty()
        {
            return self
                .port
                .read(buf);
        }

        let n = buf
            .len()
            .min(
                self.prefetched_input
                    .len(),
            );
        buf[..n].copy_from_slice(&self.prefetched_input[..n]);
        self.prefetched_input
            .drain(..n);
        Ok(n)
    }

    fn read_control_response(
        &mut self,
        timeout: Duration,
        retry_request_grace: Option<Duration>,
    ) -> Result<ControlResponse> {
        let start = Instant::now();
        let mut buf = [0u8; 64];
        let mut saw_retry_request = false;
        let mut retry_seen_at: Option<Instant> = None;

        while start.elapsed() < timeout {
            self.check_interrupted()?;

            if let (Some(grace), Some(first_seen_at)) = (retry_request_grace, retry_seen_at) {
                if first_seen_at.elapsed() >= grace {
                    return Ok(ControlResponse::RetryRequested);
                }
            }

            match self.read_input(&mut buf) {
                Ok(0) => {},
                Ok(n) => {
                    let chunk = &buf[..n];

                    if chunk.contains(&control::ACK) {
                        return Ok(ControlResponse::Ack);
                    }
                    if chunk.contains(&control::NAK) {
                        return Ok(ControlResponse::Nak);
                    }
                    if chunk.contains(&control::CAN) {
                        return Ok(ControlResponse::Cancel);
                    }
                    if let Some(grace) = retry_request_grace {
                        if chunk
                            .iter()
                            .all(|&byte| byte == control::C)
                        {
                            if grace.is_zero() {
                                return Ok(ControlResponse::RetryRequested);
                            }

                            saw_retry_request = true;
                            retry_seen_at.get_or_insert_with(Instant::now);
                            continue;
                        }
                    }

                    trace!("Ignoring non-control YMODEM response bytes: {chunk:02X?}");
                },
                Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {},
                Err(e) => return Err(Error::Io(e)),
            }
        }

        if retry_request_grace.is_some() && saw_retry_request {
            return Ok(ControlResponse::RetryRequested);
        }

        Err(Error::Timeout("Timeout waiting for YMODEM response".into()))
    }

    fn add_transfer_context(err: Error, context: impl Into<String>) -> Error {
        let context = context.into();
        match err {
            Error::Ymodem(message) => Error::Ymodem(format!("{context}: {message}")),
            Error::Timeout(message) => Error::Ymodem(format!("{context}: timeout: {message}")),
            other => other,
        }
    }

    /// Wait for the receiver to send 'C' (CRC mode request).
    pub fn wait_for_c(&mut self) -> Result<()> {
        debug!("Waiting for 'C' from receiver...");
        let start = Instant::now();

        let mut buf = [0u8; 64];

        while start.elapsed()
            < self
                .config
                .c_timeout
        {
            self.check_interrupted()?;

            match self.read_input(&mut buf) {
                Ok(0) => {},
                Ok(n) => {
                    let chunk = &buf[..n];
                    if chunk.contains(&control::C) {
                        debug!("Received 'C', starting transfer");
                        return Ok(());
                    }

                    trace!("Ignoring bytes while waiting for 'C': {chunk:02X?}");
                },
                Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {},
                Err(e) => return Err(Error::Io(e)),
            }
        }

        Err(Error::Timeout("Timeout waiting for 'C'".into()))
    }

    /// Build a YMODEM block.
    fn build_block(seq: u8, data: &[u8], use_stx: bool) -> Vec<u8> {
        let block_size = if use_stx {
            STX_BLOCK_SIZE
        } else {
            SOH_BLOCK_SIZE
        };
        let header = if use_stx { control::STX } else { control::SOH };

        let mut block = Vec::with_capacity(3 + block_size + 2);

        // Header
        block.push(header);
        block.push(seq);
        block.push(!seq);

        // Data (padded with 0x00 if necessary)
        if data.len() >= block_size {
            block.extend_from_slice(&data[..block_size]);
        } else {
            block.extend_from_slice(data);
            block.resize(3 + block_size, 0x00);
        }

        // CRC16
        let crc = crc16_xmodem(&block[3..3 + block_size]);
        block.push((crc >> 8) as u8);
        block.push((crc & 0xFF) as u8);

        block
    }

    /// Send a block and wait for ACK.
    fn send_block(&mut self, block: &[u8]) -> Result<()> {
        for retry in 0..self
            .config
            .max_retries
        {
            self.check_interrupted()?;
            trace!("Sending block (attempt {})", retry + 1);

            self.port
                .write_all(block)?;
            self.port
                .flush()?;

            match self.read_control_response(
                self.config
                    .char_timeout,
                Some(RETRY_REQUEST_GRACE),
            ) {
                Ok(ControlResponse::Ack) => {
                    trace!("Block ACKed");
                    return Ok(());
                },
                Ok(ControlResponse::Nak) => {
                    debug!("Block NAKed, retrying...");
                },
                Ok(ControlResponse::RetryRequested) => {
                    debug!("Receiver requested block retransmission with 'C'");
                },
                Ok(ControlResponse::Cancel) => {
                    return Err(Error::Ymodem("Transfer cancelled by receiver".into()));
                },
                Err(Error::Timeout(_)) => {
                    debug!("Timeout waiting for ACK, retrying...");
                },
                Err(e) => return Err(e),
            }
        }

        Err(Error::Ymodem(format!(
            "Block transfer failed after {} retries",
            self.config
                .max_retries
        )))
    }

    /// Send file information block (block 0).
    ///
    /// Format: `filename\0filesize\0`
    pub fn send_file_info(&mut self, filename: &str, filesize: usize) -> Result<()> {
        debug!("Sending file info: {filename} ({filesize} bytes)");

        // Build block 0 data
        let mut data = Vec::with_capacity(SOH_BLOCK_SIZE);
        data.extend_from_slice(filename.as_bytes());
        data.push(0x00);
        data.extend_from_slice(
            filesize
                .to_string()
                .as_bytes(),
        );
        data.push(0x00);

        let block = Self::build_block(0, &data, false);
        self.send_block(&block)
            .map_err(|err| {
                Self::add_transfer_context(
                    err,
                    format!("while sending YMODEM file info for {filename}"),
                )
            })
    }

    /// Send EOT (End of Transmission).
    fn send_eot(&mut self) -> Result<EotOutcome> {
        debug!("Sending EOT");

        for _retry in 0..self
            .config
            .max_retries
        {
            self.check_interrupted()?;

            self.port
                .write_all(&[control::EOT])?;
            self.port
                .flush()?;

            let response_deadline = Instant::now()
                + if self
                    .config
                    .finish_without_c
                {
                    self.config
                        .char_timeout
                } else {
                    self.config
                        .char_timeout
                        .max(POST_EOT_C_TIMEOUT)
                };
            let mut saw_ack = false;
            let mut ack_seen_at: Option<Instant> = None;
            let mut transition_data = Vec::new();
            let mut buf = [0u8; 64];

            loop {
                self.check_interrupted()?;

                if self
                    .config
                    .finish_without_c
                {
                    if let Some(ack_time) = ack_seen_at {
                        if ack_time.elapsed() >= Duration::from_millis(300) {
                            debug!("EOT ACKed without trailing 'C'");
                            return Ok(EotOutcome::Complete);
                        }
                    }
                }

                let now = Instant::now();
                if now >= response_deadline {
                    break;
                }

                match self.read_input(&mut buf) {
                    Ok(0) => {},
                    Ok(n) => {
                        let chunk = &buf[..n];
                        let saw_c = chunk.contains(&control::C);
                        let saw_ack_in_chunk = chunk.contains(&control::ACK);

                        if chunk.contains(&control::CAN) {
                            return Err(Error::Ymodem("Transfer cancelled by receiver".into()));
                        }
                        if chunk.contains(&control::NAK) {
                            break;
                        }
                        if saw_c && (saw_ack || saw_ack_in_chunk) {
                            return Ok(EotOutcome::SendFinish);
                        }
                        if saw_ack_in_chunk {
                            saw_ack = true;
                            ack_seen_at = Some(Instant::now());
                        }

                        if !chunk
                            .iter()
                            .all(|&byte| {
                                matches!(
                                    byte,
                                    control::ACK | control::NAK | control::CAN | control::C
                                )
                            })
                        {
                            transition_data.extend_from_slice(chunk);
                            if transition_data
                                .windows(SEBOOT_MAGIC.len())
                                .any(|window| window == SEBOOT_MAGIC)
                            {
                                self.extend_trailing_data(
                                    &transition_data,
                                    "EOT followed by SEBOOT response; handing trailing bytes to caller",
                                );
                                return Ok(EotOutcome::Complete);
                            }

                            trace!("Ignoring non-control YMODEM response bytes: {chunk:02X?}");
                        }
                    },
                    Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {
                        if saw_ack
                            && self
                                .config
                                .finish_without_c
                        {
                            debug!("EOT ACKed without trailing 'C'");
                            return Ok(EotOutcome::Complete);
                        }
                    },
                    Err(e) => return Err(Error::Io(e)),
                }
            }

            if saw_ack {
                self.extend_trailing_data(&transition_data, "EOT buffered trailing response bytes");
                debug!("EOT ACKed without trailing 'C'");
                return Ok(EotOutcome::Complete);
            }
        }

        // Consider EOT sent even without ACK
        Ok(EotOutcome::Complete)
    }

    /// Send finish block (empty block 0 to end session).
    pub fn send_finish(&mut self) -> Result<()> {
        debug!("Sending finish block");
        let block = Self::build_block(0, &[], false);

        for retry in 0..self
            .config
            .max_retries
        {
            self.check_interrupted()?;
            trace!("Sending finish block (attempt {})", retry + 1);

            self.port
                .write_all(&block)?;
            self.port
                .flush()?;

            let start = Instant::now();
            let mut buf = [0u8; 64];
            let mut transition_data = Vec::new();

            while start.elapsed()
                < self
                    .config
                    .char_timeout
            {
                self.check_interrupted()?;

                match self.read_input(&mut buf) {
                    Ok(0) => {},
                    Ok(n) => {
                        let chunk = &buf[..n];

                        if chunk.contains(&control::CAN) {
                            return Err(Error::Ymodem("Transfer cancelled by receiver".into()));
                        }
                        if chunk.contains(&control::NAK) {
                            debug!("Finish block NAKed, retrying...");
                            break;
                        }
                        if let Some(ack_index) = chunk
                            .iter()
                            .position(|&byte| byte == control::ACK)
                        {
                            transition_data.extend_from_slice(&chunk[..ack_index]);
                            transition_data.extend_from_slice(&chunk[ack_index + 1..]);
                            self.extend_trailing_data(
                                &transition_data,
                                "Finish block ACK followed by SEBOOT response; handing trailing bytes to caller",
                            );
                            trace!("Finish block ACKed");
                            return Ok(());
                        }

                        transition_data.extend_from_slice(chunk);
                        trace!(
                            "Ignoring non-control YMODEM response bytes after finish block: {chunk:02X?}"
                        );
                    },
                    Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {},
                    Err(e) => return Err(Error::Io(e)),
                }
            }

            debug!("Timeout waiting for finish block ACK, retrying...");
        }

        Err(Error::Ymodem(format!(
            "Block transfer failed after {} retries",
            self.config
                .max_retries
        )))
    }

    /// Transfer file data.
    ///
    /// # Arguments
    ///
    /// * `filename` - Name of the file being transferred
    /// * `data` - File data to transfer
    /// * `progress` - Optional progress callback (current, total)
    pub fn transfer<F>(&mut self, filename: &str, data: &[u8], mut progress: F) -> Result<()>
    where
        F: FnMut(usize, usize),
    {
        self.check_interrupted()?;

        debug!(
            "Starting YMODEM transfer: {} ({} bytes)",
            filename,
            data.len()
        );

        // Wait for receiver to request transfer
        self.wait_for_c()
            .map_err(|err| {
                Self::add_transfer_context(
                    err,
                    format!("while waiting for receiver request for {filename}"),
                )
            })?;

        // Send file info (block 0)
        self.send_file_info(filename, data.len())?;

        // Note: WS63 device does NOT send a second 'C' after block 0 ACK.
        // Proceed directly to data blocks (confirmed by fbb_burntool and ws63flash).

        // Send data blocks
        let mut seq: u8 = 1;
        let mut offset = 0;
        let total = data.len();

        while offset < total {
            self.check_interrupted()?;

            let chunk_end = (offset + STX_BLOCK_SIZE).min(total);
            let chunk = &data[offset..chunk_end];

            let block = Self::build_block(seq, chunk, true);
            self.send_block(&block)
                .map_err(|err| {
                    Self::add_transfer_context(
                        err,
                        format!(
                            "while sending YMODEM data block {seq} for {filename} at offset 0x{offset:08X}"
                        ),
                    )
                })?;

            offset = chunk_end;
            seq = seq.wrapping_add(1);

            progress(offset, total);
        }

        // Send EOT
        let eot_outcome = self
            .send_eot()
            .map_err(|err| {
                Self::add_transfer_context(
                    err,
                    format!("while finishing YMODEM payload for {filename}"),
                )
            })?;

        if matches!(eot_outcome, EotOutcome::SendFinish)
            || self
                .config
                .finish_without_c
        {
            let _ = self
                .send_finish()
                .map_err(|err| {
                    Self::add_transfer_context(
                        err,
                        format!("while closing YMODEM session for {filename}"),
                    )
                });
        }

        debug!("YMODEM transfer complete");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_block_soh() {
        let data = [0x01, 0x02, 0x03];

        let block = YmodemTransfer::<std::io::Cursor<Vec<u8>>>::build_block(1, &data, false);

        assert_eq!(block[0], control::SOH);
        assert_eq!(block[1], 1);
        assert_eq!(block[2], 0xFE);
        assert_eq!(block.len(), 3 + SOH_BLOCK_SIZE + 2);
    }

    #[test]
    fn test_build_block_stx() {
        let data = vec![0xAA; STX_BLOCK_SIZE];

        let block = YmodemTransfer::<std::io::Cursor<Vec<u8>>>::build_block(5, &data, true);

        assert_eq!(block[0], control::STX);
        assert_eq!(block[1], 5);
        assert_eq!(block[2], 0xFA);
        assert_eq!(block.len(), 3 + STX_BLOCK_SIZE + 2);
    }

    // =====================================================================
    // Regression tests for YMODEM protocol fixes
    // =====================================================================

    /// Mock serial port with separate read/write buffers for YMODEM testing.
    ///
    /// Unlike `Cursor<Vec<u8>>`, this keeps reads and writes independent.
    struct MockSerial {
        read_chunks: std::collections::VecDeque<Vec<u8>>,
        write_buf: Vec<u8>,
    }

    impl MockSerial {
        fn new(response: &[u8]) -> Self {
            let chunks = response
                .iter()
                .copied()
                .map(|byte| vec![byte])
                .collect::<Vec<_>>();
            Self::with_chunks(chunks)
        }

        fn with_chunks<I>(chunks: I) -> Self
        where
            I: IntoIterator<Item = Vec<u8>>,
        {
            Self {
                read_chunks: chunks
                    .into_iter()
                    .collect(),
                write_buf: Vec::new(),
            }
        }
    }

    impl std::io::Read for MockSerial {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self
                .read_chunks
                .is_empty()
            {
                return Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "no data"));
            }

            let chunk = self
                .read_chunks
                .pop_front()
                .unwrap();
            let n = buf
                .len()
                .min(chunk.len());
            buf[..n].copy_from_slice(&chunk[..n]);

            if n < chunk.len() {
                self.read_chunks
                    .push_front(chunk[n..].to_vec());
            }

            Ok(n)
        }
    }

    impl std::io::Write for MockSerial {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.write_buf
                .extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// Regression: YMODEM transfer must only call wait_for_c ONCE at the start.
    ///
    /// WS63 device sends a single 'C' after acknowledging the download command
    /// (or after handshake for LoaderBoot). It does NOT send a second 'C' after
    /// block 0 (file info) ACK. Previously hisiflash called wait_for_c twice
    /// (once in transfer(), once before data blocks), causing a timeout on the
    /// second wait.
    ///
    /// Reference: ws63flash ymodem_xfer() only waits for 'C' once at the start,
    /// then sends block 0, then immediately proceeds to data blocks.
    /// fbb_burntool HandleWaitStartC also transitions to data sending after
    /// one 'C' received.
    #[test]
    fn test_ymodem_transfer_single_c_wait() {
        // Simulate device that sends: C, ACK(block0), ACK(data), ACK(EOT), ACK(finish)
        // NO second 'C' between block 0 ACK and data blocks.
        let response = vec![
            control::C,   // Initial 'C' for YMODEM start
            control::ACK, // ACK for block 0 (file info)
            control::ACK, // ACK for data block 1
            control::ACK, // ACK for EOT
            control::ACK, // ACK for finish block
        ];

        let mut port = MockSerial::new(&response);
        let config = YmodemConfig {
            char_timeout: Duration::from_millis(100),
            c_timeout: Duration::from_millis(200),
            max_retries: 1,
            finish_without_c: true,
            verbose: 0,
        };

        let cancel = crate::CancelContext::none();
        let mut ymodem = YmodemTransfer::with_config(&mut port, config, &cancel);
        let test_data = vec![0x42; 100]; // Small test payload
        let result = ymodem.transfer("test.bin", &test_data, |_, _| {});

        assert!(
            result.is_ok(),
            "YMODEM transfer should succeed with single 'C' (no second 'C' after block 0). Error: \
             {:?}",
            result.err()
        );
    }

    /// Regression: YMODEM transfer must NOT wait for 'C' before finish block.
    ///
    /// After EOT is ACKed, ws63flash sends the finish block immediately.
    /// Previously hisiflash waited for 'C' before send_finish(), which could
    /// timeout if the device doesn't send 'C' at that point.
    #[test]
    fn test_ymodem_no_c_before_finish() {
        // Response sequence without any extra 'C' between EOT ACK and finish
        let response = vec![
            control::C,   // Initial 'C'
            control::ACK, // ACK for block 0
            control::ACK, // ACK for data block 1
            control::ACK, // ACK for EOT
            // Note: NO 'C' here before finish block
            control::ACK, // ACK for finish block
        ];

        let mut port = MockSerial::new(&response);
        let config = YmodemConfig {
            char_timeout: Duration::from_millis(100),
            c_timeout: Duration::from_millis(200),
            max_retries: 1,
            finish_without_c: true,
            verbose: 0,
        };

        let cancel = crate::CancelContext::none();
        let mut ymodem = YmodemTransfer::with_config(&mut port, config, &cancel);
        let test_data = vec![0x55; 50];
        let result = ymodem.transfer("test.bin", &test_data, |_, _| {});

        assert!(
            result.is_ok(),
            "YMODEM should complete without waiting for 'C' before finish block. Error: {:?}",
            result.err()
        );
    }

    /// Regression: YMODEM transfer with exactly 1024 bytes (one full STX
    /// block).
    #[test]
    fn test_ymodem_transfer_exact_block_size() {
        let response = vec![
            control::C,   // Initial 'C'
            control::ACK, // ACK for block 0
            control::ACK, // ACK for data block 1
            control::ACK, // ACK for EOT
            control::ACK, // ACK for finish block
        ];

        let mut port = MockSerial::new(&response);
        let config = YmodemConfig {
            char_timeout: Duration::from_millis(100),
            c_timeout: Duration::from_millis(200),
            max_retries: 1,
            finish_without_c: true,
            verbose: 0,
        };

        let cancel = crate::CancelContext::none();
        let mut ymodem = YmodemTransfer::with_config(&mut port, config, &cancel);
        let test_data = vec![0xCC; STX_BLOCK_SIZE]; // Exactly 1024 bytes
        let result = ymodem.transfer("exact_block.bin", &test_data, |_, _| {});

        assert!(
            result.is_ok(),
            "YMODEM should handle exactly 1024-byte payload. Error: {:?}",
            result.err()
        );
    }

    /// Regression: YMODEM transfer with multi-block data.
    #[test]
    fn test_ymodem_transfer_multi_block() {
        let num_blocks = 3;
        let mut response = vec![
            control::C,   // Initial 'C'
            control::ACK, // ACK for block 0
        ];
        response.extend(std::iter::repeat_n(control::ACK, num_blocks)); // ACK for each data block
        response.push(control::ACK); // ACK for EOT
        response.push(control::ACK); // ACK for finish block

        let mut port = MockSerial::new(&response);
        let config = YmodemConfig {
            char_timeout: Duration::from_millis(100),
            c_timeout: Duration::from_millis(200),
            max_retries: 1,
            finish_without_c: true,
            verbose: 0,
        };

        let cancel = crate::CancelContext::none();
        let mut ymodem = YmodemTransfer::with_config(&mut port, config, &cancel);
        // 3 full blocks = 3072 bytes
        let test_data = vec![0xDD; STX_BLOCK_SIZE * num_blocks];
        let mut progress_calls = 0;
        let result = ymodem.transfer("multi_block.bin", &test_data, |current, total| {
            assert_eq!(total, STX_BLOCK_SIZE * num_blocks);
            assert!(current <= total);
            progress_calls += 1;
        });

        assert!(
            result.is_ok(),
            "YMODEM should handle multi-block transfer. Error: {:?}",
            result.err()
        );
        assert_eq!(
            progress_calls, num_blocks,
            "Progress should be called once per block"
        );
    }

    #[test]
    fn test_ymodem_transfer_accepts_ack_amid_noise() {
        let mut port = MockSerial::with_chunks([
            vec![control::C],
            b"ready for download\r\n\x06".to_vec(),
            b"log noise\x06".to_vec(),
            vec![control::C],
            vec![control::ACK],
        ]);
        let config = YmodemConfig {
            char_timeout: Duration::from_millis(100),
            c_timeout: Duration::from_millis(200),
            max_retries: 2,
            finish_without_c: true,
            verbose: 0,
        };

        let cancel = crate::CancelContext::none();
        let mut ymodem = YmodemTransfer::with_config(&mut port, config, &cancel);
        let test_data = vec![0xA5; 32];
        let result = ymodem.transfer("noisy.bin", &test_data, |_, _| {});

        assert!(
            result.is_ok(),
            "YMODEM should accept ACK embedded in serial log noise. Error: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_ymodem_skips_finish_without_c_when_configured() {
        let mut port = MockSerial::with_chunks([
            vec![control::C],
            vec![control::ACK],
            vec![control::ACK],
            vec![control::ACK],
        ]);
        let config = YmodemConfig {
            char_timeout: Duration::from_millis(100),
            c_timeout: Duration::from_millis(200),
            max_retries: 1,
            finish_without_c: false,
            verbose: 0,
        };

        let cancel = crate::CancelContext::none();
        let mut ymodem = YmodemTransfer::with_config(&mut port, config, &cancel);
        let test_data = vec![0x5A; 32];
        let result = ymodem.transfer("bs21e.bin", &test_data, |_, _| {});

        assert!(
            result.is_ok(),
            "YMODEM should complete when EOT is ACKed without trailing 'C' and finish block is disabled. Error: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_ymodem_transfer_retries_block_on_c_request() {
        let mut port = MockSerial::with_chunks([
            vec![control::C],
            vec![control::ACK],
            vec![control::C],
            vec![control::ACK],
            vec![control::ACK],
            vec![control::ACK],
        ]);
        let config = YmodemConfig {
            char_timeout: Duration::from_millis(100),
            c_timeout: Duration::from_millis(200),
            max_retries: 2,
            finish_without_c: true,
            verbose: 0,
        };

        let cancel = crate::CancelContext::none();
        let mut ymodem = YmodemTransfer::with_config(&mut port, config, &cancel);
        let test_data = vec![0x5A; 16];
        let result = ymodem.transfer("retry.bin", &test_data, |_, _| {});

        assert!(
            result.is_ok(),
            "YMODEM should retry the current block when receiver sends 'C'. Error: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_ymodem_handles_ack_and_c_in_same_eot_chunk() {
        let mut port = MockSerial::with_chunks([
            vec![control::C],
            vec![control::ACK],
            vec![control::ACK],
            vec![control::ACK, control::C],
            vec![control::ACK, 0xEF, 0xBE, 0xAD, 0xDE, 0x01],
        ]);
        let config = YmodemConfig {
            char_timeout: Duration::from_millis(100),
            c_timeout: Duration::from_millis(200),
            max_retries: 1,
            finish_without_c: false,
            verbose: 0,
        };

        let cancel = crate::CancelContext::none();
        let mut ymodem = YmodemTransfer::with_config(&mut port, config, &cancel);
        let test_data = vec![0x5A; 32];
        let result = ymodem.transfer("bs21e.bin", &test_data, |_, _| {});
        let trailing_data = ymodem.take_trailing_data();
        drop(ymodem);

        assert!(
            result.is_ok(),
            "YMODEM should send finish block when ACK and 'C' arrive in the same EOT read. Error: {:?}",
            result.err()
        );

        let finish_block = YmodemTransfer::<std::io::Cursor<Vec<u8>>>::build_block(0, &[], false);
        assert!(
            port.write_buf
                .ends_with(&finish_block),
            "finish block should be written after mixed ACK+'C' EOT response"
        );
        assert_eq!(
            trailing_data,
            vec![0xEF, 0xBE, 0xAD, 0xDE, 0x01],
            "trailing SEBOOT bytes after finish ACK should be preserved for the caller"
        );
    }

    #[test]
    fn test_wait_for_c_interrupted_immediate() {
        let mut port = MockSerial::new(&[]);
        let config = YmodemConfig {
            char_timeout: Duration::from_millis(50),
            c_timeout: Duration::from_millis(100),
            max_retries: 1,
            finish_without_c: true,
            verbose: 0,
        };

        let cancel = crate::CancelContext::new(|| true);
        let mut ymodem = YmodemTransfer::with_config(&mut port, config, &cancel);
        let result = ymodem.wait_for_c();

        assert!(matches!(
            result,
            Err(Error::Io(ref io)) if io.kind() == std::io::ErrorKind::Interrupted
        ));
    }

    #[test]
    fn test_transfer_interrupted_before_start() {
        let mut port = MockSerial::new(&[]);
        let config = YmodemConfig {
            char_timeout: Duration::from_millis(50),
            c_timeout: Duration::from_millis(100),
            max_retries: 1,
            finish_without_c: true,
            verbose: 0,
        };

        let cancel = crate::CancelContext::new(|| true);
        let mut ymodem = YmodemTransfer::with_config(&mut port, config, &cancel);
        let result = ymodem.transfer("app.bin", &[0x11, 0x22], |_, _| {});

        assert!(matches!(
            result,
            Err(Error::Io(ref io)) if io.kind() == std::io::ErrorKind::Interrupted
        ));
        assert!(
            port.write_buf
                .is_empty(),
            "Interrupted transfer should not write any YMODEM data"
        );
    }
}
