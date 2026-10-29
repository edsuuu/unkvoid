//! MLS group state for one encrypted call.
//!
//! This deliberately has no socket, SFU, Laravel, or RTP integration. The delivery service maps
//! the opaque [`E2eeSignal`] to `e2eeJoin` (`key_package`), `e2eeCommit`, or `e2eeWelcome`:
//! `{ version: 1, group: bytes, epoch: u64, kind: key_package|commit|welcome, sender: account_id,
//! payload: bytes }`. `payload` is the TLS serialization defined by MLS (a raw `KeyPackage` for
//! `key_package`, an `MlsMessageOut` for `commit` and `welcome`). The delivery service must bind
//! `sender` to the authenticated account, cap the payload size, and may reorder/duplicate it;
//! this module treats stale and future commits as non-fatal so the caller can sync and retry.
//!
//! `E2eeSignal::epoch` is unauthenticated delivery metadata supplied by the relay. It is useful
//! only for the relay's one-commit-per-epoch ordering and for deciding whether to retry a
//! message. The authoritative epoch, membership and transcript are authenticated by MLS after
//! [`E2eeGroup::process_commit`] or [`E2eeGroup::join_welcome`] succeeds.

use std::collections::BTreeMap;

use openmls::prelude::tls_codec::{
    Deserialize as TlsDeserializeTrait, Serialize as TlsSerializeTrait,
};
use openmls::prelude::*;
use openmls_basic_credential::SignatureKeyPair;
use openmls_rust_crypto::OpenMlsRustCrypto;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use media::{Epoch, MEDIA_KEY_LEN, MediaReceiver, MediaSender, Origin, Policy};

/// The only MLS ciphersuite accepted by the first encrypted-call implementation.
pub const MLS_CIPHERSUITE: Ciphersuite =
    Ciphersuite::MLS_128_DHKEMX25519_CHACHA20POLY1305_SHA256_Ed25519;

/// Domain separation for sender/origin media keys exported from MLS.
pub const MEDIA_EXPORTER_LABEL: &str = "unkvoid-media-sender-key-v1";
const SIGNAL_VERSION: u8 = 1;
const PRIVACY_CODE_DOMAIN: &[u8] = b"unkvoid-privacy-code-v1";
const PRIVACY_CODE_MODULUS: u128 = 1_000_000_000_000_000_000_000_000_000_000;
/// Every media origin whose previous sender key can be needed by an in-flight packet after a
/// Commit. The cache is overwritten at the next epoch transition; it never contains a future
/// epoch key and never leaves this process.
const MEDIA_ORIGINS: [Origin; 4] = [*b"scrn", *b"scra", *b"camr", *b"micr"];

/// The opaque MLS payload type that the later signaling layer will relay.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalKind {
    KeyPackage,
    Commit,
    Welcome,
}

/// Versioned envelope around an MLS payload. It intentionally contains no media key or plaintext.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct E2eeSignal {
    pub version: u8,
    pub group_id: Vec<u8>,
    /// Unauthenticated relay metadata: the epoch in which the handshake message was created.
    /// A commit advances peers to +1. MLS validates the real epoch from the signed payload.
    pub epoch: Epoch,
    pub kind: SignalKind,
    pub sender_account_id: String,
    pub payload: Vec<u8>,
}

/// A unique session member, represented by its MLS signature public key.
///
/// Several devices may legitimately have the same account id. The member id, rather than the
/// account id, is included in the media exporter context so their per-origin counters never
/// share an AEAD key and nonce space.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MemberId(Vec<u8>);

impl MemberId {
    /// The MLS signature public-key bytes, for display-independent matching.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// One active MLS leaf. Accounts may occur more than once, once for each signed-in device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupMember {
    pub account_id: String,
    pub member_id: MemberId,
    pub leaf_index: u32,
}

/// Result of delivering a commit from an untrusted, unordered delivery service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommitDisposition {
    Applied,
    /// A duplicate or delayed commit for an already-merged epoch.
    IgnoredStale,
    /// A later commit arrived before the one needed to reach its epoch. It is not buffered here;
    /// the signaling phase must request/supply the missing commit and retry this payload.
    DeferredFuture,
    /// The author must receive the relay's accepted/rejected result before merging its local
    /// pending commit. A winning commit must be retried after [`E2eeGroup::discard_pending`].
    DeferredPendingLocal,
    /// The MLS parser or state machine rejected a current-epoch payload without changing state.
    Rejected {
        reason: CommitRejectReason,
    },
}

/// A stable, non-secret reason why a commit was rejected. The delivery integration can stop
/// media and request resynchronization without parsing OpenMLS implementation errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitRejectReason {
    MalformedPayload,
    NotACommit,
    MlsValidation,
}

/// Errors that callers can surface without exposing MLS implementation internals.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum E2eeGroupError {
    #[error("the account id must not be empty")]
    EmptyAccountId,

    #[error("the MLS group id must not be empty")]
    EmptyGroupId,

    #[error("the MLS group has not been created or joined")]
    GroupNotReady,

    #[error("the MLS group is inactive because this member was removed")]
    GroupInactive,

    #[error("this client already has an MLS group")]
    GroupAlreadyInitialized,

    #[error("the signal uses an unsupported E2EE version")]
    UnsupportedSignalVersion,

    #[error("the signal belongs to a different MLS group")]
    WrongGroup,

    #[error("the signal kind is not valid for this operation")]
    UnexpectedSignalKind,

    #[error("the key package credential does not match the authenticated sender")]
    SenderCredentialMismatch,

    #[error("the account is not a member of the current MLS epoch")]
    UnknownMember,

    #[error("there is no locally staged MLS commit to confirm or discard")]
    NoPendingCommit,

    #[error("the requested epoch is unavailable to this member")]
    EpochUnavailable,

    #[error("too many MLS commits arrived before the Welcome")]
    TooManyPendingCommits,

    #[error("MLS rejected the operation: {0}")]
    Mls(String),
}

