//! Frame encryption that stays outside RTP packetization.
//!
//! This module is deliberately not wired into `PlainSender` or the receivers yet. A sender owns
//! one instance for each origin and epoch, while the receiver owns the matching replay state.

use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chacha20poly1305::aead::{AeadInPlace, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Nonce, Tag};

/// A media key is always 256 bits because ChaCha20-Poly1305 does not have a shorter key mode.
pub const MEDIA_KEY_LEN: usize = 32;
const ORIGIN_PREFIX_LEN: usize = 4;
const NONCE_LEN: usize = 12;
const TAG_LEN: usize = 16;
const FOOTER_MAGIC: [u8; 16] = *b"UNKVOID-E2EE-v1!";
const FOOTER_LEN: usize = FOOTER_MAGIC.len() + std::mem::size_of::<u64>() * 2 + TAG_LEN;
/// The video packetizer scans for Annex-B start codes, so binary footer bytes must not follow
/// the last NAL unescaped. This fixed ASCII form cannot contain `00 00 01`.
const H264_FOOTER_MAGIC: [u8; 16] = *b"UNKVOID-H264-v1!";
const H264_FOOTER_DATA_LEN: usize = 43; // URL-safe base64 without padding for 32 bytes.
const H264_FOOTER_LEN: usize = H264_FOOTER_MAGIC.len() + H264_FOOTER_DATA_LEN;
const AAD_DOMAIN: &[u8] = b"unkvoid-media-e2ee-v1";
const H264_START_CODE: [u8; 3] = [0, 0, 1];
const REPLAY_WINDOW_SIZE: u64 = 128;

/// Commits and their first new-epoch packets can cross in flight. Three seconds covers the RTP
/// repair/jitter path without turning a removed member's old key into a long-lived fallback.
pub const PREVIOUS_EPOCH_WINDOW: Duration = Duration::from_secs(3);

/// The MLS epoch that derived the sender key.
pub type Epoch = u64;

/// The unique four-byte nonce prefix assigned to one media origin in an epoch.
pub type Origin = [u8; ORIGIN_PREFIX_LEN];

/// The public metadata at the end of every protected frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameMetadata {
    pub epoch: Epoch,
    pub counter: u64,
}

/// A decoded payload distinguishes legacy clear media from media opened by this module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecryptedPayload {
    Clear(Vec<u8>),
    Decrypted(Vec<u8>),
}

impl DecryptedPayload {
    /// Returns the bytes regardless of whether the input frame had an E2EE footer.
    pub fn into_bytes(self) -> Vec<u8> {
        match self {
            Self::Clear(bytes) | Self::Decrypted(bytes) => bytes,
        }
    }

    /// Whether authentication succeeded for an E2EE-protected frame.
    pub fn was_encrypted(&self) -> bool {
        matches!(self, Self::Decrypted(_))
    }
}

/// A rejected frame never yields payload bytes.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum E2eeError {
    #[error("the media frame counter is exhausted")]
    CounterExhausted,

    #[error("the media frame belongs to a different epoch")]
    EpochMismatch,

    #[error("the media frame counter was replayed or is outside the replay window")]
    CounterRegression,

    #[error("the room requires encrypted media, but the frame has no E2EE footer")]
    Unencrypted,

    #[error("the media frame authentication tag did not verify")]
    AuthenticationFailed,

    #[error("the H.264 Annex-B access unit is malformed")]
    MalformedH264,

    #[error("the media frame is too large to encode")]
    FrameTooLarge,
}

/// Whether frames without an E2EE footer are still allowed for this receiver.
///
/// `AllowClear` exists only for explicit legacy rooms. An E2EE room must select `Require`, so a
/// relay cannot downgrade a protected stream by injecting a clear frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Policy {
    Require,
    AllowClear,
}

/// Encrypts consecutive frames for one origin and one epoch.
pub struct MediaSender {
    key: [u8; MEDIA_KEY_LEN],
    epoch: Epoch,
    origin: Origin,
    next_counter: Option<u64>,
}

impl MediaSender {
    /// Starts a fresh epoch at counter zero.
    pub fn new(key: [u8; MEDIA_KEY_LEN], epoch: Epoch, origin: Origin) -> Self {
        Self {
            key,
            epoch,
            origin,
            next_counter: Some(0),
        }
    }

    /// Encrypts a complete Opus payload before its RTP packet is made.
    pub fn encrypt_opus(&mut self, opus: &[u8]) -> Result<Vec<u8>, E2eeError> {
        let metadata = self.next_metadata()?;
        let aad = payload_aad(PayloadKind::Opus, self.origin, metadata);
        let (ciphertext, tag) = seal(&self.key, self.origin, metadata.counter, opus, &aad)?;

        Ok(with_footer(ciphertext, metadata, tag))
    }

