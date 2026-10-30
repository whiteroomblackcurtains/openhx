use super::{
    parser::{parse_msgpack_stream, parse_preset_document, parse_setlists, response_payload},
    protocol::*,
};
use crate::{
    client::DeviceClient,
    device::{KnownDevice, catalog::DEVICE_CATALOG, profile::DeviceProfile},
    error::HxError,
    models::{Preset, Setlist},
};
use openhx_i18n::fl;
use rusb::{Context, DeviceHandle, UsbContext};
use std::time::Duration;

/// Maximum number of attempts to initialize the USB session before giving up.
const MAX_INIT_RETRIES: u32 = 5;

/// Base back-off duration in milliseconds.
///
/// The actual wait on attempt `n` (1-indexed) is `BASE_BACKOFF_MS × 2ⁿ`,
/// yielding 600 ms, 1 200 ms, 2 400 ms, 4 800 ms for attempts 1–4.
const BASE_BACKOFF_MS: u64 = 300;

/// Offset of the per-packet sequence byte within both request and response
/// headers.
///
/// The wire field is a big-endian `u16` at bytes 8–9; in observed traffic
/// the high byte is always zero, so byte 9 is the effective `u8` seq. See
/// the header layout in `protocol.rs`.
const SEQ_BYTE_OFFSET: usize = 9;

/// Offset of the effective per-packet command byte within both request and
/// response headers.
///
/// The wire field is a big-endian `u16` at bytes 10–11; in observed traffic
/// the high byte is always zero, so byte 11 carries the effective cmd.
const CMD_BYTE_OFFSET: usize = 11;

/// Maximum number of stale packets discarded while waiting for a matching
/// sequence byte before declaring the session wedged.
///
/// Sized purely as a runaway guard. A healthy device emits at most a few
/// out-of-band packets between transactions; the per-discard `tracing`
/// log surfaces excessive drops long before this ceiling.
const MAX_STALE_DROPS: usize = 64;

/// Per-attempt timeout for the channel-handshake read.
///
/// A successful handshake responds in tens of milliseconds; anything
/// longer means the device dropped the write and a resend is cheaper
/// than waiting out the full session timeout.
const HANDSHAKE_RETRY_TIMEOUT_MS: u64 = 400;

/// Maximum number of handshake resends before surfacing a timeout to the
/// outer retry loop.
const HANDSHAKE_INNER_RETRIES: u32 = 4;

/// Renders the byte at `offset` as `0xNN`, or `<short>` if the read was
/// truncated below that offset. Used by the diagnostic logging paths.
fn fmt_byte_at(buf: &[u8], n: usize, offset: usize) -> String {
    if n > offset {
        format!("0x{:02X}", buf[offset])
    } else {
        "<short>".to_string()
    }
}

/// A USB client that communicates with any supported device
/// over bulk endpoints using the device's proprietary
/// MessagePack-over-USB protocol.
pub struct Client {
    handle: DeviceHandle<Context>,
    profile: &'static DeviceProfile,
}

impl Client {
    /// Attempts to establish a USB connection with a specific device model.
    pub fn connect(device: KnownDevice) -> Result<Self, HxError> {
        Self::connect_profile(device.profile())
    }

    /// Scans the USB bus to auto-detect and connect to the first supported device.
    ///
    /// This method iterates through the internal [`DEVICE_CATALOG`]. It is ideal for
    /// generic initialization where the exact hardware model isn't known ahead of time.
    pub fn detect() -> Result<Self, HxError> {
        for profile in DEVICE_CATALOG {
            match Self::connect_profile(profile) {
                Ok(client) => {
                    eprintln!("{}", fl!("usb-detected", device = client.profile.name));
                    return Ok(client);
                }
                Err(HxError::DeviceNotFound) => continue,
                Err(e) => return Err(e),
            }
        }
        Err(HxError::DeviceNotFound)
    }

