//! State machine that binds one live SFU room to its in-memory MLS group.
//!
//! The SFU only orders opaque handshakes. This module decides whether this peer is the
//! deterministic committer, keeps the authenticated `peerId` to MLS-leaf association needed by
//! media, and exposes a deliberately small status surface to the native interfaces.

use std::collections::HashMap;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use media::{MediaReceiver, MediaSender, Policy, Source};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::e2ee_group::{
    CommitDisposition, E2eeGroup, E2eeGroupError, E2eeSignal, GroupMember, MemberId,
};
use crate::failure::Failure;

/// `room` is stable for a voice channel and `session_id` is a random value minted by the SFU
/// when that Room is created. Recreating an empty Room therefore cannot reuse an MLS group id.
pub fn group_id(room: &str, session_id: &str) -> Vec<u8> {
    let mut hash = Sha256::new();

    hash.update(b"unkvoid-mls-voice-room-v1");
    hash.update((room.len() as u64).to_be_bytes());
    hash.update(room.as_bytes());
    hash.update((session_id.len() as u64).to_be_bytes());
    hash.update(session_id.as_bytes());

    hash.finalize().to_vec()
}

/// The only encryption states the UI needs to render. Technical MLS/SFU details stay in tracing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallCipherState {
    Establishing,
    Encrypted,
    Failed(Failure),
}

/// MLS plus live-SFU bindings for one local voice session.
pub struct E2eeCall {
    group_id: Vec<u8>,
    group: E2eeGroup,
    own_peer_id: String,
    peer_members: HashMap<String, MemberId>,
    state: CallCipherState,
    /// A joining peer can receive the room's commit broadcast before its targeted Welcome.
    /// Commits are buffered briefly; those at/before the Welcome epoch are expected and stale.
    awaiting_welcome: bool,
    /// Commits racing ahead of the targeted Welcome are replayed only if newer than its epoch.
    pending_welcome_commits: Vec<E2eeSignal>,
}

impl E2eeCall {
    pub fn wire(&self, signal: &E2eeSignal) -> Value {
        json!({
            "version": signal.version,
            "group": URL_SAFE_NO_PAD.encode(&signal.group_id),
            "epoch": signal.epoch,
            "payload": URL_SAFE_NO_PAD.encode(&signal.payload),
        })
    }

    pub fn parse_wire(
        &self,
        value: &Value,
        sender_account_id: &str,
        kind: crate::e2ee_group::SignalKind,
    ) -> Result<E2eeSignal, E2eeGroupError> {
        let group_id = value["group"]
            .as_str()
            .and_then(|v| URL_SAFE_NO_PAD.decode(v).ok())
            .ok_or(E2eeGroupError::WrongGroup)?;
        let payload = value["payload"]
            .as_str()
            .and_then(|v| URL_SAFE_NO_PAD.decode(v).ok())
            .ok_or(E2eeGroupError::WrongGroup)?;
        let signal = E2eeSignal {
            version: value["version"].as_u64().unwrap_or_default() as u8,
            group_id,
            epoch: value["epoch"].as_u64().unwrap_or_default(),
            kind,
            sender_account_id: sender_account_id.to_owned(),
            payload,
        };
        self.require_group(&signal)?;
        Ok(signal)
    }

    pub fn sender_set(&self) -> Result<HashMap<Source, MediaSender>, E2eeGroupError> {
        [
            Source::Screen,
            Source::ScreenAudio,
            Source::Camera,
            Source::Mic,
        ]
        .into_iter()
        .map(|source| self.sender(source).map(|sender| (source, sender)))
        .collect()
    }

    pub fn receiver_for_public_key(
        &self,
        bytes: &[u8],
        source: Source,
    ) -> Result<MediaReceiver, E2eeGroupError> {
        let member = self
            .group
            .members()?
            .into_iter()
            .find(|member| member.member_id.as_bytes() == bytes)
            .ok_or(E2eeGroupError::UnknownMember)?;
        self.receiver(&member.member_id, source)
    }
    /// Makes the one MLS identity for this device/session. The first peer creates its group;
    /// later peers return the KeyPackage that must be relayed with `e2eeJoin`.
    pub fn start(
        account_id: &str,
        room: &str,
        session_id: &str,
        own_peer_id: String,
        first_peer: bool,
    ) -> Result<(Self, Option<E2eeSignal>), E2eeGroupError> {
        let group_id = group_id(room, session_id);
        let mut group = E2eeGroup::new(account_id)?;
        let own_member = group.member_id();
        let mut peer_members = HashMap::new();

        peer_members.insert(own_peer_id.clone(), own_member);

        if first_peer {
            group.create_group(&group_id)?;

            return Ok((
                Self {
                    group_id,
                    group,
                    own_peer_id,
                    peer_members,
                    state: CallCipherState::Encrypted,
                    awaiting_welcome: false,
                    pending_welcome_commits: Vec::new(),
                },
                None,
            ));
        }

        let signal = group.key_package_signal(&group_id)?;

        Ok((
            Self {
                group_id,
                group,
                own_peer_id,
                peer_members,
                state: CallCipherState::Establishing,
                awaiting_welcome: true,
                pending_welcome_commits: Vec::new(),
            },
            Some(signal),
        ))
    }

