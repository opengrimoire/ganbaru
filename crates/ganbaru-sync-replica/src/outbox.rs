//! The device-local proposal outbox of an admin installation.
//!
//! An admin keeps every proposal it signs here until its own replica's selected chain holds it
//! or the anchor rejects it with a final reason. Each pass replays the waiting proposals, so a
//! proposal survives a crash between signing and sequencing, an unreachable anchor, an anchor
//! restored from a backup, and a handover that supersedes the chain that held it. The anchor's
//! own proposals take the same path.

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
/// the anchor of the selected chain, while its epoch is frozen, after a handover won, or while
/// a new anchor waits for writers the old one stored, because each of these settles once the
/// chain moves on.
pub fn is_final(rejection: ProposalRejection) -> bool {
    !matches!(
        rejection,
        ProposalRejection::NotAnchor
            | ProposalRejection::EpochFrozen
            | ProposalRejection::HandoverConflict
            | ProposalRejection::CatchingUp
    )
}

/// One proposal in the outbox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboxEntry {
    /// The signed proposal.
    pub proposal: SignedProposal,
    /// The final rejection the anchor gave, or `None` while the proposal waits.
    pub rejection: Option<ProposalRejection>,
}

impl OutboxEntry {
    /// Whether the proposal still waits for an anchor.
    pub fn pending(&self) -> bool {
        self.rejection.is_none()
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
    /// a final rejection it can drop.
    pub fn has_room(&self) -> bool {
        self.entries.len() < MAX_OUTBOX_PROPOSALS
            || self.entries.iter().any(|entry| !entry.pending())
    }

    /// Adds a waiting proposal and writes the outbox. A proposal already present is left as it
    /// is. At the bound the oldest final rejection makes room, so rejections never block new
    /// proposals; an outbox full of waiting proposals refuses. Returns whether the proposal was
    /// added.
    pub fn add(&mut self, proposal: SignedProposal) -> Result<bool, String> {
        let hash = proposal.hash();
        if self.position(&hash).is_some() {
            return Ok(false);
        }
        let mut next = self.entries.clone();
        if next.len() >= MAX_OUTBOX_PROPOSALS {
            let Some(rejected) = next.iter().position(|entry| !entry.pending()) else {
                return Err("sync proposal outbox is full".to_string());
            };
            next.remove(rejected);
        }
        next.push(OutboxEntry {
            proposal,
            rejection: None,
        });
        self.replace(next)?;
        Ok(true)
    }

    /// Records the anchor's answer to a proposal. A final rejection marks it rejected, so it is
    /// never proposed again; a revision or a rejection that is not final leaves it waiting until
    /// [`Self::prune`] finds it in the selected chain.
    pub fn settle(
        &mut self,
        hash: &Digest32,
        outcome: Result<u64, ProposalRejection>,
    ) -> Result<(), String> {
        let Err(rejection) = outcome else {
            return Ok(());
        };
        if !is_final(rejection) {
            return Ok(());
        }
        let Some(index) = self.position(hash) else {
            return Ok(());
        };
        if self.entries[index].rejection == Some(rejection) {
            return Ok(());
        }
        let mut next = self.entries.clone();
        next[index].rejection = Some(rejection);
        self.replace(next)
    }

    /// Drops every waiting proposal the selected chain of the local replica holds.
    pub async fn prune(
        &mut self,
        engine: &Engine,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
    ) -> Result<(), String> {
        let mut next = Vec::with_capacity(self.entries.len());
        for entry in &self.entries {
            let held = entry.pending()
                && engine
                    .proposal_revision(conn, ctx, &entry.proposal.hash())
                    .await
                    .map_err(|error| format!("read sync proposal revision: {error}"))?
                    .is_some();
            if !held {
                next.push(entry.clone());
            }
        }
        if next.len() == self.entries.len() {
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
            proposals: next
                .iter()
                .map(|entry| StoredProposal {
                    proposal: URL_SAFE_NO_PAD.encode(entry.proposal.encode()),
                    rejection: entry.rejection.map(ProposalRejection::as_u8),
                })
                .collect(),
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
    Ok(OutboxEntry {
        proposal,
        rejection,
    })
}