    /// Internal routine to handle the low-level `libusb` setup for a matched profile.
    ///
    /// This handles detaching default OS kernel drivers, setting the active configuration,
    /// claiming the proprietary interface, and clearing any endpoint halts.
    fn connect_profile(profile: &'static DeviceProfile) -> Result<Self, HxError> {
        let context = Context::new()?;

        let handle = context
            .open_device_with_vid_pid(profile.vendor_id, profile.product_id)
            .ok_or(HxError::DeviceNotFound)?;

        // Ask libusb to detach any kernel driver bound to the interface so we
        // can claim it exclusively.
        // Platforms without kernel driver detach (e.g. Windows) report
        // `NotSupported`; there is nothing to detach there, so carry on.
        match handle.set_auto_detach_kernel_driver(true) {
            Ok(()) | Err(rusb::Error::NotSupported) => {}
            Err(e) => {
                return Err(HxError::protocol(fl!(
                    "usb-kernel-detach-failed",
                    error = e.to_string()
                )));
            }
        }

        // Ensure configuration 1 — the only configuration that exposes the
        // vendor-specific bulk endpoints.
        if handle.active_configuration()? != 1 {
            handle.set_active_configuration(1)?;
        }

        handle.claim_interface(INTERFACE)?;
        handle.clear_halt(EP_OUT)?;
        handle.clear_halt(EP_IN)?;

        let client = Self { handle, profile };

        // Discard data buffered by a prior unclosed session so it cannot
        // misalign the subsequent request/response pairing.
        client.drain_stale_data();

        Ok(client)
    }

    /// Core retry loop for reading presets, extracted to avoid name collision
    /// with the [`DeviceClient`] trait method of the same name.
    fn read_presets_impl(&self, setlist: u8) -> Result<Vec<Preset>, HxError> {
        let timeout = Duration::from_millis(TIMEOUT_MS);

        for attempt in 0..MAX_INIT_RETRIES {
            if attempt > 0 {
                self.wait_with_backoff(attempt);
            }
            // Drain runs on every attempt, not just retries: the previous
            // transaction's trailing packets persist in the IN buffer
            // across calls on a cached client.
            self.drain_stale_data();

            match self.run_session(setlist, timeout) {
                Ok(mut presets) => {
                    presets.sort_unstable_by_key(|p| p.index);
                    return Ok(presets);
                }
                Err(HxError::Usb(rusb::Error::Timeout)) if attempt + 1 < MAX_INIT_RETRIES => {
                    continue;
                }
                Err(e) => return Err(e),
            }
        }

        Err(HxError::protocol(fl!(
            "usb-device-unresponsive",
            device = self.profile.name,
            attempts = MAX_INIT_RETRIES
        )))
    }

    /// Core retry loop for listing setlists, extracted to avoid name collision
    /// with the [`DeviceClient`] trait method of the same name.
    fn list_setlists_impl(&self) -> Result<Vec<Setlist>, HxError> {
        let timeout = Duration::from_millis(TIMEOUT_MS);

        for attempt in 0..MAX_INIT_RETRIES {
            if attempt > 0 {
                self.wait_with_backoff(attempt);
            }
            self.drain_stale_data();

            match self.run_setlists_session(timeout) {
                Ok(setlists) => return Ok(setlists),
                Err(HxError::Usb(rusb::Error::Timeout)) if attempt + 1 < MAX_INIT_RETRIES => {
                    continue;
                }
                Err(e) => return Err(e),
            }
        }

        Err(HxError::protocol(fl!(
            "usb-device-unresponsive",
            device = self.profile.name,
            attempts = MAX_INIT_RETRIES
        )))
    }

    /// Opens the presets resource and parses the setlist names from its reply.
    ///
    /// The device answers the open-presets command with its setlist table, so
    /// this needs no packet beyond the ones a preset listing already sends.
    fn run_setlists_session(&self, timeout: Duration) -> Result<Vec<Setlist>, HxError> {
        let mut buf = vec![0u8; 512];

        self.session_init(&mut buf, timeout)?;

        self.handle.write_bulk(EP_OUT, OPEN_PRESETS, timeout)?;
        let n = self.read_for_seq(OPEN_PRESETS[SEQ_BYTE_OFFSET], &mut buf, timeout)?;

        let setlists = parse_setlists(response_payload(&buf, n)?)?;

        self.drain_stale_data();

        Ok(setlists)
    }