    /// Encrypts an Annex-B access unit before H.264 RTP packetization can make FU-A or STAP-A.
    pub fn encrypt_h264(&mut self, annex_b: &[u8]) -> Result<Vec<u8>, E2eeError> {
        // O empacotador RTP não manda AUD (9) nem enchimento (12): contados aqui, faltariam do
        // outro lado e a tag de todo quadro falharia. O encoder da NVIDIA abre cada um com AUD.
        let nals: Vec<Nal<'_>> = parse_clear_annex_b(annex_b)?
            .into_iter()
            .filter(|nal| !matches!(nal.header & 0x1f, 9 | 12))
            .collect();
        let metadata = self.next_metadata()?;
        let aad = h264_aad(self.origin, metadata, &nals)?;
        let plaintext = join_payloads(&nals);
        let (ciphertext, tag) = seal(&self.key, self.origin, metadata.counter, &plaintext, &aad)?;

        let mut protected = Vec::with_capacity(annex_b.len() + H264_FOOTER_LEN + nals.len());
        let mut offset = 0;

        for nal in &nals {
            let end = offset + nal.payload.len();
            let body = ciphertext
                .get(offset..end)
                .ok_or(E2eeError::AuthenticationFailed)?;

            protected.extend(H264_START_CODE);
            protected.push(nal.header);
            append_emulation_prevention(&mut protected, body);
            offset = end;
        }

        Ok(with_h264_footer(protected, metadata, tag))
    }

    fn next_metadata(&mut self) -> Result<FrameMetadata, E2eeError> {
        let counter = self.next_counter.ok_or(E2eeError::CounterExhausted)?;

        self.next_counter = counter.checked_add(1);

        Ok(FrameMetadata {
            epoch: self.epoch,
            counter,
        })
    }
}

/// Opens consecutive frames for one origin and one epoch.
pub struct MediaReceiver {
    key: [u8; MEDIA_KEY_LEN],
    epoch: Epoch,
    origin: Origin,
    policy: Policy,
    replay: ReplayWindow,
}

impl MediaReceiver {
    pub fn new(key: [u8; MEDIA_KEY_LEN], epoch: Epoch, origin: Origin, policy: Policy) -> Self {
        Self {
            key,
            epoch,
            origin,
            policy,
            replay: ReplayWindow::default(),
        }
    }

    /// The epoch this receiver's key authenticates.
    pub fn epoch(&self) -> Epoch {
        self.epoch
    }

    /// Opens a complete Opus payload. A payload without an E2EE footer remains compatible.
    pub fn decrypt_opus(&mut self, payload: &[u8]) -> Result<DecryptedPayload, E2eeError> {
        let Some((ciphertext, footer)) = split_footer(payload) else {
            return self.clear_or_reject(payload);
        };

        let replay_update = self.validate_metadata(footer.metadata)?;
        let aad = payload_aad(PayloadKind::Opus, self.origin, footer.metadata);
        let plaintext = open(
            &self.key,
            self.origin,
            footer.metadata.counter,
            ciphertext,
            footer.tag,
            &aad,
        )?;

        // The bit is only committed after AEAD authentication succeeds. A forged packet must
        // not be able to consume a counter and prevent delivery of the real frame.
        self.replay.commit(replay_update);

        Ok(DecryptedPayload::Decrypted(plaintext))
    }

    /// Opens a complete reassembled Annex-B access unit, before it reaches an H.264 decoder.
    pub fn decrypt_h264(&mut self, annex_b: &[u8]) -> Result<DecryptedPayload, E2eeError> {
        let Some((protected, footer)) = split_h264_footer(annex_b) else {
            return self.clear_or_reject(annex_b);
        };

        let replay_update = self.validate_metadata(footer.metadata)?;

        let protected_nals = parse_protected_annex_b(protected)?;
        let ciphertext_payloads: Vec<Vec<u8>> = protected_nals
            .iter()
            .map(|nal| remove_emulation_prevention(nal.payload))
            .collect::<Result<_, _>>()?;
        let ciphertext_nals: Vec<Nal<'_>> = protected_nals
            .iter()
            .zip(&ciphertext_payloads)
            .map(|(nal, payload)| Nal {
                header: nal.header,
                payload,
            })
            .collect();

        let aad = h264_aad(self.origin, footer.metadata, &ciphertext_nals)?;
        let ciphertext = join_payloads(&ciphertext_nals);
        let plaintext = open(
            &self.key,
            self.origin,
            footer.metadata.counter,
            &ciphertext,
            footer.tag,
            &aad,
        )?;
        let mut clear = Vec::with_capacity(protected.len());
        let mut offset = 0;

        for nal in &ciphertext_nals {
            let end = offset + nal.payload.len();
            let body = plaintext
                .get(offset..end)
                .ok_or(E2eeError::AuthenticationFailed)?;

            clear.extend(H264_START_CODE);
            clear.push(nal.header);
            clear.extend(body);
            offset = end;
        }

        // As above, do not change replay state before the tag covers every NAL header/body.
        self.replay.commit(replay_update);

        Ok(DecryptedPayload::Decrypted(clear))
    }

