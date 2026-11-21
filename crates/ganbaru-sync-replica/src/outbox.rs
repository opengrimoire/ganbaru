//! The device-local proposal outbox of an admin installation.
//!
//! An admin keeps every proposal it signs here until its own replica's selected chain holds it
//! or the anchor rejects it with a final reason. Each pass replays the waiting proposals, so a
//! proposal survives a crash between signing and sequencing, an unreachable anchor, an anchor
//! restored from a backup, and a handover that supersedes the chain that held it. The anchor's
//! own proposals take the same path.
//!
//! Each entry is in one [`OutboxState`]: gated on a handover the chain does not hold yet,
//! pending, held by a selected entry, or finally rejected. [`OutboxState::transition`] owns
//! every move, and loading refuses a file whose entry combines states.

use std::path::{Path, PathBuf};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ganbaru_atomic_files::{
    FileAccess, FinalSymlink, create_private_directories, read_regular_file_capped,
    write_file_atomically,
};
use ganbaru_sync::{Engine, SpaceContext};
use ganbaru_sync_contracts::{Digest32, Entry, ProposalRejection, SignedProposal};
use serde::{Deserialize, Serialize};
use sqlx::SqliteConnection;

const OUTBOX_SCHEMA_VERSION: u32 = 1;

/// Most proposals an outbox holds. Admins propose one change at a time from the UI and
/// reconciliation, so a full outbox means proposals are not reaching an anchor.
pub const MAX_OUTBOX_PROPOSALS: usize = 256;

/// Largest outbox file this installation writes or reads.
pub const MAX_OUTBOX_BYTES: usize = 4 * 1024 * 1024;

/// Whether the anchor's rejection ends a proposal. A proposal waits while the receiver is not
/// the anchor of the selected chain, while its epoch is frozen, after a handover won, while a
/// new anchor waits for writers the old one stored, while the anchor holds the signer apart,
/// while the anchor cannot open the current key epoch, or while a key rotation is pending,
/// because each of these settles once the chain or the anchor moves on.
pub fn is_final(rejection: ProposalRejection) -> bool {
    !matches!(
        rejection,
        ProposalRejection::NotAnchor
            | ProposalRejection::EpochFrozen
            | ProposalRejection::HandoverConflict
            | ProposalRejection::CatchingUp
            | ProposalRejection::DeviceQuarantined
            | ProposalRejection::KeyUnavailable
            | ProposalRejection::RotationPending
    )
}

/// Where one proposal of the outbox stands. `released_from` names the handover that released
/// the proposal from its gate, kept so the outcome of that handover's follow-ups can be counted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutboxState {
    /// Waits for the handover whose proposal hash is `after`. A gated proposal is neither sent
    /// nor sequenced until the selected chain holds that handover, so the follow-ups of a
    /// forced handover are durable before it seals and run only after it.
    Gated {
        /// The handover the proposal waits for.
        after: Digest32,
    },
    /// Waits for an anchor to sequence it.
    Pending {
        /// The handover that released the proposal, if it was gated.
        released_from: Option<Digest32>,
    },
    /// The local replica's selected chain holds the proposal in an entry of `epoch`. It is
    /// kept until a later epoch holds it too, because a handover sealed from an earlier
    /// revision supersedes it and then it waits again.
    Held {
        /// The epoch of the selected entry that holds the proposal.
        epoch: u32,
        /// The handover that released the proposal, if it was gated.
        released_from: Option<Digest32>,
    },
    /// The anchor refused the proposal for a final reason, so it is never proposed again.
    Rejected {
        /// The final rejection the anchor gave.
        rejection: ProposalRejection,
        /// The handover that released the proposal, if it was gated.
        released_from: Option<Digest32>,
    },
}

/// What the outbox learned about one proposal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutboxEvent {
    /// The anchor refused the proposal with this final rejection.
    Rejected(ProposalRejection),
    /// The local replica's selected chain moved, see [`ChainFacts`].
    Followed(ChainFacts),
}

/// What the local replica's selected chain says about one proposal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChainFacts {
    /// The epoch of the selected chain, or `None` before it has one.
    pub epoch: Option<u32>,
    /// Whether the chain holds the handover a gated proposal waits for. Ignored for a
    /// proposal that is not gated.
    pub gate_sealed: bool,
    /// The epoch of the selected entry that holds the proposal, when one does.
    pub held_at: Option<u32>,
}

/// A move [`OutboxState::transition`] refuses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IllegalOutboxMove {
    /// A rejection that is not final never settles a proposal.
    NotFinal(ProposalRejection),
    /// A gated proposal was never sent, so no anchor answered it.
    RejectedWhileGated,
}