    /// Core retry loop for selecting a preset, extracted to avoid name collision
    /// with the [`DeviceClient`] trait method of the same name.
    fn select_preset_impl(&self, setlist: u8, preset: u8) -> Result<(), HxError> {
        let timeout = Duration::from_millis(TIMEOUT_MS);

        for attempt in 0..MAX_INIT_RETRIES {
            if attempt > 0 {
                self.wait_with_backoff(attempt);
            }
            self.drain_stale_data();

            match self.run_select_session(setlist, preset, timeout) {
                Ok(_) => return Ok(()),
                Err(HxError::Usb(rusb::Error::Timeout)) if attempt + 1 < MAX_INIT_RETRIES => {
                    continue;
                }
                Err(e) => return Err(e),
            }
        }

        Err(HxError::protocol(fl!(
            "usb-device-unresponsive",
            device = self.profile.name,
            attempts = MAX_INIT_RETRIES
        )))
    }

    /// Internal routine to handle the session cycle specifically for changing a preset.
    fn run_select_session(
        &self,
        setlist: u8,
        preset: u8,
        timeout: Duration,
    ) -> Result<(), HxError> {
        let mut buf = vec![0u8; 512];

        self.session_init(&mut buf, timeout)?;

        // Send the Select Preset command (after handshake, the next seq is 0x06)
        let seq = 0x06;
        let request = build_select_preset_request(seq, setlist, preset);

        self.handle.write_bulk(EP_OUT, &request, timeout)?;
        self.read_for_seq(seq, &mut buf, timeout)?;

        // Consume the burst of notifications triggered by the device after
        // the preset change before releasing the endpoint for the next operation.
        self.drain_stale_data();

        Ok(())
    }

    fn run_edit_session(
        &self,
        transaction: u16,
        operation: i64,
        target: rmpv::Value,
        timeout: Duration,
    ) -> Result<(), HxError> {
        let mut buf = vec![0u8; 512];

        for packet in edit_channel_handshake() {
            let seq = packet[SEQ_BYTE_OFFSET];
            self.handle.write_bulk(EP_OUT, &packet, timeout)?;
            let n = self.read_for_seq(seq, &mut buf, timeout)?;
            tracing::debug!(
                stage = "edit-handshake",
                seq,
                len = n,
                hex = format!("{:02X?}", &buf[..n.min(32)]),
                "edit read response"
            );
        }

        let request = build_edit_request(0x04, 0x0000_1009, transaction, operation, target);
        self.handle.write_bulk(EP_OUT, &request, timeout)?;
        let n = self.read_for_seq(0x04, &mut buf, timeout)?;
        parse_edit_response(&buf[..n])?;
        self.drain_stale_data();
        Ok(())
    }