    fn clear_or_reject(&self, payload: &[u8]) -> Result<DecryptedPayload, E2eeError> {
        match self.policy {
            Policy::Require => Err(E2eeError::Unencrypted),
            Policy::AllowClear => Ok(DecryptedPayload::Clear(payload.to_vec())),
        }
    }

    fn validate_metadata(&self, metadata: FrameMetadata) -> Result<ReplayUpdate, E2eeError> {
        if metadata.epoch != self.epoch {
            return Err(E2eeError::EpochMismatch);
        }

        self.replay.prepare(metadata.counter)
    }
}

/// Selects the current receiver or the one immediately before it at the media boundary.
///
/// MLS itself never accepts a previous epoch as current group state. This narrowly scoped
/// window is only for RTP that was sent before the ordered Commit reached a peer and was delayed
/// by packetization, retransmission, or the local playout queue.
pub struct MediaReceiveWindow {
    current: MediaReceiver,
    previous: Option<(Instant, MediaReceiver)>,
}

impl MediaReceiveWindow {
    pub fn new(current: MediaReceiver) -> Self {
        Self {
            current,
            previous: None,
        }
    }

    /// Installs a new MLS epoch and grants the old receiver one short delivery window.
    pub fn rotate(&mut self, next: MediaReceiver) {
        if next.epoch() == self.current.epoch() {
            self.current = next;

            return;
        }

        self.previous = Some((
            Instant::now() + PREVIOUS_EPOCH_WINDOW,
            std::mem::replace(&mut self.current, next),
        ));
    }

    pub fn decrypt_opus(&mut self, payload: &[u8]) -> Result<DecryptedPayload, E2eeError> {
        self.receiver_for(payload)?.decrypt_opus(payload)
    }

    pub fn decrypt_h264(&mut self, annex_b: &[u8]) -> Result<DecryptedPayload, E2eeError> {
        self.receiver_for(annex_b)?.decrypt_h264(annex_b)
    }

    fn receiver_for(&mut self, frame: &[u8]) -> Result<&mut MediaReceiver, E2eeError> {
        let Some(metadata) = frame_metadata(frame) else {
            return Ok(&mut self.current);
        };

        if metadata.epoch == self.current.epoch() {
            return Ok(&mut self.current);
        }

        if let Some((until, previous)) = self.previous.as_mut()
            && metadata.epoch == previous.epoch()
            && Instant::now() <= *until
        {
            return Ok(previous);
        }

        Err(E2eeError::EpochMismatch)
    }
}

/// RFC 3711-style replay state for a 128-frame window. `seen` bit zero is `highest`; bit `n`
/// is `highest - n`. It deliberately has no network-order assumption: RTP may be reordered.
#[derive(Default)]
struct ReplayWindow {
    highest: Option<u64>,
    seen: u128,
}

#[derive(Clone, Copy)]
enum ReplayUpdate {
    First { counter: u64 },
    Advance { counter: u64, shift: u64 },
    Within { offset: u32 },
}

impl ReplayWindow {
    fn prepare(&self, counter: u64) -> Result<ReplayUpdate, E2eeError> {
        let Some(highest) = self.highest else {
            return Ok(ReplayUpdate::First { counter });
        };

        if counter > highest {
            return Ok(ReplayUpdate::Advance {
                counter,
                shift: counter - highest,
            });
        }

        let distance = highest - counter;

        if distance >= REPLAY_WINDOW_SIZE {
            return Err(E2eeError::CounterRegression);
        }

        let offset = u32::try_from(distance).map_err(|_| E2eeError::CounterRegression)?;

        if self.seen & (1_u128 << offset) != 0 {
            return Err(E2eeError::CounterRegression);
        }

        Ok(ReplayUpdate::Within { offset })
    }

    fn commit(&mut self, update: ReplayUpdate) {
        match update {
            ReplayUpdate::First { counter } => {
                self.highest = Some(counter);
                self.seen = 1;
            }
            ReplayUpdate::Advance { counter, shift } => {
                self.highest = Some(counter);
                self.seen = if shift >= REPLAY_WINDOW_SIZE {
                    1
                } else {
                    (self.seen << shift) | 1
                };
            }
            ReplayUpdate::Within { offset } => self.seen |= 1_u128 << offset,
        }
    }
}