    pub fn state(&self) -> CallCipherState {
        self.state
    }

    pub fn group_id(&self) -> &[u8] {
        &self.group_id
    }

    pub fn own_member_id(&self) -> MemberId {
        self.group.member_id()
    }

    pub fn peer_member(&self, peer_id: &str) -> Option<&MemberId> {
        self.peer_members.get(peer_id)
    }

    pub fn is_designated_committer(&self) -> Result<bool, E2eeGroupError> {
        Ok(self.group.designated_committer()?.member_id == self.group.member_id())
    }

    pub fn is_designated_after_peer_leaves(&self, peer_id: &str) -> Result<bool, E2eeGroupError> {
        let leaving = self.peer_members.get(peer_id);
        let next = self
            .group
            .members()?
            .into_iter()
            .filter(|member| Some(&member.member_id) != leaving)
            .min_by_key(|member| member.leaf_index);
        Ok(next.is_some_and(|member| member.member_id == self.group.member_id()))
    }

    /// Validates a KeyPackage, records the SFU-authenticated sender peer, and returns whether it
    /// still needs to be added. Duplicate join/retry packets do not create a second leaf.
    pub fn receive_key_package(
        &mut self,
        sender_peer_id: &str,
        signal: &E2eeSignal,
    ) -> Result<bool, E2eeGroupError> {
        self.require_group(signal)?;
        let member_id = self.group.member_id_from_key_package(signal)?;
        self.peer_members
            .insert(sender_peer_id.to_owned(), member_id.clone());

        Ok(!self
            .group
            .members()?
            .into_iter()
            .any(|member| member.member_id == member_id))
    }

    pub fn stage_add(&mut self, key_package: &E2eeSignal) -> Result<E2eeSignal, E2eeGroupError> {
        self.group.add_member(key_package)
    }

    pub fn stage_remove_peer(
        &mut self,
        peer_id: &str,
    ) -> Result<Option<E2eeSignal>, E2eeGroupError> {
        let Some(member_id) = self.peer_members.get(peer_id).cloned() else {
            return Ok(None);
        };

        let commit = self.group.remove_member_id(&member_id)?;

        Ok(Some(commit))
    }

    pub fn confirm_pending(&mut self) -> Result<Option<E2eeSignal>, E2eeGroupError> {
        let welcome = self.group.confirm_pending()?;
        self.state = CallCipherState::Encrypted;

        Ok(welcome)
    }

    pub fn discard_pending(&mut self) -> Result<(), E2eeGroupError> {
        self.group.discard_pending()
    }

    pub fn receive_commit(
        &mut self,
        signal: &E2eeSignal,
    ) -> Result<CommitDisposition, E2eeGroupError> {
        self.require_group(signal)?;
        if self.awaiting_welcome {
            if self.pending_welcome_commits.len() == 64 {
                return Err(E2eeGroupError::TooManyPendingCommits);
            }
            self.pending_welcome_commits.push(signal.clone());
            return Ok(CommitDisposition::IgnoredStale);
        }
        let disposition = self.group.process_commit(signal)?;

        if matches!(disposition, CommitDisposition::Applied) {
            self.state = CallCipherState::Encrypted;
        }

        Ok(disposition)
    }

    pub fn receive_welcome(
        &mut self,
        signal: &E2eeSignal,
    ) -> Result<Vec<E2eeSignal>, E2eeGroupError> {
        if signal.group_id != self.group_id {
            return Err(E2eeGroupError::WrongGroup);
        }

        self.group.join_welcome(signal)?;
        self.awaiting_welcome = false;
        self.state = CallCipherState::Encrypted;
        let mut commits = std::mem::take(&mut self.pending_welcome_commits)
            .into_iter()
            .filter(|commit| commit.epoch > signal.epoch)
            .collect::<Vec<_>>();
        commits.sort_by_key(|commit| commit.epoch);

        Ok(commits)
    }