impl std::fmt::Display for IllegalOutboxMove {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFinal(rejection) => {
                write!(
                    formatter,
                    "rejection {rejection} does not settle a proposal"
                )
            }
            Self::RejectedWhileGated => {
                formatter.write_str("a gated proposal cannot be rejected before it is sent")
            }
        }
    }
}

impl OutboxState {
    /// Applies `event`. Returns the next state, or `None` when the proposal leaves the outbox
    /// because a later epoch of the selected chain still holds it.
    ///
    /// - A final rejection rejects a waiting or held proposal and replaces the reason of a
    ///   rejected one; it is refused for a gated proposal and for a reason that is not final.
    /// - Following the chain keeps a rejected proposal and a gated one whose handover the chain
    ///   does not hold yet. Any other proposal is held at the epoch of the entry that holds it,
    ///   dropped once the chain is in a later epoch, and waits while no entry holds it; a
    ///   released gate becomes its `released_from`.
    pub fn transition(self, event: OutboxEvent) -> Result<Option<Self>, IllegalOutboxMove> {
        match event {
            OutboxEvent::Rejected(rejection) => {
                if !is_final(rejection) {
                    return Err(IllegalOutboxMove::NotFinal(rejection));
                }
                match self {
                    Self::Gated { .. } => Err(IllegalOutboxMove::RejectedWhileGated),
                    Self::Pending { released_from }
                    | Self::Held { released_from, .. }
                    | Self::Rejected { released_from, .. } => Ok(Some(Self::Rejected {
                        rejection,
                        released_from,
                    })),
                }
            }
            OutboxEvent::Followed(facts) => Ok(self.follow(facts)),
        }
    }

    fn follow(self, facts: ChainFacts) -> Option<Self> {
        let released_from = match self {
            Self::Rejected { .. } => return Some(self),
            Self::Gated { .. } if !facts.gate_sealed => return Some(self),
            Self::Gated { after } => Some(after),
            Self::Pending { released_from } | Self::Held { released_from, .. } => released_from,
        };
        match facts.held_at {
            Some(held_at) if facts.epoch.is_some_and(|epoch| epoch > held_at) => None,
            Some(epoch) => Some(Self::Held {
                epoch,
                released_from,
            }),
            None => Some(Self::Pending { released_from }),
        }
    }
}

/// One proposal in the outbox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboxEntry {
    /// The signed proposal.
    pub proposal: SignedProposal,
    /// Where the proposal stands.
    pub state: OutboxState,
}

impl OutboxEntry {
    /// Whether the proposal still waits for an anchor and is not gated on a handover.
    pub fn pending(&self) -> bool {
        matches!(self.state, OutboxState::Pending { .. })
    }

    /// The final rejection the anchor gave, when it rejected the proposal.
    pub fn rejection(&self) -> Option<ProposalRejection> {
        match self.state {
            OutboxState::Rejected { rejection, .. } => Some(rejection),
            _ => None,
        }
    }

    /// The epoch of the selected entry that holds the proposal, when the chain holds it.
    pub fn held(&self) -> Option<u32> {
        match self.state {
            OutboxState::Held { epoch, .. } => Some(epoch),
            _ => None,
        }
    }

    /// The handover a gated proposal waits for.
    pub fn after(&self) -> Option<Digest32> {
        match self.state {
            OutboxState::Gated { after } => Some(after),
            _ => None,
        }
    }

    /// The handover that released the proposal from its gate.
    pub fn released_from(&self) -> Option<Digest32> {
        match self.state {
            OutboxState::Gated { .. } => None,
            OutboxState::Pending { released_from }
            | OutboxState::Held { released_from, .. }
            | OutboxState::Rejected { released_from, .. } => released_from,
        }
    }

    /// Whether [`Outbox::add`] may drop the entry to make room: a final rejection or a held
    /// proposal. A gated proposal is never dropped, since nothing else would sign it again.
    fn evictable(&self) -> bool {
        matches!(
            self.state,
            OutboxState::Held { .. } | OutboxState::Rejected { .. }
        )
    }