/// An in-memory MLS client identity and, once created or joined, a call group.
///
/// A production persistence layer will serialize its provider storage and signer securely; phase
/// 2 keeps it in memory so no private material enters the API, SFU, or Laravel.
pub struct E2eeGroup {
    account_id: String,
    provider: OpenMlsRustCrypto,
    signer: SignatureKeyPair,
    credential_with_key: CredentialWithKey,
    key_package: KeyPackage,
    group: Option<MlsGroup>,
    /// A Welcome is released only after the relay orders and accepts its matching Commit.
    pending_welcome: Option<E2eeSignal>,
    /// Exporter output for exactly the epoch immediately before the active one. A receiver
    /// already decoding an RTP frame may need it while the ordered Commit reaches the app.
    previous_media_keys: BTreeMap<(Epoch, MemberId, Origin), [u8; MEDIA_KEY_LEN]>,
}

impl E2eeGroup {
    /// Makes a fresh per-session Ed25519 identity and a single-use MLS KeyPackage.
    pub fn new(account_id: impl Into<String>) -> Result<Self, E2eeGroupError> {
        let account_id = account_id.into();

        if account_id.is_empty() {
            return Err(E2eeGroupError::EmptyAccountId);
        }

        let provider = OpenMlsRustCrypto::default();
        let signer =
            SignatureKeyPair::new(MLS_CIPHERSUITE.signature_algorithm()).map_err(mls_error)?;
        signer.store(provider.storage()).map_err(mls_error)?;

        let credential_with_key = CredentialWithKey {
            credential: BasicCredential::new(account_id.as_bytes().to_vec()).into(),
            signature_key: signer.to_public_vec().into(),
        };
        let key_package = KeyPackage::builder()
            .build(
                MLS_CIPHERSUITE,
                &provider,
                &signer,
                credential_with_key.clone(),
            )
            .map_err(mls_error)?
            .into_key_package();

        Ok(Self {
            account_id,
            provider,
            signer,
            credential_with_key,
            key_package,
            group: None,
            pending_welcome: None,
            previous_media_keys: BTreeMap::new(),
        })
    }

    /// Creates this client's one-member group. `group_id` must be unique for each call session.
    pub fn create_group(&mut self, group_id: impl AsRef<[u8]>) -> Result<(), E2eeGroupError> {
        let group_id = group_id.as_ref();

        if group_id.is_empty() {
            return Err(E2eeGroupError::EmptyGroupId);
        }

        if self.group.is_some() {
            return Err(E2eeGroupError::GroupAlreadyInitialized);
        }

        let config = MlsGroupCreateConfig::builder()
            .ciphersuite(MLS_CIPHERSUITE)
            // A welcome carries enough public state for an in-memory peer to join without a
            // second side channel for the ratchet tree.
            .use_ratchet_tree_extension(true)
            .build();
        let group = MlsGroup::new_with_group_id(
            &self.provider,
            &self.signer,
            &config,
            GroupId::from_slice(group_id),
            self.credential_with_key.clone(),
        )
        .map_err(mls_error)?;

        self.group = Some(group);
        Ok(())
    }

    /// Serializes this session's single-use KeyPackage for the future opaque signaling channel.
    pub fn key_package_signal(
        &self,
        group_id: impl AsRef<[u8]>,
    ) -> Result<E2eeSignal, E2eeGroupError> {
        let group_id = group_id.as_ref();

        if group_id.is_empty() {
            return Err(E2eeGroupError::EmptyGroupId);
        }

        Ok(E2eeSignal {
            version: SIGNAL_VERSION,
            group_id: group_id.to_vec(),
            epoch: self
                .group
                .as_ref()
                .map_or(0, |group| group.epoch().as_u64()),
            kind: SignalKind::KeyPackage,
            sender_account_id: self.account_id.clone(),
            payload: self
                .key_package
                .tls_serialize_detached()
                .map_err(mls_error)?,
        })
    }

    /// Stages the authenticated sender of `key_package` in a Commit for the current epoch.
    ///
    /// The returned Commit must first be accepted by the room relay. Until
    /// [`Self::confirm_pending`] is called this member remains at the old epoch and the Welcome
    /// is deliberately retained locally. This makes a concurrently rejected Commit reversible
    /// instead of letting two authors silently merge different epoch states.
    pub fn add_member(&mut self, key_package: &E2eeSignal) -> Result<E2eeSignal, E2eeGroupError> {
        self.validate_signal(key_package, SignalKind::KeyPackage)?;
        let parsed =
            KeyPackageIn::tls_deserialize_exact(&key_package.payload).map_err(mls_error)?;
        let package = parsed
            .validate(self.provider.crypto(), ProtocolVersion::Mls10)
            .map_err(mls_error)?;
        let credential = CredentialWithKey::from(package.leaf_node());

        if credential_identity(&credential.credential)? != key_package.sender_account_id {
            return Err(E2eeGroupError::SenderCredentialMismatch);
        }

        let (group_id, epoch, commit, welcome) = {
            let provider = &self.provider;
            let signer = &self.signer;
            let group = self.group.as_mut().ok_or(E2eeGroupError::GroupNotReady)?;

            if !group.is_active() {
                return Err(E2eeGroupError::GroupInactive);
            }

            let group_id = group.group_id().to_vec();
            let epoch = group.epoch().as_u64();
            let (commit, welcome, _) = group
                .add_members(provider, signer, &[package])
                .map_err(mls_error)?;

            (group_id, epoch, commit, welcome)
        };

        let commit = signal_from_message(
            group_id.clone(),
            epoch,
            SignalKind::Commit,
            &self.account_id,
            commit,
        )?;
        self.pending_welcome = Some(signal_from_message(
            group_id,
            epoch,
            SignalKind::Welcome,
            &self.account_id,
            welcome,
        )?);

        Ok(commit)
    }