    /// A deferred future Commit means this client is missing ordering state. Sending a fresh
    /// KeyPackage asks the active designated committer for a new Welcome without trusting the
    /// relay to retain old MLS bytes.
    pub fn fresh_join_request(&self) -> Result<E2eeSignal, E2eeGroupError> {
        self.group.key_package_signal(&self.group_id)
    }

    pub fn sender(&self, source: Source) -> Result<MediaSender, E2eeGroupError> {
        self.group.media_sender(source.e2ee_origin())
    }

    pub fn receiver(
        &self,
        member_id: &MemberId,
        source: Source,
    ) -> Result<MediaReceiver, E2eeGroupError> {
        self.group
            .media_receiver(member_id, source.e2ee_origin(), Policy::Require)
    }

    pub fn members(&self) -> Result<Vec<GroupMember>, E2eeGroupError> {
        self.group.members()
    }

    pub fn privacy_code(&self) -> Result<String, E2eeGroupError> {
        self.group.privacy_code()
    }

    pub fn fail(&mut self) {
        self.state = CallCipherState::Failed(Failure::E2ee);
    }

    /// Stable, UI-safe data: the code is human-verifiable and member ids are public MLS
    /// signature keys. No exporter output, KeyPackage, Welcome, or raw failure is exposed.
    pub fn status(&self) -> Value {
        let state = match self.state {
            CallCipherState::Establishing => json!({ "state": "establishing" }),
            CallCipherState::Encrypted => json!({ "state": "encrypted" }),
            CallCipherState::Failed(reason) => json!({ "state": "failed", "reason": reason }),
        };

        let members = self
            .members()
            .unwrap_or_default()
            .into_iter()
            .map(|member| {
                json!({
                    "accountId": member.account_id,
                    "memberId": URL_SAFE_NO_PAD.encode(member.member_id.as_bytes()),
                    "leafIndex": member.leaf_index,
                })
            })
            .collect::<Vec<_>>();

        let mut status = state;
        let Some(fields) = status.as_object_mut() else {
            return status;
        };

        fields.insert(
            "privacyCode".to_owned(),
            self.privacy_code()
                .map(Value::String)
                .unwrap_or(Value::Null),
        );
        fields.insert("members".to_owned(), Value::Array(members));
        fields.insert(
            "group".to_owned(),
            Value::String(URL_SAFE_NO_PAD.encode(&self.group_id)),
        );

        status
    }

    pub fn own_peer_id(&self) -> &str {
        &self.own_peer_id
    }

    fn require_group(&self, signal: &E2eeSignal) -> Result<(), E2eeGroupError> {
        if signal.group_id == self.group_id {
            Ok(())
        } else {
            Err(E2eeGroupError::WrongGroup)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_active_room_session_makes_a_stable_distinct_group_id() {
        assert_eq!(
            group_id("voice:1", "session-a"),
            group_id("voice:1", "session-a")
        );
        assert_ne!(
            group_id("voice:1", "session-a"),
            group_id("voice:1", "session-b")
        );
    }

    #[test]
    fn joiner_ignores_commit_broadcast_until_its_welcome_arrives() {
        let (mut owner, _) =
            E2eeCall::start("user:a", "voice:1", "session-a", "peer-a".into(), true).unwrap();
        let (mut joiner, key_package) =
            E2eeCall::start("user:b", "voice:1", "session-a", "peer-b".into(), false).unwrap();
        let key_package = key_package.unwrap();
        owner.receive_key_package("peer-b", &key_package).unwrap();
        let commit = owner.stage_add(&key_package).unwrap();
        let welcome = owner
            .confirm_pending()
            .unwrap()
            .expect("Welcome do novo membro");

        let (_later_joiner, later_package) =
            E2eeCall::start("user:c", "voice:1", "session-a", "peer-c".into(), false).unwrap();
        let later_package = later_package.unwrap();
        owner.receive_key_package("peer-c", &later_package).unwrap();
        let newer_commit = owner.stage_add(&later_package).unwrap();
        owner.confirm_pending().unwrap();

        assert_eq!(
            joiner.receive_commit(&commit).unwrap(),
            CommitDisposition::IgnoredStale
        );
        assert_eq!(
            joiner.receive_commit(&newer_commit).unwrap(),
            CommitDisposition::IgnoredStale
        );
        let pending = joiner.receive_welcome(&welcome).unwrap();
        assert_eq!(
            pending.len(),
            1,
            "o commit posterior ao Welcome deve ser preservado"
        );
        assert_eq!(
            joiner.receive_commit(&pending[0]).unwrap(),
            CommitDisposition::Applied
        );
        assert_eq!(
            joiner.receive_commit(&commit).unwrap(),
            CommitDisposition::IgnoredStale
        );
    }
}