    fn read_current_preset_impl(&self) -> Result<crate::models::PresetDetails, HxError> {
        let timeout = Duration::from_millis(TIMEOUT_MS);
        let mut buf = vec![0u8; 8 * 1024];
        self.drain_stale_data();

        for packet in edit_channel_handshake() {
            let seq = packet[SEQ_BYTE_OFFSET];
            self.handle.write_bulk(EP_OUT, &packet, timeout)?;
            self.read_for_seq(seq, &mut buf, timeout)?;
        }

        let request = |sequence: u8, transaction: u16, operation: i64, target: rmpv::Value| {
            build_edit_request(sequence, 0x0000_1009, transaction, operation, target)
        };
        for (sequence, (transaction, operation, target)) in [
            (1, 76, rmpv::Value::Map(Vec::new())),
            (
                2,
                24,
                rmpv::Value::Map(vec![(rmpv::Value::from(118i64), rmpv::Value::from(128i64))]),
            ),
            (3, 23, rmpv::Value::Nil),
        ]
        .into_iter()
        .enumerate()
        {
            let sequence = 0x04u8 + sequence as u8;
            let packet = request(sequence, transaction, operation, target);
            self.handle.write_bulk(EP_OUT, &packet, timeout)?;
            let n = self.read_for_seq(sequence, &mut buf, timeout)?;
            tracing::debug!(
                stage = operation,
                seq = sequence,
                len = n,
                hex = format!("{:02X?}", &buf[..n.min(32)]),
                "edit read response"
            );
            parse_edit_response(&buf[..n])?;
        }

        let packet = request(0x07, 4, 22, rmpv::Value::Nil);
        self.handle.write_bulk(EP_OUT, &packet, timeout)?;
        let n = self.read_for_seq(0x07, &mut buf, timeout)?;
        tracing::debug!(
            stage = 22,
            seq = 0x07,
            len = n,
            hex = format!("{:02X?}", &buf[..n.min(32)]),
            "edit read response"
        );
        let declared_payload_len =
            u32::from_le_bytes(buf[20..24].try_into().expect("stream length header")) as usize;
        let expected_stream_len = declared_payload_len
            .checked_add(RESPONSE_PAYLOAD_OFFSET - RESPONSE_HEADER_SIZE)
            .ok_or_else(|| HxError::protocol("preset stream length overflow"))?;
        let mut stream = buf[RESPONSE_HEADER_SIZE..n].to_vec();
        let mut seq = 8u8;
        let mut offset = 0x0000_1009u32;
        let mut empty_chunks = 0u8;

        while stream.len() < expected_stream_len && stream.len() < 64 * 1024 {
            if parse_preset_document(&stream).is_ok() {
                break;
            }
            let packet = build_edit_chunk_request(seq, offset);
            self.handle.write_bulk(EP_OUT, &packet, timeout)?;
            let n = self.read_for_seq(seq, &mut buf, timeout)?;
            tracing::debug!(
                stage = "edit-chunk",
                seq,
                len = n,
                hex = format!("{:02X?}", &buf[..n.min(32)]),
                "edit stream response"
            );
            if n <= RESPONSE_HEADER_SIZE {
                empty_chunks = empty_chunks.saturating_add(1);
                if empty_chunks >= 8 {
                    return Err(HxError::protocol(
                        "preset stream returned too many empty credit frames",
                    ));
                }
                seq = seq.wrapping_add(1);
                continue;
            }
            let chunk = &buf[RESPONSE_HEADER_SIZE..n];
            empty_chunks = 0;
            stream.extend_from_slice(chunk);
            if stream.len() > expected_stream_len {
                stream.truncate(expected_stream_len);
            }
            seq = seq.wrapping_add(1);
            offset = offset
                .checked_add(0x100)
                .ok_or_else(|| HxError::protocol("preset stream offset overflow"))?;
        }

        let details = parse_preset_document(&stream)?;
        self.drain_stale_data();
        Ok(details)
    }

    /// Executes a single, complete preset extraction session for `setlist`.
    fn run_session(&self, setlist: u8, timeout: Duration) -> Result<Vec<Preset>, HxError> {
        let mut buf = vec![0u8; 512];
        let mut raw_stream: Vec<u8> = Vec::with_capacity(4_096);

        self.session_init(&mut buf, timeout)?;

        // Phase 1: open presets resource
        self.handle.write_bulk(EP_OUT, OPEN_PRESETS, timeout)?;
        self.read_for_seq(OPEN_PRESETS[SEQ_BYTE_OFFSET], &mut buf, timeout)?;

        // Phase 2: start stream for the requested setlist (first response
        // carries payload)
        let open_stream = build_open_stream_request(OPEN_STREAM_SEQ, setlist);
        self.handle.write_bulk(EP_OUT, &open_stream, timeout)?;
        let n = self.read_for_seq(OPEN_STREAM_SEQ, &mut buf, timeout)?;
        self.collect_payload(&mut raw_stream, &buf, n);

        // Phase 3: paginate until end-of-stream
        let mut seq: u8 = PAGINATION_INITIAL_SEQ;
        let mut offset: u32 = STREAM_INITIAL_OFFSET;

        loop {
            let request = build_pagination_request(seq, offset);
            self.handle.write_bulk(EP_OUT, &request, timeout)?;
            let n = self.read_for_seq(seq, &mut buf, timeout)?;
            self.collect_payload(&mut raw_stream, &buf, n);

            // Phase 4: a response shorter than MAX_CHUNK_SIZE signals
            // end-of-stream.
            if n < MAX_CHUNK_SIZE {
                break;
            }

            offset = offset
                .checked_add(OFFSET_STEP)
                .ok_or_else(|| HxError::protocol(fl!("usb-stream-offset-overflow")))?;
            seq = seq.wrapping_add(1);
        }

        parse_msgpack_stream(&raw_stream, self.profile.preset_count, setlist)
    }