    /// Joins from a Welcome received through the opaque signaling channel.
    pub fn join_welcome(&mut self, welcome: &E2eeSignal) -> Result<(), E2eeGroupError> {
        if self.group.is_some() {
            return Err(E2eeGroupError::GroupAlreadyInitialized);
        }

        if welcome.version != SIGNAL_VERSION {
            return Err(E2eeGroupError::UnsupportedSignalVersion);
        }

        if welcome.kind != SignalKind::Welcome {
            return Err(E2eeGroupError::UnexpectedSignalKind);
        }

        let message = MlsMessageIn::tls_deserialize_exact(&welcome.payload).map_err(mls_error)?;
        let MlsMessageBodyIn::Welcome(welcome_message) = message.extract() else {
            return Err(E2eeGroupError::UnexpectedSignalKind);
        };
        let config = MlsGroupJoinConfig::builder()
            // Who joined by Welcome commits too once the creator leaves: without the extension
            // its Welcomes went out with no ratchet tree and the next peer could not join.
            .use_ratchet_tree_extension(true)
            .build();
        let processed =
            ProcessedWelcome::new_from_welcome(&self.provider, &config, welcome_message)
                .map_err(mls_error)?;
        let staged = processed
            .into_staged_welcome(&self.provider, None)
            .map_err(mls_error)?;

        if staged.group_context().group_id().as_slice() != welcome.group_id {
            return Err(E2eeGroupError::WrongGroup);
        }

        let group = staged.into_group(&self.provider).map_err(mls_error)?;

        if group.epoch().as_u64() != welcome.epoch.saturating_add(1) {
            return Err(E2eeGroupError::Mls(
                "welcome epoch did not advance exactly once".to_owned(),
            ));
        }

        self.group = Some(group);
        Ok(())
    }

    /// Stages removal of every active device for `account_id` in one Commit.
    ///
    /// Like [`Self::add_member`], this does not advance the local epoch until the room relay
    /// accepts the Commit and the caller invokes [`Self::confirm_pending`].
    pub fn remove_member(&mut self, account_id: &str) -> Result<E2eeSignal, E2eeGroupError> {
        let (group_id, epoch, commit) = {
            let provider = &self.provider;
            let signer = &self.signer;
            let group = self.group.as_mut().ok_or(E2eeGroupError::GroupNotReady)?;

            if !group.is_active() {
                return Err(E2eeGroupError::GroupInactive);
            }

            let members = group
                .members()
                .filter(|member| {
                    credential_identity(&member.credential)
                        .is_ok_and(|identity| identity == account_id)
                })
                .map(|member| member.index)
                .collect::<Vec<_>>();

            if members.is_empty() {
                return Err(E2eeGroupError::UnknownMember);
            }

            let group_id = group.group_id().to_vec();
            let epoch = group.epoch().as_u64();
            let (commit, _, _) = group
                .remove_members(provider, signer, &members)
                .map_err(mls_error)?;

            (group_id, epoch, commit)
        };

        signal_from_message(
            group_id,
            epoch,
            SignalKind::Commit,
            &self.account_id,
            commit,
        )
    }

    /// Stages removal of the given MLS session leaves in one Commit. This is the path used for a
    /// `peerLeft`: accounts can have more than one active device, and losing one peer must not
    /// remove the others.
    pub fn remove_member_ids(
        &mut self,
        member_ids: &[MemberId],
    ) -> Result<E2eeSignal, E2eeGroupError> {
        let (group_id, epoch, commit) = {
            let provider = &self.provider;
            let signer = &self.signer;
            let group = self.group.as_mut().ok_or(E2eeGroupError::GroupNotReady)?;

            if !group.is_active() {
                return Err(E2eeGroupError::GroupInactive);
            }

            let members = group
                .members()
                .filter(|member| member_ids.iter().any(|id| id.0 == member.signature_key))
                .map(|member| member.index)
                .collect::<Vec<_>>();

            if members.is_empty() {
                return Err(E2eeGroupError::UnknownMember);
            }

            let group_id = group.group_id().to_vec();
            let epoch = group.epoch().as_u64();
            let (commit, _, _) = group
                .remove_members(provider, signer, &members)
                .map_err(mls_error)?;

            (group_id, epoch, commit)
        };

        signal_from_message(
            group_id,
            epoch,
            SignalKind::Commit,
            &self.account_id,
            commit,
        )
    }

    /// Merges the locally staged Commit after the room relay accepted it as the sole Commit for
    /// its epoch. If this was an add, returns the Welcome that may now be delivered to the new
    /// member; callers must never forward that Welcome before this succeeds.
    pub fn confirm_pending(&mut self) -> Result<Option<E2eeSignal>, E2eeGroupError> {
        self.retain_current_epoch_media_keys()?;
        let provider = &self.provider;
        let group = self.group.as_mut().ok_or(E2eeGroupError::GroupNotReady)?;

        if group.pending_commit().is_none() {
            return Err(E2eeGroupError::NoPendingCommit);
        }

        group.merge_pending_commit(provider).map_err(mls_error)?;
        Ok(self.pending_welcome.take())
    }