/// Returns protected-frame metadata only when a complete E2EE footer is present.
pub fn frame_metadata(frame: &[u8]) -> Option<FrameMetadata> {
    split_footer(frame)
        .or_else(|| split_h264_footer(frame))
        .map(|(_, footer)| footer.metadata)
}

#[derive(Clone, Copy)]
enum PayloadKind {
    Opus = 1,
    H264 = 2,
}

struct Footer {
    metadata: FrameMetadata,
    tag: [u8; TAG_LEN],
}

struct Nal<'bytes> {
    header: u8,
    payload: &'bytes [u8],
}

fn with_footer(mut payload: Vec<u8>, metadata: FrameMetadata, tag: [u8; TAG_LEN]) -> Vec<u8> {
    payload.extend(FOOTER_MAGIC);
    payload.extend(metadata.epoch.to_be_bytes());
    payload.extend(metadata.counter.to_be_bytes());
    payload.extend(tag);

    payload
}

fn with_h264_footer(mut payload: Vec<u8>, metadata: FrameMetadata, tag: [u8; TAG_LEN]) -> Vec<u8> {
    let mut data = [0_u8; std::mem::size_of::<u64>() * 2 + TAG_LEN];
    data[..8].copy_from_slice(&metadata.epoch.to_be_bytes());
    data[8..16].copy_from_slice(&metadata.counter.to_be_bytes());
    data[16..].copy_from_slice(&tag);
    payload.extend(H264_FOOTER_MAGIC);
    payload.extend(URL_SAFE_NO_PAD.encode(data).as_bytes());
    payload
}

fn split_footer(frame: &[u8]) -> Option<(&[u8], Footer)> {
    let footer_start = frame.len().checked_sub(FOOTER_LEN)?;
    let footer = frame.get(footer_start..)?;
    let magic = footer.get(..FOOTER_MAGIC.len())?;

    if magic != FOOTER_MAGIC {
        return None;
    }

    let epoch_start = FOOTER_MAGIC.len();
    let counter_start = epoch_start + std::mem::size_of::<u64>();
    let tag_start = counter_start + std::mem::size_of::<u64>();
    let epoch = u64::from_be_bytes(footer.get(epoch_start..counter_start)?.try_into().ok()?);
    let counter = u64::from_be_bytes(footer.get(counter_start..tag_start)?.try_into().ok()?);
    let tag = footer.get(tag_start..)?.try_into().ok()?;

    Some((
        &frame[..footer_start],
        Footer {
            metadata: FrameMetadata { epoch, counter },
            tag,
        },
    ))
}

fn split_h264_footer(frame: &[u8]) -> Option<(&[u8], Footer)> {
    let footer_start = frame.len().checked_sub(H264_FOOTER_LEN)?;
    let footer = frame.get(footer_start..)?;

    if footer.get(..H264_FOOTER_MAGIC.len())? != H264_FOOTER_MAGIC {
        return None;
    }

    let data = URL_SAFE_NO_PAD
        .decode(footer.get(H264_FOOTER_MAGIC.len()..)?)
        .ok()?;
    if data.len() != std::mem::size_of::<u64>() * 2 + TAG_LEN {
        return None;
    }

    let epoch = u64::from_be_bytes(data.get(..8)?.try_into().ok()?);
    let counter = u64::from_be_bytes(data.get(8..16)?.try_into().ok()?);
    let tag = data.get(16..)?.try_into().ok()?;

    Some((
        &frame[..footer_start],
        Footer {
            metadata: FrameMetadata { epoch, counter },
            tag,
        },
    ))
}

fn payload_aad(kind: PayloadKind, origin: Origin, metadata: FrameMetadata) -> Vec<u8> {
    let mut aad = Vec::with_capacity(AAD_DOMAIN.len() + 1 + ORIGIN_PREFIX_LEN + 16);

    aad.extend(AAD_DOMAIN);
    aad.push(kind as u8);
    aad.extend(origin);
    aad.extend(metadata.epoch.to_be_bytes());
    aad.extend(metadata.counter.to_be_bytes());

    aad
}