    /// Appends the MessagePack payload portion of a bulk IN response to `dest`.
    ///
    /// Each response is prefixed by [`RESPONSE_HEADER_SIZE`] bytes that are
    /// not part of the MessagePack stream and must be skipped.
    #[inline]
    fn collect_payload(&self, dest: &mut Vec<u8>, buf: &[u8], n: usize) {
        if n > RESPONSE_HEADER_SIZE {
            dest.extend_from_slice(&buf[RESPONSE_HEADER_SIZE..n]);
        }
    }

    /// Flushes the USB IN endpoint of any unread data.
    ///
    /// Every transaction is followed by a short, unsolicited "epilogue":
    /// a handful of header-only frames (one with `cmd = 0x08`, then two
    /// or three with `cmd = 0x10` — a code the host never emits) carrying
    /// the just-completed transaction's ID and seq numbers continuing
    /// past the last ACK. They land in the IN buffer between calls.
    /// Consuming them here keeps the next session's request/response
    /// pairing aligned and also clears any leftover from a crashed
    /// session. The [`DRAIN_TIMEOUT_MS`] window assumes the tail finishes
    /// within ~50 ms of the last packet; [`Self::read_for_seq`] guards
    /// against the case where it doesn't.
    fn drain_stale_data(&self) {
        let timeout = Duration::from_millis(DRAIN_TIMEOUT_MS);
        let mut buf = vec![0u8; 512];
        while let Ok(n) = self.handle.read_bulk(EP_IN, &mut buf, timeout) {
            tracing::debug!(
                seq = fmt_byte_at(&buf, n, SEQ_BYTE_OFFSET),
                cmd = fmt_byte_at(&buf, n, CMD_BYTE_OFFSET),
                len = n,
                hex = format!("{:02X?}", &buf[..n.min(16)]),
                "drained stale pkt",
            );
        }
    }

    /// Reads from the IN endpoint until the response's sequence byte
    /// matches `expected`, discarding stale or out-of-band packets along
    /// the way.
    ///
    /// Out-of-band packets that arrive between transactions can land in
    /// the IN buffer ahead of the ACK and shift every subsequent read by
    /// one. Validating the sequence byte at [`SEQ_BYTE_OFFSET`] lets the
    /// caller discard such packets inline as a safety net behind
    /// [`Self::drain_stale_data`]. Each re-read uses the caller's full
    /// timeout: a genuine reply queued behind a stale packet returns
    /// immediately from the OS buffer regardless, and a shorter window
    /// would only risk misclassifying a slow-but-valid read as stale.
    fn read_for_seq(
        &self,
        expected: u8,
        buf: &mut [u8],
        timeout: Duration,
    ) -> Result<usize, HxError> {
        for drops in 0..=MAX_STALE_DROPS {
            let n = self.handle.read_bulk(EP_IN, buf, timeout)?;
            if n > SEQ_BYTE_OFFSET && buf[SEQ_BYTE_OFFSET] == expected {
                return Ok(n);
            }
            tracing::debug!(
                expected = format!("0x{expected:02X}"),
                got = fmt_byte_at(buf, n, SEQ_BYTE_OFFSET),
                cmd = fmt_byte_at(buf, n, CMD_BYTE_OFFSET),
                len = n,
                hex = format!("{:02X?}", &buf[..n.min(16)]),
                drop = drops + 1,
                cap = MAX_STALE_DROPS,
                "discarding out-of-band USB pkt",
            );
        }
        Err(HxError::Usb(rusb::Error::Timeout))
    }

    /// Runs the full five-packet session-init exchange.
    ///
    /// The channel handshake is delegated to [`Self::handshake`], which
    /// retries internally on a dropped first write. The remaining four
    /// packets respond reliably once the channel is up and use the
    /// standard write-then-[`Self::read_for_seq`] path.
    fn session_init(&self, buf: &mut [u8], timeout: Duration) -> Result<(), HxError> {
        self.handshake(buf, timeout)?;
        for packet in &SESSION_INIT_SEQUENCE[1..] {
            self.handle.write_bulk(EP_OUT, packet, timeout)?;
            self.read_for_seq(packet[SEQ_BYTE_OFFSET], buf, timeout)?;
        }
        Ok(())
    }