    /// Whether the entry belongs to the follow-ups of the handover `handover` and has not
    /// reached its goal: it is still gated, still waits, or the anchor rejected it for a reason
    /// other than the device being removed already or never enrolled.
    fn incomplete_after(&self, handover: &Digest32) -> bool {
        match self.state {
            OutboxState::Gated { after } => after == *handover,
            OutboxState::Pending { released_from } => released_from.as_ref() == Some(handover),
            OutboxState::Held { .. } => false,
            OutboxState::Rejected {
                rejection,
                released_from,
            } => {
                released_from.as_ref() == Some(handover)
                    && !matches!(
                        rejection,
                        ProposalRejection::DeviceRemoved | ProposalRejection::UnknownDevice
                    )
            }
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OutboxFile {
    schema_version: u32,
    proposals: Vec<StoredProposal>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredProposal {
    proposal: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    rejection: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    held: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    after: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    released_from: Option<String>,
}

/// The outbox file of one vault with the proposals it holds.
#[derive(Debug)]
pub struct Outbox {
    path: PathBuf,
    entries: Vec<OutboxEntry>,
}

impl Outbox {
    /// Reads the outbox at `path`, empty when the file does not exist. Refuses a file that is
    /// too large, does not parse, names an unknown rejection, or holds a proposal whose
    /// signature does not verify.
    pub fn load(path: &Path) -> Result<Self, String> {
        let bytes = match read_regular_file_capped(
            path,
            u64::try_from(MAX_OUTBOX_BYTES).unwrap_or(u64::MAX),
            FinalSymlink::Refuse,
        ) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self {
                    path: path.to_path_buf(),
                    entries: Vec::new(),
                });
            }
            Err(error) => return Err(format!("read sync proposal outbox: {error}")),
        };
        let file: OutboxFile = serde_json::from_slice(&bytes)
            .map_err(|error| format!("parse sync proposal outbox: {error}"))?;
        if file.schema_version != OUTBOX_SCHEMA_VERSION {
            return Err("sync proposal outbox has an unsupported schema version".to_string());
        }
        if file.proposals.len() > MAX_OUTBOX_PROPOSALS {
            return Err("sync proposal outbox holds too many proposals".to_string());
        }
        let entries = file
            .proposals
            .into_iter()
            .map(decode_entry)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            path: path.to_path_buf(),
            entries,
        })
    }

    /// Every proposal, in the order they were added.
    pub fn entries(&self) -> &[OutboxEntry] {
        &self.entries
    }

    /// The proposals still waiting for an anchor, in the order they were added.
    pub fn pending(&self) -> Vec<SignedProposal> {
        self.entries
            .iter()
            .filter(|entry| entry.pending())
            .map(|entry| entry.proposal.clone())
            .collect()
    }

    /// The first proposal, waiting or rejected, whose entry matches. Reconciliation proposes a
    /// change only when no proposal of it exists, because each signing takes a new creation
    /// time and so a new hash.
    pub fn find(&self, matches: impl Fn(&Entry) -> bool) -> Option<&OutboxEntry> {
        self.entries
            .iter()
            .find(|entry| matches(&entry.proposal.proposal.entry))
    }

    /// Whether [`Self::add`] can take another proposal: the outbox is below its bound or holds
    /// a final rejection or a held proposal it can drop.
    pub fn has_room(&self) -> bool {
        self.entries.len() < MAX_OUTBOX_PROPOSALS || self.entries.iter().any(OutboxEntry::evictable)
    }

    /// Adds a waiting proposal and writes the outbox. A proposal already present is left as it
    /// is. At the bound the oldest final rejection makes room, then the oldest held proposal, so
    /// neither blocks new proposals; an outbox full of waiting proposals refuses. Returns
    /// whether the proposal was added.
    pub fn add(&mut self, proposal: SignedProposal) -> Result<bool, String> {
        let hash = proposal.hash();
        if self.position(&hash).is_some() {
            return Ok(false);
        }
        let mut next = self.entries.clone();
        if next.len() >= MAX_OUTBOX_PROPOSALS {
            let Some(evicted) = next
                .iter()
                .position(|entry| entry.rejection().is_some())
                .or_else(|| next.iter().position(OutboxEntry::evictable))
            else {
                return Err("sync proposal outbox is full".to_string());
            };
            next.remove(evicted);
        }
        next.push(OutboxEntry {
            proposal,
            state: OutboxState::Pending {
                released_from: None,
            },
        });
        self.replace(next)?;
        Ok(true)
    }

    /// Adds `proposals`, in order, gated on the handover whose proposal hash is `after`, and
    /// writes the outbox once, so either all of them are kept or none is. A proposal already
    /// present is left as it is. Refuses when they do not fit, after dropping what
    /// [`Self::add`] would drop to make room.
    pub fn add_gated(
        &mut self,
        proposals: Vec<SignedProposal>,
        after: Digest32,
    ) -> Result<(), String> {
        let mut next = self.entries.clone();
        for proposal in proposals {
            let hash = proposal.hash();
            if next.iter().any(|entry| entry.proposal.hash() == hash) {
                continue;
            }
            if next.len() >= MAX_OUTBOX_PROPOSALS {
                let Some(evicted) = next
                    .iter()
                    .position(|entry| entry.rejection().is_some())
                    .or_else(|| next.iter().position(OutboxEntry::evictable))
                else {
                    return Err("sync proposal outbox is full".to_string());
                };
                next.remove(evicted);
            }
            next.push(OutboxEntry {
                proposal,
                state: OutboxState::Gated { after },
            });
        }
        if next == self.entries {
            return Ok(());
        }
        self.replace(next)
    }

    /// Drops every proposal still gated on a handover, as when that handover was refused or
    /// lost to another one, so it never runs.
    pub fn withdraw_gated(&mut self) -> Result<(), String> {
        if self.entries.iter().all(|entry| entry.after().is_none()) {
            return Ok(());
        }
        let next = self
            .entries
            .iter()
            .filter(|entry| entry.after().is_none())
            .cloned()
            .collect();
        self.replace(next)
    }

    /// How many follow-ups of the handover with proposal hash `handover` have not reached their
    /// goal: still gated, still waiting, or finally rejected. A rejection because the device was
    /// already removed or never enrolled counts as done, since the removal it asked for holds.
    pub fn forced_incomplete(&self, handover: &Digest32) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.incomplete_after(handover))
            .count()
    }

    /// Withdraws a proposal that was never sent, as when its signer could not keep the secret
    /// it names.
    pub fn withdraw(&mut self, hash: &Digest32) -> Result<(), String> {
        let Some(index) = self.position(hash) else {
            return Ok(());
        };
        let mut next = self.entries.clone();
        next.remove(index);
        self.replace(next)
    }

    /// Records the anchor's answer to a proposal. A final rejection marks a waiting or held
    /// proposal rejected, so it is never proposed again; a revision or a rejection that is not
    /// final leaves it waiting until [`Self::prune`] finds it in the selected chain. A rejection
    /// of a gated proposal answers nothing this outbox sent, so it changes nothing.
    pub fn settle(
        &mut self,
        hash: &Digest32,
        outcome: Result<u64, ProposalRejection>,
    ) -> Result<(), String> {
        let Err(rejection) = outcome else {
            return Ok(());
        };
        let Some(index) = self.position(hash) else {
            return Ok(());
        };
        let current = self.entries[index].state;
        let Ok(Some(state)) = current.transition(OutboxEvent::Rejected(rejection)) else {
            return Ok(());
        };
        if state == current {
            return Ok(());
        }
        let mut next = self.entries.clone();
        next[index].state = state;
        self.replace(next)
    }

    /// Follows the selected chain of the local replica with [`OutboxState::transition`]: a
    /// waiting proposal it holds is marked held at the epoch of its entry; a held proposal is
    /// dropped once the chain is in a later epoch that still holds it, and waits again when a
    /// handover from an earlier revision superseded its entry, so the next anchor sequences it
    /// again. A gated proposal is released once the selected chain holds the handover it waits
    /// for, and waits like any other.
    pub async fn prune(
        &mut self,
        engine: &Engine,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
    ) -> Result<(), String> {
        let epoch = engine
            .authorization(conn, ctx)
            .await
            .map_err(|error| format!("read sync authorization: {error}"))?
            .map(|view| view.epoch);
        let mut next = Vec::with_capacity(self.entries.len());
        for entry in &self.entries {
            let facts = chain_facts(engine, conn, ctx, entry, epoch).await?;
            let followed = entry
                .state
                .transition(OutboxEvent::Followed(facts))
                .map_err(|error| format!("follow sync proposal outbox: {error}"))?;
            if let Some(state) = followed {
                next.push(OutboxEntry {
                    proposal: entry.proposal.clone(),
                    state,
                });
            }
        }
        if next == self.entries {
            return Ok(());
        }
        self.replace(next)
    }

    /// Deletes the outbox file, as Reset sync state does.
    pub fn remove(path: &Path) -> Result<(), String> {
        match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!("remove sync proposal outbox: {error}")),
        }
    }

    /// The entry of the proposal with `hash`, waiting or finally rejected.
    pub fn get(&self, hash: &Digest32) -> Option<&OutboxEntry> {
        self.position(hash).map(|index| &self.entries[index])
    }

    fn position(&self, hash: &Digest32) -> Option<usize> {
        self.entries
            .iter()
            .position(|entry| entry.proposal.hash() == *hash)
    }

    fn replace(&mut self, next: Vec<OutboxEntry>) -> Result<(), String> {
        let file = OutboxFile {
            schema_version: OUTBOX_SCHEMA_VERSION,
            proposals: next.iter().map(encode_entry).collect(),
        };
        let bytes = serde_json::to_vec_pretty(&file)
            .map_err(|error| format!("encode sync proposal outbox: {error}"))?;
        if bytes.len() > MAX_OUTBOX_BYTES {
            return Err("sync proposal outbox is full".to_string());
        }
        if let Some(parent) = self.path.parent() {
            create_private_directories(parent)
                .map_err(|error| format!("create sync proposal outbox directory: {error}"))?;
        }
        write_file_atomically(&self.path, &bytes, FileAccess::OwnerOnly)
            .map_err(|error| format!("write sync proposal outbox: {error}"))?;
        self.entries = next;
        Ok(())
    }
}