    /// Clears a locally staged Commit after the room relay rejected it. The caller can then
    /// process the accepted competing Commit and, if still necessary, stage a new change in the
    /// resulting epoch.
    pub fn discard_pending(&mut self) -> Result<(), E2eeGroupError> {
        let provider = &self.provider;
        let group = self.group.as_mut().ok_or(E2eeGroupError::GroupNotReady)?;

        if group.pending_commit().is_none() {
            return Err(E2eeGroupError::NoPendingCommit);
        }

        group
            .clear_pending_commit(provider.storage())
            .map_err(mls_error)?;
        self.pending_welcome = None;
        Ok(())
    }

    /// Processes a commit without trusting the delivery service's ordering or deduplication.
    pub fn process_commit(
        &mut self,
        commit: &E2eeSignal,
    ) -> Result<CommitDisposition, E2eeGroupError> {
        self.validate_signal(commit, SignalKind::Commit)?;
        let current_epoch = self.epoch()?;

        if commit.epoch < current_epoch {
            return Ok(CommitDisposition::IgnoredStale);
        }

        if commit.epoch > current_epoch {
            return Ok(CommitDisposition::DeferredFuture);
        }

        // Save the old sender keys before MLS advances. This is deliberately done before
        // parsing the opaque Commit: retaining a current key after a malformed packet is safe,
        // and avoids a borrow of the group while it is staged below.
        self.retain_current_epoch_media_keys()?;

        let message = match MlsMessageIn::tls_deserialize_exact(&commit.payload) {
            Ok(message) => message,
            Err(error) => {
                tracing::warn!(?error, "MLS commit rejected: malformed payload");
                return Ok(CommitDisposition::Rejected {
                    reason: CommitRejectReason::MalformedPayload,
                });
            }
        };
        let protocol = match message.try_into_protocol_message() {
            Ok(protocol) => protocol,
            Err(error) => {
                tracing::warn!(
                    ?error,
                    "MLS commit rejected: payload is not a protocol commit"
                );
                return Ok(CommitDisposition::Rejected {
                    reason: CommitRejectReason::NotACommit,
                });
            }
        };
        let provider = &self.provider;
        let group = self.group.as_mut().ok_or(E2eeGroupError::GroupNotReady)?;

        if !group.is_active() {
            return Err(E2eeGroupError::GroupInactive);
        }

        if group.pending_commit().is_some() {
            return Ok(CommitDisposition::DeferredPendingLocal);
        }

        let processed = match group.process_message(provider, protocol) {
            Ok(processed) => processed,
            Err(error) => {
                tracing::warn!(?error, "MLS commit rejected by validation");
                return Ok(CommitDisposition::Rejected {
                    reason: CommitRejectReason::MlsValidation,
                });
            }
        };

        let ProcessedMessageContent::StagedCommitMessage(staged) = processed.into_content() else {
            tracing::warn!("MLS commit rejected: unexpected processed content");
            return Ok(CommitDisposition::Rejected {
                reason: CommitRejectReason::NotACommit,
            });
        };

        group
            .merge_staged_commit(provider, *staged)
            .map_err(mls_error)?;

        Ok(CommitDisposition::Applied)
    }

    /// The current MLS epoch, which is also the media-key epoch.
    pub fn epoch(&self) -> Result<Epoch, E2eeGroupError> {
        Ok(self.active_group()?.epoch().as_u64())
    }