    /// Performs the channel handshake, retrying internally on timeout.
    ///
    /// The first [`HANDSHAKE`] write of a session is occasionally dropped
    /// by the device while it finishes settling from the previous session.
    /// A short-timeout resend handled here recovers in roughly
    /// [`HANDSHAKE_RETRY_TIMEOUT_MS`] milliseconds, avoiding the outer
    /// retry's full read timeout plus exponential back-off per occurrence.
    fn handshake(&self, buf: &mut [u8], outer_timeout: Duration) -> Result<(), HxError> {
        let short = Duration::from_millis(HANDSHAKE_RETRY_TIMEOUT_MS);
        let expected = HANDSHAKE[SEQ_BYTE_OFFSET];

        for attempt in 0..HANDSHAKE_INNER_RETRIES {
            self.handle.write_bulk(EP_OUT, HANDSHAKE, outer_timeout)?;
            match self.read_for_seq(expected, buf, short) {
                Ok(_) => return Ok(()),
                Err(HxError::Usb(rusb::Error::Timeout))
                    if attempt + 1 < HANDSHAKE_INNER_RETRIES => {}
                Err(e) => return Err(e),
            }
        }
        Err(HxError::Usb(rusb::Error::Timeout))
    }

    /// Suspends the current thread according to the exponential backoff curve.
    ///
    /// Called when entering retry iteration `attempt` (≥ 1), so `attempt`
    /// failures have already happened — that's the value to report.
    fn wait_with_backoff(&self, attempt: u32) {
        let wait_ms = BASE_BACKOFF_MS * (1 << attempt);

        eprintln!(
            "{}",
            fl!(
                "usb-retry-attempt",
                device = self.profile.name,
                current = attempt,
                total = MAX_INIT_RETRIES,
                wait_ms = wait_ms
            )
        );
        std::thread::sleep(Duration::from_millis(wait_ms));
    }
}