/// Reads what the selected chain says about `entry`. The proposal's own position is read only
/// when it matters: not for a rejected proposal, nor for a gated one whose handover the chain
/// does not hold yet.
async fn chain_facts(
    engine: &Engine,
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    entry: &OutboxEntry,
    epoch: Option<u32>,
) -> Result<ChainFacts, String> {
    let mut facts = ChainFacts {
        epoch,
        gate_sealed: false,
        held_at: None,
    };
    match entry.state {
        OutboxState::Rejected { .. } => return Ok(facts),
        OutboxState::Gated { after } => {
            facts.gate_sealed = position_epoch(engine, conn, ctx, &after).await?.is_some();
            if !facts.gate_sealed {
                return Ok(facts);
            }
        }
        OutboxState::Pending { .. } | OutboxState::Held { .. } => {}
    }
    facts.held_at = position_epoch(engine, conn, ctx, &entry.proposal.hash()).await?;
    Ok(facts)
}

/// The epoch of the selected entry that holds the proposal with hash `hash`.
async fn position_epoch(
    engine: &Engine,
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    hash: &Digest32,
) -> Result<Option<u32>, String> {
    Ok(engine
        .proposal_position(conn, ctx, hash)
        .await
        .map_err(|error| format!("read sync proposal revision: {error}"))?
        .map(|(epoch, _)| epoch))
}