    /// Current MLS leaves, sorted by account then member id. Account ids are not unique: each
    /// signed-in device has its own [`MemberId`] and therefore its own media-key nonce space.
    pub fn members(&self) -> Result<Vec<GroupMember>, E2eeGroupError> {
        let mut members = self
            .active_group()?
            .members()
            .map(|member| {
                Ok(GroupMember {
                    account_id: credential_identity(&member.credential)?,
                    member_id: MemberId(member.signature_key),
                    leaf_index: member.index.u32(),
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        members.sort_by(|left, right| {
            left.account_id
                .cmp(&right.account_id)
                .then(left.member_id.cmp(&right.member_id))
        });
        Ok(members)
    }

    /// This session's unique MLS member id, for sender keys and media headers in phase 3.
    pub fn member_id(&self) -> MemberId {
        MemberId(self.signer.to_public_vec())
    }

    /// The deterministic author for membership changes in the current epoch: the active member
    /// with the smallest MLS leaf index. If it leaves, the next smallest active leaf takes over.
    /// The relay still serializes every Commit, because malformed/old clients can violate this
    /// convenience rule and the relay cannot authenticate opaque MLS contents.
    pub fn designated_committer(&self) -> Result<GroupMember, E2eeGroupError> {
        self.members()?
            .into_iter()
            .min_by_key(|member| member.leaf_index)
            .ok_or(E2eeGroupError::UnknownMember)
    }

    /// Derives a 256-bit sender/origin key from the MLS exporter of the current epoch.
    pub fn media_key(
        &self,
        participant: &MemberId,
        origin: Origin,
    ) -> Result<[u8; MEDIA_KEY_LEN], E2eeGroupError> {
        self.media_key_for_epoch(self.epoch()?, participant, origin)
    }

    /// Same derivation with an explicit epoch check. Past epochs are intentionally unavailable.
    pub fn media_key_for_epoch(
        &self,
        epoch: Epoch,
        participant: &MemberId,
        origin: Origin,
    ) -> Result<[u8; MEDIA_KEY_LEN], E2eeGroupError> {
        if let Some(key) = self
            .previous_media_keys
            .get(&(epoch, participant.clone(), origin))
        {
            return Ok(*key);
        }

        let group = self.active_group()?;

        if epoch != group.epoch().as_u64() {
            return Err(E2eeGroupError::EpochUnavailable);
        }

        if !group
            .members()
            .any(|member| member.signature_key == participant.0)
        {
            return Err(E2eeGroupError::UnknownMember);
        }

        let context =
            media_exporter_context(group.group_id().as_slice(), epoch, participant, origin)?;
        let bytes = group
            .export_secret(
                self.provider.crypto(),
                MEDIA_EXPORTER_LABEL,
                &context,
                MEDIA_KEY_LEN,
            )
            .map_err(mls_error)?;

        bytes.try_into().map_err(|_| {
            E2eeGroupError::Mls(
                "MLS exporter returned a media key with the wrong length".to_owned(),
            )
        })
    }

    /// Creates a phase-3 sender for this session's own participant key. It does not install it in
    /// the live media path yet.
    pub fn media_sender(&self, origin: Origin) -> Result<MediaSender, E2eeGroupError> {
        let epoch = self.epoch()?;
        Ok(MediaSender::new(
            self.media_key_for_epoch(epoch, &self.member_id(), origin)?,
            epoch,
            origin,
        ))
    }

    /// Creates a phase-3 receiver for another participant's current sender key, with an explicit
    /// downgrade policy selected by the room integration in a later phase.
    pub fn media_receiver(
        &self,
        participant: &MemberId,
        origin: Origin,
        policy: Policy,
    ) -> Result<MediaReceiver, E2eeGroupError> {
        let epoch = self.epoch()?;
        Ok(MediaReceiver::new(
            self.media_key_for_epoch(epoch, participant, origin)?,
            epoch,
            origin,
            policy,
        ))
    }

    /// Builds a receiver for the active or immediately preceding MLS epoch. The caller applies
    /// the time limit for accepting the preceding epoch at the media boundary.
    pub fn media_receiver_for_epoch(
        &self,
        epoch: Epoch,
        participant: &MemberId,
        origin: Origin,
        policy: Policy,
    ) -> Result<MediaReceiver, E2eeGroupError> {
        Ok(MediaReceiver::new(
            self.media_key_for_epoch(epoch, participant, origin)?,
            epoch,
            origin,
            policy,
        ))
    }

    /// Validates a relayed KeyPackage and returns the sender's MLS session identity. It is used
    /// only to bind the authenticated SFU `peerId` to a leaf; the commit still authenticates the
    /// membership change.
    pub fn member_id_from_key_package(
        &self,
        key_package: &E2eeSignal,
    ) -> Result<MemberId, E2eeGroupError> {
        if key_package.version != SIGNAL_VERSION {
            return Err(E2eeGroupError::UnsupportedSignalVersion);
        }

        if key_package.kind != SignalKind::KeyPackage {
            return Err(E2eeGroupError::UnexpectedSignalKind);
        }

        let parsed =
            KeyPackageIn::tls_deserialize_exact(&key_package.payload).map_err(mls_error)?;
        let package = parsed
            .validate(self.provider.crypto(), ProtocolVersion::Mls10)
            .map_err(mls_error)?;

        Ok(MemberId(
            package.leaf_node().signature_key().as_slice().to_vec(),
        ))
    }

    /// Stable, human-comparable digits over the group id, epoch, and every current Basic
    /// Credential/signature public key. It is deliberately not a secret and changes on membership
    /// changes, allowing humans to detect inconsistent authenticated transcripts.
    pub fn privacy_code(&self) -> Result<String, E2eeGroupError> {
        let group = self.active_group()?;
        let mut members = group
            .members()
            .map(|member| {
                Ok((
                    credential_identity(&member.credential)?,
                    member.signature_key,
                ))
            })
            .collect::<Result<Vec<(String, Vec<u8>)>, E2eeGroupError>>()?;
        members.sort();

        let mut hash = Sha256::new();
        hash.update(PRIVACY_CODE_DOMAIN);
        append_len_prefixed(&mut hash, group.group_id().as_slice())?;
        hash.update(group.epoch().as_u64().to_be_bytes());

        for (identity, signature_key) in members {
            append_len_prefixed(&mut hash, identity.as_bytes())?;
            append_len_prefixed(&mut hash, &signature_key)?;
        }

        let digest = hash.finalize();
        let value = u128::from_be_bytes(
            digest[..16]
                .try_into()
                .expect("SHA-256 prefix is sixteen bytes"),
        ) % PRIVACY_CODE_MODULUS;
        let digits = format!("{value:030}");

        Ok(format!(
            "{} {} {} {} {} {}",
            &digits[..5],
            &digits[5..10],
            &digits[10..15],
            &digits[15..20],
            &digits[20..25],
            &digits[25..]
        ))
    }

    fn validate_signal(
        &self,
        signal: &E2eeSignal,
        expected: SignalKind,
    ) -> Result<(), E2eeGroupError> {
        if signal.version != SIGNAL_VERSION {
            return Err(E2eeGroupError::UnsupportedSignalVersion);
        }

        if signal.kind != expected {
            return Err(E2eeGroupError::UnexpectedSignalKind);
        }

        if self.active_group()?.group_id().as_slice() != signal.group_id {
            return Err(E2eeGroupError::WrongGroup);
        }

        Ok(())
    }

    fn active_group(&self) -> Result<&MlsGroup, E2eeGroupError> {
        let group = self.group.as_ref().ok_or(E2eeGroupError::GroupNotReady)?;

        if !group.is_active() {
            return Err(E2eeGroupError::GroupInactive);
        }

        Ok(group)
    }

    /// The media boundary allows a short previous-epoch receive window, so keep only the
    /// exporter keys for the epoch that is about to become previous. A second Commit discards
    /// the older cache before it can be used again.
    fn retain_current_epoch_media_keys(&mut self) -> Result<(), E2eeGroupError> {
        let epoch = self.epoch()?;
        let members = self.members()?;
        let mut keys = Vec::with_capacity(members.len() * MEDIA_ORIGINS.len());

        for member in members {
            for origin in MEDIA_ORIGINS {
                keys.push((
                    member.member_id.clone(),
                    origin,
                    self.media_key_for_epoch(epoch, &member.member_id, origin)?,
                ));
            }
        }

        self.previous_media_keys
            .retain(|(cached_epoch, _, _), _| *cached_epoch >= epoch);

        for (member_id, origin, key) in keys {
            self.previous_media_keys
                .insert((epoch, member_id, origin), key);
        }

        Ok(())
    }
}

fn signal_from_message(
    group_id: Vec<u8>,
    epoch: Epoch,
    kind: SignalKind,
    sender_account_id: &str,
    message: MlsMessageOut,
) -> Result<E2eeSignal, E2eeGroupError> {
    Ok(E2eeSignal {
        version: SIGNAL_VERSION,
        group_id,
        epoch,
        kind,
        sender_account_id: sender_account_id.to_owned(),
        payload: message.tls_serialize_detached().map_err(mls_error)?,
    })
}

fn credential_identity(credential: &Credential) -> Result<String, E2eeGroupError> {
    let basic = BasicCredential::try_from(credential.clone()).map_err(mls_error)?;

    String::from_utf8(basic.identity().to_vec())
        .map_err(|_| E2eeGroupError::Mls("Basic Credential is not a UTF-8 account id".to_owned()))
}

fn media_exporter_context(
    group_id: &[u8],
    epoch: Epoch,
    participant: &MemberId,
    origin: Origin,
) -> Result<Vec<u8>, E2eeGroupError> {
    let mut context = Vec::with_capacity(group_id.len() + participant.0.len() + origin.len() + 24);

    append_len_prefixed_bytes(&mut context, group_id)?;
    context.extend(epoch.to_be_bytes());
    append_len_prefixed_bytes(&mut context, &participant.0)?;
    context.extend(origin);

    Ok(context)
}

fn append_len_prefixed(hash: &mut Sha256, bytes: &[u8]) -> Result<(), E2eeGroupError> {
    let length = u32::try_from(bytes.len())
        .map_err(|_| E2eeGroupError::Mls("privacy-code input is too large".to_owned()))?;

    hash.update(length.to_be_bytes());
    hash.update(bytes);
    Ok(())
}

fn append_len_prefixed_bytes(output: &mut Vec<u8>, bytes: &[u8]) -> Result<(), E2eeGroupError> {
    let length = u32::try_from(bytes.len())
        .map_err(|_| E2eeGroupError::Mls("MLS exporter context is too large".to_owned()))?;

    output.extend(length.to_be_bytes());
    output.extend(bytes);
    Ok(())
}

fn mls_error(error: impl std::fmt::Debug) -> E2eeGroupError {
    E2eeGroupError::Mls(format!("{error:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const GROUP_ID: &[u8] = b"unkvoid-e2ee-phase-2-test-call";
    const CAMERA: Origin = [0, 0, 0, 1];
    const SCREEN: Origin = [0, 0, 0, 2];

    fn establish(names: &[&str]) -> Vec<E2eeGroup> {
        assert!(!names.is_empty());

        let mut members = names
            .iter()
            .map(|name| E2eeGroup::new(*name).expect("session identity"))
            .collect::<Vec<_>>();
        members[0].create_group(GROUP_ID).expect("create group");

        for joining_index in 1..members.len() {
            let key_package = members[joining_index]
                .key_package_signal(GROUP_ID)
                .expect("key package");
            let commit = members[0]
                .add_member(&key_package)
                .expect("stage add member");
            let welcome = members[0]
                .confirm_pending()
                .expect("accept staged add")
                .expect("add has a welcome only after acceptance");

            for member in members.iter_mut().take(joining_index).skip(1) {
                assert_eq!(
                    member.process_commit(&commit).expect("process add"),
                    CommitDisposition::Applied
                );
            }

            members[joining_index]
                .join_welcome(&welcome)
                .expect("join welcome");
        }

        members
    }

    fn assert_converged<'member>(
        members: impl IntoIterator<Item = &'member E2eeGroup>,
        participant: &MemberId,
        origin: Origin,
    ) {
        let members = members.into_iter().collect::<Vec<_>>();
        let epoch = members[0].epoch().expect("epoch");
        let key = members[0]
            .media_key(participant, origin)
            .expect("media key");

        for member in members {
            assert_eq!(member.epoch().expect("epoch"), epoch);
            assert_eq!(
                member.media_key(participant, origin).expect("media key"),
                key
            );
        }
    }

    #[test]
    fn two_participants_converge_on_epoch_privacy_code_and_media_keys() {
        let members = establish(&["alice", "bob"]);
        let alice = members[0].member_id();
        let bob = members[1].member_id();

        assert_eq!(
            members[0]
                .members()
                .expect("members")
                .into_iter()
                .map(|member| member.account_id)
                .collect::<Vec<_>>(),
            vec!["alice", "bob"]
        );
        assert_eq!(
            members[0]
                .designated_committer()
                .expect("committer")
                .member_id,
            alice
        );
        assert_converged(&members, &alice, CAMERA);
        assert_converged(&members, &bob, SCREEN);
        assert_eq!(
            members[0].privacy_code().expect("privacy"),
            members[1].privacy_code().expect("privacy")
        );

        let mut sender = members[0].media_sender(CAMERA).expect("media sender");
        let encrypted = sender.encrypt_opus(&[0x5a; 160]).expect("encrypt");
        let mut receiver = members[1]
            .media_receiver(&alice, CAMERA, Policy::Require)
            .expect("media receiver");
        assert_eq!(
            receiver
                .decrypt_opus(&encrypted)
                .expect("decrypt")
                .into_bytes(),
            vec![0x5a; 160]
        );
    }

    #[test]
    fn five_participants_converge_on_every_sender_key() {
        let members = establish(&["alice", "bob", "carol", "dave", "erin"]);

        for participant in members[0].members().expect("members") {
            assert_converged(&members, &participant.member_id, CAMERA);
            assert_converged(&members, &participant.member_id, SCREEN);
        }

        let code = members[0].privacy_code().expect("privacy");
        assert!(
            members
                .iter()
                .all(|member| member.privacy_code().expect("privacy") == code)
        );
    }

    #[test]
    fn removed_member_cannot_export_the_next_epoch_and_new_member_cannot_export_the_previous_one() {
        let mut members = establish(&["alice", "bob", "carol"]);
        let alice = members[0].member_id();
        let old_epoch = members[0].epoch().expect("old epoch");
        let old_key = members[0].media_key(&alice, CAMERA).expect("old key");
        let remove = members[0].remove_member("bob").expect("remove bob");
        assert_eq!(members[0].epoch().expect("unconfirmed epoch"), old_epoch);
        assert_eq!(members[0].confirm_pending().expect("accept removal"), None);

        assert_eq!(
            members[1]
                .process_commit(&remove)
                .expect("bob processes removal"),
            CommitDisposition::Applied
        );
        assert_eq!(
            members[2]
                .process_commit(&remove)
                .expect("carol processes removal"),
            CommitDisposition::Applied
        );
        assert_eq!(members[0].epoch().expect("new epoch"), old_epoch + 1);
        assert_eq!(
            members[2].media_key(&alice, CAMERA).expect("new key"),
            members[0].media_key(&alice, CAMERA).expect("new key")
        );
        assert_ne!(
            members[0].media_key(&alice, CAMERA).expect("new key"),
            old_key
        );
        assert_eq!(
            members[1].media_key(&alice, CAMERA),
            Err(E2eeGroupError::GroupInactive)
        );

        let mut dave = E2eeGroup::new("dave").expect("dave identity");
        assert_eq!(
            dave.media_key_for_epoch(old_epoch, &alice, CAMERA),
            Err(E2eeGroupError::GroupNotReady)
        );
        let key_package = dave.key_package_signal(GROUP_ID).expect("dave key package");
        let commit = members[0].add_member(&key_package).expect("stage dave");
        let welcome = members[0]
            .confirm_pending()
            .expect("accept dave")
            .expect("welcome after acceptance");

        assert_eq!(
            members[2]
                .process_commit(&commit)
                .expect("carol processes add"),
            CommitDisposition::Applied
        );
        dave.join_welcome(&welcome).expect("dave joins");
        assert_eq!(
            dave.media_key_for_epoch(old_epoch + 1, &alice, CAMERA),
            Err(E2eeGroupError::EpochUnavailable)
        );
        assert_converged([&members[0], &members[2], &dave], &alice, CAMERA);
    }

    #[test]
    fn duplicate_delayed_and_out_of_order_commits_are_non_fatal() {
        let mut members = establish(&["alice", "bob"]);
        let mut carol = E2eeGroup::new("carol").expect("carol identity");
        let carol_key_package = carol
            .key_package_signal(GROUP_ID)
            .expect("carol key package");
        let carol_commit = members[0]
            .add_member(&carol_key_package)
            .expect("stage carol");
        let carol_welcome = members[0]
            .confirm_pending()
            .expect("accept carol")
            .expect("carol welcome");
        let mut dave = E2eeGroup::new("dave").expect("dave identity");
        let dave_key_package = dave.key_package_signal(GROUP_ID).expect("dave key package");
        let dave_commit = members[0]
            .add_member(&dave_key_package)
            .expect("stage dave");
        let dave_welcome = members[0]
            .confirm_pending()
            .expect("accept dave")
            .expect("dave welcome");

        assert_eq!(
            members[1]
                .process_commit(&dave_commit)
                .expect("future commit"),
            CommitDisposition::DeferredFuture
        );
        assert_eq!(
            members[1]
                .process_commit(&carol_commit)
                .expect("first commit"),
            CommitDisposition::Applied
        );
        assert_eq!(
            members[1]
                .process_commit(&carol_commit)
                .expect("duplicate commit"),
            CommitDisposition::IgnoredStale
        );
        assert_eq!(
            members[1]
                .process_commit(&dave_commit)
                .expect("retried future commit"),
            CommitDisposition::Applied
        );
        carol.join_welcome(&carol_welcome).expect("carol joins");
        assert_eq!(
            carol
                .process_commit(&dave_commit)
                .expect("carol catches up"),
            CommitDisposition::Applied
        );
        dave.join_welcome(&dave_welcome).expect("dave joins");
        assert_converged(
            [&members[0], &members[1], &carol, &dave],
            &members[0].member_id(),
            CAMERA,
        );
    }

    #[test]
    fn privacy_code_is_equal_for_peers_and_changes_on_join_and_leave() {
        let mut members = establish(&["alice", "bob"]);
        let before = members[0].privacy_code().expect("privacy before");
        let mut carol = E2eeGroup::new("carol").expect("carol identity");
        let key_package = carol.key_package_signal(GROUP_ID).expect("key package");
        let add_commit = members[0].add_member(&key_package).expect("stage carol");
        let welcome = members[0]
            .confirm_pending()
            .expect("accept carol")
            .expect("welcome after acceptance");

        assert_eq!(
            members[1]
                .process_commit(&add_commit)
                .expect("bob processes add"),
            CommitDisposition::Applied
        );
        carol.join_welcome(&welcome).expect("carol joins");
        let after_join = members[0].privacy_code().expect("privacy after join");
        assert_ne!(before, after_join);
        assert_eq!(after_join, members[1].privacy_code().expect("bob privacy"));
        assert_eq!(after_join, carol.privacy_code().expect("carol privacy"));
        assert!(after_join.split(' ').all(|group| group.len() == 5));
        assert_eq!(after_join.len(), 35, "six groups of five digits");

        let remove = members[0].remove_member("bob").expect("remove bob");
        members[0].confirm_pending().expect("accept removal");
        assert_eq!(
            members[1]
                .process_commit(&remove)
                .expect("bob processes remove"),
            CommitDisposition::Applied
        );
        assert_eq!(
            carol
                .process_commit(&remove)
                .expect("carol processes remove"),
            CommitDisposition::Applied
        );
        let after_leave = members[0].privacy_code().expect("privacy after leave");
        assert_ne!(after_join, after_leave);
        assert_eq!(
            after_leave,
            carol.privacy_code().expect("carol privacy after leave")
        );
    }

    #[test]
    fn concurrent_commits_are_staged_until_one_is_accepted_and_the_other_is_discarded() {
        let mut members = establish(&["alice", "bob"]);
        let mut carol = E2eeGroup::new("carol").expect("carol identity");
        let dave = E2eeGroup::new("dave").expect("dave identity");
        let carol_package = carol
            .key_package_signal(GROUP_ID)
            .expect("carol key package");
        let dave_package = dave.key_package_signal(GROUP_ID).expect("dave key package");
        let alice_commit = members[0]
            .add_member(&carol_package)
            .expect("alice stages first commit");
        let bob_commit = members[1]
            .add_member(&dave_package)
            .expect("bob stages competing commit");

        assert_eq!(
            members[0].epoch().expect("alice remains in current epoch"),
            1
        );
        assert_eq!(members[1].epoch().expect("bob remains in current epoch"), 1);
        assert_eq!(
            members[1]
                .process_commit(&alice_commit)
                .expect("winning commit waits for explicit rejection"),
            CommitDisposition::DeferredPendingLocal
        );

        // The relay accepts the first Commit it received (Alice) and rejects Bob's matching
        // epoch. Alice releases Carol's Welcome only after that acceptance; Bob clears his
        // losing staged state before processing the winner.
        let carol_welcome = members[0]
            .confirm_pending()
            .expect("alice accepts")
            .expect("welcome after acceptance");
        members[1]
            .discard_pending()
            .expect("bob discards rejected commit");
        assert_eq!(
            members[1]
                .process_commit(&alice_commit)
                .expect("bob processes winner"),
            CommitDisposition::Applied
        );
        carol
            .join_welcome(&carol_welcome)
            .expect("Carol joins only after accepted commit");

        let alice = members[0].member_id();
        assert_converged([&members[0], &members[1], &carol], &alice, CAMERA);
        assert_eq!(
            members[0].privacy_code().expect("privacy"),
            members[1].privacy_code().expect("privacy")
        );
        assert_ne!(
            alice_commit.payload, bob_commit.payload,
            "the competing Commit was real and distinct"
        );
        assert_eq!(
            dave.epoch(),
            Err(E2eeGroupError::GroupNotReady),
            "rejected Commit leaked no Welcome"
        );
    }

    #[test]
    fn duplicate_account_sessions_have_distinct_media_keys_and_remove_together() {
        let mut members = establish(&["bob", "alice", "alice"]);
        let alice_sessions = members[0]
            .members()
            .expect("members")
            .into_iter()
            .filter(|member| member.account_id == "alice")
            .collect::<Vec<_>>();

        assert_eq!(alice_sessions.len(), 2);
        assert_ne!(alice_sessions[0].member_id, alice_sessions[1].member_id);
        assert_ne!(
            members[0]
                .media_key(&alice_sessions[0].member_id, CAMERA)
                .expect("first alice key"),
            members[0]
                .media_key(&alice_sessions[1].member_id, CAMERA)
                .expect("second alice key"),
            "two devices must never share a ChaCha20-Poly1305 key/counter pair"
        );

        let remove = members[0]
            .remove_member("alice")
            .expect("remove every Alice device");
        members[0].confirm_pending().expect("accept removal");
        assert_eq!(
            members[1]
                .process_commit(&remove)
                .expect("first Alice removal"),
            CommitDisposition::Applied
        );
        assert_eq!(
            members[2]
                .process_commit(&remove)
                .expect("second Alice removal"),
            CommitDisposition::Applied
        );
        assert_eq!(members[0].members().expect("remaining members").len(), 1);
    }

    #[test]
    fn rejected_commit_exposes_a_stable_reason() {
        let mut members = establish(&["alice", "bob"]);
        let mut invalid = members[0]
            .key_package_signal(GROUP_ID)
            .expect("signal envelope");
        invalid.kind = SignalKind::Commit;
        invalid.payload = vec![0xff];

        assert_eq!(
            members[1]
                .process_commit(&invalid)
                .expect("invalid current-epoch commit is non-fatal"),
            CommitDisposition::Rejected {
                reason: CommitRejectReason::MalformedPayload
            }
        );
    }
}