fn h264_aad(
    origin: Origin,
    metadata: FrameMetadata,
    nals: &[Nal<'_>],
) -> Result<Vec<u8>, E2eeError> {
    let mut aad = payload_aad(PayloadKind::H264, origin, metadata);
    let nal_count = u32::try_from(nals.len()).map_err(|_| E2eeError::FrameTooLarge)?;

    aad.extend(nal_count.to_be_bytes());

    for nal in nals {
        let payload_len = u32::try_from(nal.payload.len()).map_err(|_| E2eeError::FrameTooLarge)?;

        aad.push(nal.header);
        aad.extend(payload_len.to_be_bytes());
    }

    Ok(aad)
}

fn seal(
    key: &[u8; MEDIA_KEY_LEN],
    origin: Origin,
    counter: u64,
    plaintext: &[u8],
    aad: &[u8],
) -> Result<(Vec<u8>, [u8; TAG_LEN]), E2eeError> {
    let cipher =
        ChaCha20Poly1305::new_from_slice(key).map_err(|_| E2eeError::AuthenticationFailed)?;
    let mut ciphertext = plaintext.to_vec();
    let tag = cipher
        .encrypt_in_place_detached(
            Nonce::from_slice(&nonce(origin, counter)),
            aad,
            &mut ciphertext,
        )
        .map_err(|_| E2eeError::AuthenticationFailed)?;

    Ok((ciphertext, tag.into()))
}

fn open(
    key: &[u8; MEDIA_KEY_LEN],
    origin: Origin,
    counter: u64,
    ciphertext: &[u8],
    tag: [u8; TAG_LEN],
    aad: &[u8],
) -> Result<Vec<u8>, E2eeError> {
    let cipher =
        ChaCha20Poly1305::new_from_slice(key).map_err(|_| E2eeError::AuthenticationFailed)?;
    let mut plaintext = ciphertext.to_vec();

    cipher
        .decrypt_in_place_detached(
            Nonce::from_slice(&nonce(origin, counter)),
            aad,
            &mut plaintext,
            Tag::from_slice(&tag),
        )
        .map_err(|_| E2eeError::AuthenticationFailed)?;

    Ok(plaintext)
}

fn nonce(origin: Origin, counter: u64) -> [u8; NONCE_LEN] {
    let mut nonce = [0; NONCE_LEN];

    nonce[..ORIGIN_PREFIX_LEN].copy_from_slice(&origin);
    nonce[ORIGIN_PREFIX_LEN..].copy_from_slice(&counter.to_be_bytes());

    nonce
}

fn parse_clear_annex_b(annex_b: &[u8]) -> Result<Vec<Nal<'_>>, E2eeError> {
    let starts = three_byte_start_codes(annex_b);
    let Some(&first) = starts.first() else {
        return Err(E2eeError::MalformedH264);
    };

    if first > 1 || (first == 1 && annex_b.first() != Some(&0)) {
        return Err(E2eeError::MalformedH264);
    }

    let mut nals = Vec::with_capacity(starts.len());

    for (position, start) in starts.iter().copied().enumerate() {
        let end = starts.get(position + 1).copied().unwrap_or(annex_b.len());
        let body = annex_b
            .get(start + H264_START_CODE.len()..end)
            .ok_or(E2eeError::MalformedH264)?;
        let padding = body.iter().rev().take_while(|byte| **byte == 0).count();
        let body = body
            .get(
                ..body
                    .len()
                    .checked_sub(padding)
                    .ok_or(E2eeError::MalformedH264)?,
            )
            .ok_or(E2eeError::MalformedH264)?;
        let Some((&header, payload)) = body.split_first() else {
            return Err(E2eeError::MalformedH264);
        };

        validate_nal_header(header)?;
        nals.push(Nal { header, payload });
    }

    Ok(nals)
}

fn parse_protected_annex_b(annex_b: &[u8]) -> Result<Vec<Nal<'_>>, E2eeError> {
    let (start_len, starts) = if annex_b.starts_with(&[0, 0, 0, 1]) {
        (4, four_byte_start_codes(annex_b))
    } else if annex_b.starts_with(&H264_START_CODE) {
        (H264_START_CODE.len(), three_byte_start_codes(annex_b))
    } else {
        return Err(E2eeError::MalformedH264);
    };

    if starts.first() != Some(&0) {
        return Err(E2eeError::MalformedH264);
    }

    let mut nals = Vec::with_capacity(starts.len());

    for (position, start) in starts.iter().copied().enumerate() {
        let end = starts.get(position + 1).copied().unwrap_or(annex_b.len());
        let body = annex_b
            .get(start + start_len..end)
            .ok_or(E2eeError::MalformedH264)?;
        let Some((&header, payload)) = body.split_first() else {
            return Err(E2eeError::MalformedH264);
        };

        validate_nal_header(header)?;
        nals.push(Nal { header, payload });
    }

    Ok(nals)
}

fn validate_nal_header(header: u8) -> Result<(), E2eeError> {
    if header & 0x80 != 0 || header & 0x1f == 0 {
        return Err(E2eeError::MalformedH264);
    }

    Ok(())
}