impl DeviceClient for Client {
    #[inline]
    fn profile(&self) -> &'static DeviceProfile {
        self.profile
    }

    #[inline]
    fn read_presets(&self) -> Result<Vec<Preset>, HxError> {
        self.read_presets_impl(0)
    }

    #[inline]
    fn read_setlist_presets(&self, setlist: u8) -> Result<Vec<Preset>, HxError> {
        self.profile.validate_setlist(setlist)?;
        self.read_presets_impl(setlist)
    }

    #[inline]
    fn list_setlists(&self) -> Result<Vec<Setlist>, HxError> {
        self.list_setlists_impl()
    }

    #[inline]
    fn select_preset(&self, setlist: u8, preset: u8) -> Result<(), HxError> {
        self.profile.validate_setlist(setlist)?;
        self.select_preset_impl(setlist, preset)
    }

    fn read_current_preset(&self) -> Result<crate::models::PresetDetails, HxError> {
        self.read_current_preset_impl()
    }

    fn read_current_preset_index(&self) -> Result<Option<u8>, HxError> {
        let timeout = Duration::from_millis(TIMEOUT_MS);
        let mut buf = vec![0u8; 8 * 1024];
        self.drain_stale_data();

        let mut handshake = || -> Result<(), HxError> {
            for packet in edit_channel_handshake() {
                let seq = packet[SEQ_BYTE_OFFSET];
                self.handle.write_bulk(EP_OUT, &packet, timeout)?;
                self.read_for_seq(seq, &mut buf, timeout)?;
            }
            Ok(())
        };

        let request = |sequence: u8, transaction: u16, operation: i64, target: rmpv::Value| {
            build_edit_request(sequence, 0x0000_1009, transaction, operation, target)
        };

        let mut last_response = None;
        if let Err(error) = handshake() {
            return match error {
                HxError::Usb(rusb::Error::Timeout)
                | HxError::Protocol(_)
                | HxError::MsgPack(_)
                | HxError::InvalidStreamMarker
                | HxError::DeviceNotFound => Ok(None),
                _ => Err(error),
            };
        }

        for (index, (transaction, operation, target)) in [
            (1, 76, rmpv::Value::Map(Vec::new())),
            (
                2,
                24,
                rmpv::Value::Map(vec![(rmpv::Value::from(118i64), rmpv::Value::from(128i64))]),
            ),
            (3, 23, rmpv::Value::Nil),
        ]
        .into_iter()
        .enumerate()
        {
            let sequence = 0x04u8 + index as u8;
            let packet = request(sequence, transaction, operation, target);
            match self.handle.write_bulk(EP_OUT, &packet, timeout) {
                Ok(_) => {}
                Err(rusb::Error::Timeout) => return Ok(None),
                Err(err) => return Err(HxError::Usb(err)),
            }

            let n = match self.read_for_seq(sequence, &mut buf, timeout) {
                Ok(len) => len,
                Err(HxError::Usb(rusb::Error::Timeout)) => return Ok(None),
                Err(err) => return Err(err),
            };

            if let Err(error) = parse_edit_response(&buf[..n]) {
                match error {
                    HxError::Usb(rusb::Error::Timeout)
                    | HxError::Protocol(_)
                    | HxError::MsgPack(_)
                    | HxError::InvalidStreamMarker => return Ok(None),
                    _ => return Err(error),
                }
            }
            last_response = Some(buf[..n].to_vec());
        }

        match last_response.as_deref() {
            Some(response) => parse_current_preset_identity_response(response),
            None => Ok(None),
        }
    }

    fn rename_preset(&self, setlist: u8, preset: u8, name: &str) -> Result<(), HxError> {
        self.profile.validate_setlist(setlist)?;
        if u16::from(preset) >= self.profile.preset_count {
            return Err(HxError::protocol(format!(
                "preset {preset} out of range (0–{})",
                self.profile.preset_count - 1
            )));
        }
        if name.is_empty() || name.contains('\0') {
            return Err(HxError::protocol(
                "preset name must be non-empty and contain no NUL",
            ));
        }
        self.run_edit_session(
            1,
            6,
            rmpv::Value::Map(vec![
                (rmpv::Value::from(107i64), rmpv::Value::from(setlist)),
                (rmpv::Value::from(108i64), rmpv::Value::from(preset)),
                (
                    rmpv::Value::from(109i64),
                    rmpv::Value::from(format!("{name}\0")),
                ),
            ]),
            Duration::from_millis(TIMEOUT_MS),
        )
    }

    fn save_preset(&self, setlist: u8, preset: u8, name: &str) -> Result<(), HxError> {
        self.profile.validate_setlist(setlist)?;
        if u16::from(preset) >= self.profile.preset_count {
            return Err(HxError::protocol(format!(
                "preset {preset} out of range (0–{})",
                self.profile.preset_count - 1
            )));
        }
        if name.is_empty() || name.contains('\0') {
            return Err(HxError::protocol(
                "preset name must be non-empty and contain no NUL",
            ));
        }
        self.run_edit_session(
            1,
            71,
            rmpv::Value::Map(vec![
                (rmpv::Value::from(107i64), rmpv::Value::from(setlist)),
                (rmpv::Value::from(108i64), rmpv::Value::from(preset)),
                (
                    rmpv::Value::from(109i64),
                    rmpv::Value::from(format!("{name}\0")),
                ),
            ]),
            Duration::from_millis(TIMEOUT_MS),
        )
    }

    fn move_block(&self, source: u8, destination: u8, temporary: u8) -> Result<(), HxError> {
        if source >= 40 || destination >= 40 || temporary >= 40 {
            return Err(HxError::protocol("block slot must be below 40"));
        }
        if source / 20 != destination / 20 || source / 20 != temporary / 20 {
            return Err(HxError::protocol(
                "block moves must stay within the same DSP path",
            ));
        }
        if source == destination || source == temporary || destination == temporary {
            return Err(HxError::protocol("block move slots must be distinct"));
        }

        for (source_slot, destination_slot) in [
            (source, temporary),
            (destination, source),
            (temporary, destination),
        ] {
            self.run_edit_session(
                1,
                78,
                rmpv::Value::Map(vec![
                    (rmpv::Value::from(98i64), rmpv::Value::from(source_slot)),
                    (rmpv::Value::from(26i64), rmpv::Value::from(0i64)),
                ]),
                Duration::from_millis(TIMEOUT_MS),
            )?;
            self.run_edit_session(
                1,
                43,
                rmpv::Value::Map(vec![
                    (rmpv::Value::from(75i64), rmpv::Value::from(source_slot)),
                    (
                        rmpv::Value::from(76i64),
                        rmpv::Value::from(destination_slot),
                    ),
                ]),
                Duration::from_millis(TIMEOUT_MS),
            )?;
        }
        Ok(())
    }

    fn set_parameter(
        &self,
        block: u8,
        parameter: u8,
        value: f32,
        model: u8,
    ) -> Result<(), HxError> {
        if model > 1 {
            return Err(HxError::protocol("model must be 0 (main) or 1 (paired)"));
        }
        if !value.is_finite() {
            return Err(HxError::protocol("parameter value must be finite"));
        }
        self.run_edit_session(
            1,
            30,
            rmpv::Value::Map(vec![
                (rmpv::Value::from(98i64), rmpv::Value::from(block)),
                (rmpv::Value::from(29i64), rmpv::Value::Boolean(true)),
                (rmpv::Value::from(26i64), rmpv::Value::from(model)),
                (rmpv::Value::from(28i64), rmpv::Value::from(parameter)),
                (rmpv::Value::from(119i64), rmpv::Value::F32(value)),
            ]),
            Duration::from_millis(TIMEOUT_MS),
        )
    }
}