fn encode_entry(entry: &OutboxEntry) -> StoredProposal {
    let mut stored = StoredProposal {
        proposal: URL_SAFE_NO_PAD.encode(entry.proposal.encode()),
        rejection: None,
        held: None,
        after: None,
        released_from: entry
            .released_from()
            .map(|hash| URL_SAFE_NO_PAD.encode(hash)),
    };
    match entry.state {
        OutboxState::Gated { after } => stored.after = Some(URL_SAFE_NO_PAD.encode(after)),
        OutboxState::Pending { .. } => {}
        OutboxState::Held { epoch, .. } => stored.held = Some(epoch),
        OutboxState::Rejected { rejection, .. } => stored.rejection = Some(rejection.as_u8()),
    }
    stored
}

/// Decodes one stored entry. At most one of `after`, `held`, and `rejection` is set, and a
/// gated entry names no releasing handover, so a file that combines them is refused.
fn decode_entry(stored: StoredProposal) -> Result<OutboxEntry, String> {
    let bytes = URL_SAFE_NO_PAD
        .decode(stored.proposal.as_bytes())
        .map_err(|_| "sync proposal outbox holds a malformed proposal".to_string())?;
    let proposal = SignedProposal::decode(&bytes)
        .map_err(|error| format!("sync proposal outbox holds an invalid proposal: {error}"))?;
    let rejection = stored
        .rejection
        .map(|value| {
            ProposalRejection::from_u8(value)
                .filter(|rejection| is_final(*rejection))
                .ok_or_else(|| "sync proposal outbox names an unknown rejection".to_string())
        })
        .transpose()?;
    let after = stored.after.as_deref().map(decode_digest).transpose()?;
    let released_from = stored
        .released_from
        .as_deref()
        .map(decode_digest)
        .transpose()?;
    let state = match (after, stored.held, rejection, released_from) {
        (Some(after), None, None, None) => OutboxState::Gated { after },
        (None, None, None, released_from) => OutboxState::Pending { released_from },
        (None, Some(epoch), None, released_from) => OutboxState::Held {
            epoch,
            released_from,
        },
        (None, None, Some(rejection), released_from) => OutboxState::Rejected {
            rejection,
            released_from,
        },
        _ => return Err("sync proposal outbox holds a contradictory entry".to_string()),
    };
    Ok(OutboxEntry { proposal, state })
}

fn decode_digest(text: &str) -> Result<Digest32, String> {
    URL_SAFE_NO_PAD
        .decode(text.as_bytes())
        .ok()
        .and_then(|bytes| Digest32::try_from(bytes).ok())
        .ok_or_else(|| "sync proposal outbox names a malformed handover".to_string())
}