fn three_byte_start_codes(annex_b: &[u8]) -> Vec<usize> {
    annex_b
        .windows(H264_START_CODE.len())
        .enumerate()
        .filter_map(|(index, bytes)| (bytes == H264_START_CODE).then_some(index))
        .collect()
}

fn four_byte_start_codes(annex_b: &[u8]) -> Vec<usize> {
    annex_b
        .windows(4)
        .enumerate()
        .filter_map(|(index, bytes)| (bytes == [0, 0, 0, 1]).then_some(index))
        .collect()
}

fn join_payloads(nals: &[Nal<'_>]) -> Vec<u8> {
    let total = nals.iter().map(|nal| nal.payload.len()).sum();
    let mut payloads = Vec::with_capacity(total);

    for nal in nals {
        payloads.extend(nal.payload);
    }

    payloads
}

fn append_emulation_prevention(output: &mut Vec<u8>, payload: &[u8]) {
    let mut zeroes = 0;

    for &byte in payload {
        if zeroes >= 2 && byte <= 3 {
            output.push(3);
            zeroes = 0;
        }

        output.push(byte);
        zeroes = if byte == 0 { zeroes + 1 } else { 0 };
    }
}

fn remove_emulation_prevention(payload: &[u8]) -> Result<Vec<u8>, E2eeError> {
    let mut output = Vec::with_capacity(payload.len());
    let mut zeroes = 0;

    for (index, &byte) in payload.iter().enumerate() {
        if zeroes >= 2 && byte == 3 && payload.get(index + 1).is_some_and(|next| *next <= 3) {
            continue;
        }

        if zeroes >= 2 && byte == 3 && payload.get(index + 1).is_none() {
            return Err(E2eeError::MalformedH264);
        }

        output.push(byte);
        zeroes = if byte == 0 { zeroes + 1 } else { 0 };
    }

    Ok(output)
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use bytes::Bytes;
    use rtc::rtp::codec::h264::H264Payloader;
    use rtc::rtp::header::Header;
    use rtc::rtp::packet::Packet;
    use rtc::rtp::packetizer::Payloader;
    use rtc::shared::marshal::Marshal;

    use super::*;

    const KEY: [u8; MEDIA_KEY_LEN] = [0x42; MEDIA_KEY_LEN];
    const ORIGIN: Origin = [0x10, 0x20, 0x30, 0x40];
    const EPOCH: Epoch = 7;
    const MTU: usize = 1_200;

    fn h264_frame() -> Vec<u8> {
        let mut frame = Vec::from([
            0, 0, 1, 0x67, 0x64, 0, 0x1f, 0, 0, 1, 0x68, 0xee, 0x3c, 0x80, 0, 0, 1, 0x65,
        ]);

        frame.extend((0..5_000).map(|index| (index % 251) as u8 + 1));

        frame
    }

    fn rtp_packets(annex_b: &[u8]) -> Vec<Vec<u8>> {
        let payloads = H264Payloader::default()
            .payload(MTU, &Bytes::copy_from_slice(annex_b))
            .expect("H.264 payloads");
        let last = payloads.len() - 1;

        payloads
            .into_iter()
            .enumerate()
            .map(|(index, payload)| {
                Packet {
                    header: Header {
                        version: 2,
                        marker: index == last,
                        payload_type: 96,
                        sequence_number: index as u16,
                        timestamp: 90_000,
                        ssrc: 5,
                        ..Header::default()
                    },
                    payload,
                }
                .marshal()
                .expect("RTP packet")
                .to_vec()
            })
            .collect()
    }

    #[test]
    fn opus_round_trip_and_legacy_payloads_are_distinguished() {
        let opus = vec![0x5a; 160];
        let mut sender = MediaSender::new(KEY, EPOCH, ORIGIN);
        let encrypted = sender.encrypt_opus(&opus).expect("encrypt Opus");
        let mut receiver = MediaReceiver::new(KEY, EPOCH, ORIGIN, Policy::AllowClear);

        assert_eq!(
            frame_metadata(&encrypted),
            Some(FrameMetadata {
                epoch: EPOCH,
                counter: 0
            })
        );
        assert_eq!(
            receiver.decrypt_opus(&encrypted).expect("decrypt Opus"),
            DecryptedPayload::Decrypted(opus.clone())
        );
        assert_eq!(
            receiver.decrypt_opus(&opus).expect("legacy Opus"),
            DecryptedPayload::Clear(opus)
        );
    }

    #[test]
    fn h264_round_trips_after_fu_a_fragmentation_with_sps_pps_and_idr_visible() {
        let clear = h264_frame();
        let mut sender = MediaSender::new(KEY, EPOCH, ORIGIN);
        let encrypted = sender.encrypt_h264(&clear).expect("encrypt H.264");
        let footer = &encrypted[encrypted.len() - H264_FOOTER_LEN..];
        assert!(!footer.windows(3).any(|bytes| bytes == H264_START_CODE));
        let packets = rtp_packets(&encrypted);

        assert!(packets.len() > 3, "the protected IDR must become FU-A");
        assert!(
            packets.iter().any(|packet| packet[12] & 0x1f == 28),
            "no FU-A packet was made"
        );

        let mut unpacker = crate::VideoUnpacker::default();
        let assembled = packets
            .iter()
            .filter_map(|packet| unpacker.push(packet))
            .next()
            .expect("complete access unit");
        let mut receiver = MediaReceiver::new(KEY, EPOCH, ORIGIN, Policy::Require);

        assert!(
            assembled.keyframe,
            "the visible SPS/IDR headers still mark a keyframe"
        );
        assert_eq!(
            receiver
                .decrypt_h264(&assembled.data)
                .expect("decrypt H.264"),
            DecryptedPayload::Decrypted(clear)
        );
    }

    #[test]
    fn h264_with_an_access_unit_delimiter_survives_the_packetizer_dropping_it() {
        // O encoder da NVIDIA abre cada quadro com um AUD, que o empacotador RTP não manda.
        let mut clear = Vec::from([0, 0, 0, 1, 0x09, 0xf0]);
        clear.extend(h264_frame());
        let mut sender = MediaSender::new(KEY, EPOCH, ORIGIN);
        let encrypted = sender.encrypt_h264(&clear).expect("encrypt H.264");
        let mut unpacker = crate::VideoUnpacker::default();
        let assembled = rtp_packets(&encrypted)
            .iter()
            .filter_map(|packet| unpacker.push(packet))
            .next()
            .expect("complete access unit");

        assert_eq!(
            MediaReceiver::new(KEY, EPOCH, ORIGIN, Policy::Require).decrypt_h264(&assembled.data),
            Ok(DecryptedPayload::Decrypted(h264_frame()))
        );
    }

    #[test]
    fn changed_clear_header_fails_authentication_without_releasing_a_partial_frame() {
        let clear = h264_frame();
        let mut sender = MediaSender::new(KEY, EPOCH, ORIGIN);
        let encrypted = sender.encrypt_h264(&clear).expect("encrypt H.264");
        let mut changed = encrypted.clone();

        changed[3] ^= 1;

        let mut receiver = MediaReceiver::new(KEY, EPOCH, ORIGIN, Policy::Require);

        assert_eq!(
            receiver.decrypt_h264(&changed),
            Err(E2eeError::AuthenticationFailed)
        );
        assert_eq!(
            receiver
                .decrypt_h264(&encrypted)
                .expect("original is still acceptable"),
            DecryptedPayload::Decrypted(clear)
        );
    }

    #[test]
    fn changed_tag_and_wrong_key_or_epoch_fail() {
        let opus = vec![0x9a; 160];
        let mut sender = MediaSender::new(KEY, EPOCH, ORIGIN);
        let encrypted = sender.encrypt_opus(&opus).expect("encrypt Opus");
        let mut changed_tag = encrypted.clone();

        *changed_tag.last_mut().expect("tag byte") ^= 1;

        assert_eq!(
            MediaReceiver::new(KEY, EPOCH, ORIGIN, Policy::Require).decrypt_opus(&changed_tag),
            Err(E2eeError::AuthenticationFailed)
        );
        assert_eq!(
            MediaReceiver::new([0x24; MEDIA_KEY_LEN], EPOCH, ORIGIN, Policy::Require)
                .decrypt_opus(&encrypted),
            Err(E2eeError::AuthenticationFailed)
        );
        assert_eq!(
            MediaReceiver::new(KEY, EPOCH + 1, ORIGIN, Policy::Require).decrypt_opus(&encrypted),
            Err(E2eeError::EpochMismatch)
        );
    }

    #[test]
    fn replay_window_allows_reordering_but_rejects_duplicates_and_old_frames() {
        let mut sender = MediaSender::new(KEY, EPOCH, ORIGIN);
        let mut frames = Vec::new();

        for counter in 0..260_u64 {
            frames.push(sender.encrypt_opus(&counter.to_be_bytes()).expect("frame"));
        }

        assert_eq!(
            frame_metadata(&frames[0]),
            Some(FrameMetadata {
                epoch: EPOCH,
                counter: 0
            })
        );
        assert_eq!(
            frame_metadata(&frames[129]),
            Some(FrameMetadata {
                epoch: EPOCH,
                counter: 129
            })
        );

        let mut receiver = MediaReceiver::new(KEY, EPOCH, ORIGIN, Policy::Require);

        // Counter 128 arrives first; 127 is still inside the 128-frame replay window.
        assert_eq!(
            receiver
                .decrypt_opus(&frames[128])
                .expect("first arrival")
                .into_bytes(),
            128_u64.to_be_bytes()
        );
        assert_eq!(
            receiver
                .decrypt_opus(&frames[127])
                .expect("reordered arrival")
                .into_bytes(),
            127_u64.to_be_bytes()
        );
        assert_eq!(
            receiver.decrypt_opus(&frames[127]),
            Err(E2eeError::CounterRegression)
        );

        // A jump much larger than the window resets the bitmap to the new high-water mark.
        assert!(receiver.decrypt_opus(&frames[259]).is_ok());
        assert_eq!(
            receiver.decrypt_opus(&frames[0]),
            Err(E2eeError::CounterRegression)
        );
    }

    #[test]
    fn require_policy_rejects_clear_frames_and_allow_clear_preserves_legacy_mode() {
        let clear = vec![0x5a; 160];

        assert_eq!(
            MediaReceiver::new(KEY, EPOCH, ORIGIN, Policy::Require).decrypt_opus(&clear),
            Err(E2eeError::Unencrypted)
        );
        assert_eq!(
            MediaReceiver::new(KEY, EPOCH, ORIGIN, Policy::AllowClear).decrypt_opus(&clear),
            Ok(DecryptedPayload::Clear(clear))
        );
    }

    #[test]
    fn emulation_prevention_keeps_ciphertext_start_codes_out_of_annex_b() {
        let metadata = FrameMetadata {
            epoch: EPOCH,
            counter: 0,
        };
        let zero_payload = [0; 3];
        let template = [Nal {
            header: 0x65,
            payload: &zero_payload,
        }];
        let aad = h264_aad(ORIGIN, metadata, &template).expect("AAD");
        let keystream = seal(&KEY, ORIGIN, metadata.counter, &zero_payload, &aad)
            .expect("known ciphertext")
            .0;
        let wanted = [0, 0, 1];
        let payload = [
            keystream[0] ^ wanted[0],
            keystream[1] ^ wanted[1],
            keystream[2] ^ wanted[2],
        ];
        let clear = [[0, 0, 1, 0x65].as_slice(), &payload].concat();
        let mut sender = MediaSender::new(KEY, EPOCH, ORIGIN);
        let encrypted = sender.encrypt_h264(&clear).expect("encrypt H.264");
        let protected = &encrypted[..encrypted.len() - H264_FOOTER_LEN];
        let nals = parse_protected_annex_b(protected).expect("separable NAL");
        let raw_ciphertext =
            remove_emulation_prevention(nals[0].payload).expect("unescape ciphertext");
        let mut receiver = MediaReceiver::new(KEY, EPOCH, ORIGIN, Policy::Require);

        assert_eq!(
            raw_ciphertext, wanted,
            "the test must exercise a ciphertext start code"
        );
        assert!(
            !protected
                .windows(3)
                .skip(1)
                .any(|bytes| bytes == H264_START_CODE),
            "ciphertext forged another Annex-B separator"
        );
        assert_eq!(
            receiver.decrypt_h264(&encrypted).expect("decrypt H.264"),
            DecryptedPayload::Decrypted(clear)
        );
    }

    #[test]
    #[ignore = "manual cost probe; run with --ignored --nocapture in release mode"]
    fn benchmark_frame_encryption_cost() {
        let h264 = h264_frame()
            .into_iter()
            .cycle()
            .take(100_000)
            .collect::<Vec<_>>();
        let opus = vec![0x55; 160];
        let video_frames = 1_000;
        let audio_frames = 10_000;
        let mut video_sender = MediaSender::new(KEY, EPOCH, ORIGIN);
        let video_start = Instant::now();

        for _ in 0..video_frames {
            std::hint::black_box(
                video_sender
                    .encrypt_h264(std::hint::black_box(&h264))
                    .expect("encrypt 1080p frame"),
            );
        }

        let mut audio_sender = MediaSender::new(KEY, EPOCH, [0x10, 0x20, 0x30, 0x41]);
        let audio_start = Instant::now();

        for _ in 0..audio_frames {
            std::hint::black_box(
                audio_sender
                    .encrypt_opus(std::hint::black_box(&opus))
                    .expect("encrypt Opus frame"),
            );
        }

        let video_per_frame = video_start.elapsed() / video_frames;
        let audio_per_frame = audio_start.elapsed() / audio_frames;

        eprintln!(
            "E2EE ChaCha20-Poly1305: 100 KB H.264 {video_per_frame:?}/frame; 160 B Opus {audio_per_frame:?}/frame"
        );
    }
}