fn parse_current_preset_identity_response(response: &[u8]) -> Result<Option<u8>, HxError> {
    if response.len() < 24 {
        return Err(HxError::protocol("edit response is too short"));
    }

    let body = &response[16..];
    if body.len() < 8 {
        return Err(HxError::protocol("edit response has no TLV body"));
    }

    let len = u32::from_le_bytes(body[4..8].try_into().unwrap()) as usize;
    let end = 8usize
        .checked_add(len)
        .filter(|end| *end <= body.len())
        .ok_or_else(|| HxError::protocol("edit response TLV is truncated"))?;
    let mut cursor = std::io::Cursor::new(&body[8..end]);
    let value = rmpv::decode::read_value(&mut cursor)?;
    let candidate_maps = std::iter::once(value.clone()).chain(
        value
            .as_map()
            .into_iter()
            .flat_map(|map| {
                map.iter()
                    .filter_map(|(key, candidate)| (key.as_i64() == Some(104)).then_some(candidate))
            })
            .cloned(),
    );

    for candidate in candidate_maps {
        let Some(map) = candidate.as_map() else {
            continue;
        };
        if let Some(index) = map.iter().find_map(|(key, val)| {
            if key.as_i64() == Some(108) {
                val.as_i64().and_then(|value| u8::try_from(value).ok())
            } else {
                None
            }
        }) {
            return Ok(Some(index));
        }
    }

    Ok(None)
}

fn parse_edit_response(response: &[u8]) -> Result<(), HxError> {
    if response.len() < 24 {
        return Err(HxError::protocol("edit response is too short"));
    }

    let body = &response[16..];
    if body.len() < 8 {
        return Err(HxError::protocol("edit response has no TLV body"));
    }
    let len = u32::from_le_bytes(body[4..8].try_into().unwrap()) as usize;
    let end = 8usize
        .checked_add(len)
        .filter(|end| *end <= body.len())
        .ok_or_else(|| HxError::protocol("edit response TLV is truncated"))?;
    let mut cursor = std::io::Cursor::new(&body[8..end]);
    let value = rmpv::decode::read_value(&mut cursor)?;
    let status = value
        .as_map()
        .and_then(|map| {
            map.iter()
                .find(|(key, _)| key.as_i64() == Some(103))
                .and_then(|(_, value)| value.as_i64())
        })
        .unwrap_or(0);
    if status != 0 && status != 1 {
        return Err(HxError::protocol(format!(
            "device rejected edit with status {status}"
        )));
    }
    Ok(())
}

impl Drop for Client {
    /// Ensures the USB interface is cleanly released back to the OS when the
    /// client is dropped.
    fn drop(&mut self) {
        let _ = self.handle.release_interface(INTERFACE);
    }
}
