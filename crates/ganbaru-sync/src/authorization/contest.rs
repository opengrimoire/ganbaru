//! Yields and objections a device seals into a contested epoch instance.
//!
//! Both are ordinary kind 4 records, but neither continues the selected head: a yield continues
//! the best entry of the instance its author's own handover opened, or, for the anchor of the
//! contested instance, the best entry of that instance past one of its objections; an objection
//! continues the anchor's best entry of the contested instance past the candidate's parent.
//! Each is sealed by the current local writer at its next sequence and enters the record store
//! at once, so the outbox pushes it with the writer's other operations.

use ganbaru_sync_contracts::bounds::DISCOVERY_KEY_BYTES;
use ganbaru_sync_contracts::{
    AdminKeyPair, AdminPublicKey, AuthorizationRecord, Digest32, Entry, Proposal,
    ProposalRejection, Remove, Sequencing, SignedProposal, VersionVector, Yield, sign_proposal,
};
use sqlx::{Connection, SqliteConnection};

use super::{AuthState, Outcome, Resolution, Stored, removal_cutoffs};
use crate::error::{SyncError, SyncResult, failpoint};
use crate::{Engine, LocalWriter, SpaceContext, log, sql};

/// Why a yield or an objection was not sealed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContestRejection {
    /// The local admin key authored no other candidate out of the instance, and the local
    /// device is not its anchor with an objection to the named handover.
    NotContestant,
    /// No stored handover has the named hash.
    UnknownCandidate,
    /// The local admin key already yielded to the named handover.
    AlreadyYielded,
    /// The instance the named handover leaves is not frozen, so a yield changes nothing.
    NotFrozen,
    /// The local device is not the anchor of the instance the handover leaves, or the local
    /// admin key is not that anchor's admin key at the handover's parent.
    NotAnchor,
    /// The named entry is not a stored handover.
    NotCandidate,
    /// The instance already objects to the handover.
    AlreadyObjected,
    /// A kit handover pins the instance's winner, or the anchor already yielded to the
    /// handover, so an objection changes nothing.
    Resolved,
    /// The rules at the chosen parent refuse the record.
    Rejected(ProposalRejection),
}

impl std::fmt::Display for ContestRejection {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let reason = match self {
            Self::NotContestant => "this device has no take in the contest",
            Self::UnknownCandidate => "no stored handover has that hash",
            Self::AlreadyYielded => "this device already gave way in the contest",
            Self::NotFrozen => "the contest is already resolved",
            Self::NotAnchor => "this device is not the anchor the handover replaces",
            Self::NotCandidate => "the entry is not a stored handover",
            Self::AlreadyObjected => "the handover is already blocked",
            Self::Resolved => "the handover can no longer be blocked",
            Self::Rejected(rejection) => return rejection.fmt(formatter),
        };
        formatter.write_str(reason)
    }
}

/// Where a contest record continues from and what it carries.
pub(super) struct ContestPoint {
    /// State after the parent entry.
    state: AuthState,
    /// Entry the record carries.
    entry: Entry,
}

impl Resolution<'_> {
    /// The stored handover with hash `handover`.
    fn candidate(&self, handover: &Digest32) -> Option<usize> {
        (1..self.walk.visits.len()).find(|index| {
            let visit = &self.walk.visits[*index];
            visit.hash == *handover && visit.handover.is_some()
        })
    }

    /// The anchor device of the instance opened at `root`.
    pub(super) fn instance_anchor(&self, root: usize) -> String {
        self.walk.selection(root).state.anchor_device_id
    }

    /// The visit of highest rank among `visits`.
    fn best_of(&self, visits: impl Iterator<Item = usize>) -> Option<usize> {
        visits.max_by(|a, b| self.walk.visits[*a].rank.cmp(&self.walk.visits[*b].rank))
    }

    /// Where `author`, the admin key of `device`, yields to `handover`: as a contestant, at the
    /// best entry of the instance its own other candidate opened, or, as the anchor of the
    /// contested instance, at its best entry past an objection to `handover`.
    pub(super) fn yield_point(
        &self,
        author: &AdminPublicKey,
        device: &str,
        handover: &Digest32,
    ) -> Result<ContestPoint, ContestRejection> {
        let candidate = self
            .candidate(handover)
            .ok_or(ContestRejection::UnknownCandidate)?;
        let instance = self.walk.visits[self.walk.parent(candidate)].instance;
        if !matches!(self.outcomes.get(&instance), Some(Outcome::FrozenAt(_))) {
            return Err(ContestRejection::NotFrozen);
        }
        if self
            .walk
            .yields()
            .iter()
            .any(|(key, named)| key == author && named == handover)
        {
            return Err(ContestRejection::AlreadyYielded);
        }
        let own: Vec<usize> = self
            .candidates
            .get(&instance)
            .into_iter()
            .flatten()
            .copied()
            .filter(|other| {
                let visit = &self.walk.visits[*other];
                *other != candidate
                    && visit.author == Some(*author)
                    && visit
                        .handover
                        .as_ref()
                        .is_some_and(|(_, target)| target == device)
            })
            .collect();
        let point = if own.is_empty() {
            let objections = self.walk.objections(candidate);
            if objections.is_empty()
                || self.walk.visits[candidate].parent_anchor_key != Some(*author)
                || self.instance_anchor(instance) != device
            {
                return Err(ContestRejection::NotContestant);
            }
            self.best_of((0..self.walk.visits.len()).filter(|visit| {
                self.walk.visits[*visit].instance == instance
                    && objections
                        .iter()
                        .any(|objection| self.walk.is_ancestor(*objection, *visit))
            }))
        } else {
            self.best_of(
                (0..self.walk.visits.len())
                    .filter(|visit| own.contains(&self.walk.visits[*visit].instance)),
            )
        };
        let point = point.ok_or(ContestRejection::NotContestant)?;
        Ok(ContestPoint {
            state: self.walk.selection(point).state,
            entry: Entry::Yield(Yield {
                handover_hash: *handover,
            }),
        })
    }

    /// Where `author`, the admin key of `device`, objects to `handover`: the best entry of the
    /// contested instance past the handover's parent, with a removal of its target.
    pub(super) fn objection_point(
        &self,
        author: &AdminPublicKey,
        device: &str,
        handover: &Digest32,
    ) -> Result<ContestPoint, ContestRejection> {
        let candidate = self
            .candidate(handover)
            .ok_or(ContestRejection::NotCandidate)?;
        let visit = &self.walk.visits[candidate];
        let parent = self.walk.parent(candidate);
        let instance = self.walk.visits[parent].instance;
        if visit.parent_anchor_key != Some(*author) || self.instance_anchor(instance) != device {
            return Err(ContestRejection::NotAnchor);
        }
        if self.pinned.contains_key(&instance)
            || self
                .walk
                .yields()
                .iter()
                .any(|(key, named)| key == author && named == handover)
        {
            return Err(ContestRejection::Resolved);
        }
        if !self.walk.objections(candidate).is_empty() {
            return Err(ContestRejection::AlreadyObjected);
        }
        let target = visit
            .handover
            .as_ref()
            .map(|(_, target)| target.clone())
            .ok_or(ContestRejection::NotCandidate)?;
        let point = self
            .best_of((0..self.walk.visits.len()).filter(|at| {
                self.walk.visits[*at].instance == instance && self.walk.is_ancestor(parent, *at)
            }))
            .unwrap_or(parent);
        Ok(ContestPoint {
            state: self.walk.selection(point).state,
            entry: Entry::Remove(Remove { device_id: target }),
        })
    }
}

/// Which contest record to seal.
enum Action {
    Yield,
    Objection {
        discovery_key: [u8; DISCOVERY_KEY_BYTES],
    },
}

impl Engine {
    /// Seals a yield of `admin` to the stored handover `handover`, see [`Resolution`]: the
    /// local device must be the target of another handover out of the same frozen instance that
    /// `admin` signed, or the anchor of that instance with an objection to `handover`. The
    /// yield continues the best entry of the instance the local handover opened, or of the
    /// contested instance past the objection, never the selected head. Returns the revision of
    /// the yield, or why it was not sealed.
    pub async fn seal_yield(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        admin: &AdminKeyPair,
        handover: &Digest32,
        writer: &mut LocalWriter<'_>,
        now_ms: u64,
    ) -> SyncResult<Result<u64, ContestRejection>> {
        self.seal_contest(conn, ctx, admin, handover, Action::Yield, writer, now_ms)
            .await
    }

    /// Seals an objection of `admin` to the stored handover `handover`: a removal of its
    /// target, which only the anchor of the instance the handover leaves may seal, at its best
    /// entry of that instance past the handover's parent. The removal takes `discovery_key`.
    /// Returns the revision of the removal, or why it was not sealed.
    #[allow(clippy::too_many_arguments)]
    pub async fn seal_objection(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        admin: &AdminKeyPair,
        handover: &Digest32,
        discovery_key: [u8; DISCOVERY_KEY_BYTES],
        writer: &mut LocalWriter<'_>,
        now_ms: u64,
    ) -> SyncResult<Result<u64, ContestRejection>> {
        self.seal_contest(
            conn,
            ctx,
            admin,
            handover,
            Action::Objection { discovery_key },
            writer,
            now_ms,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn seal_contest(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        admin: &AdminKeyPair,
        handover: &Digest32,
        action: Action,
        writer: &mut LocalWriter<'_>,
        now_ms: u64,
    ) -> SyncResult<Result<u64, ContestRejection>> {
        let mut tx = conn.begin().await?;
        log::ensure_space(&mut tx, ctx.space).await?;
        let Some(stored) = Stored::load(&mut tx, ctx.space).await? else {
            return Err(SyncError::ResetRequired("the space has no charter"));
        };
        let author = admin.public_key();
        let device = writer.certificate.certificate.device_id.clone();
        let resolution = Resolution::new(&stored);
        let point = match action {
            Action::Yield => resolution.yield_point(&author, &device, handover),
            Action::Objection { .. } => resolution.objection_point(&author, &device, handover),
        };
        let ContestPoint { state, entry } = match point {
            Ok(point) => point,
            Err(rejection) => return Ok(Err(rejection)),
        };
        let sequencing = match action {
            Action::Yield => Sequencing::Empty,
            Action::Objection { discovery_key } => {
                let Entry::Remove(remove) = &entry else {
                    return Ok(Err(ContestRejection::Rejected(
                        ProposalRejection::UnsupportedEntry,
                    )));
                };
                let Some(cutoffs) = removal_cutoffs(
                    &mut tx,
                    ctx.space,
                    &remove.device_id,
                    state.catch_up.as_deref(),
                    &VersionVector::new(),
                )
                .await?
                else {
                    return Ok(Err(ContestRejection::Rejected(
                        ProposalRejection::CatchingUp,
                    )));
                };
                Sequencing::Removal {
                    cutoffs,
                    discovery_key,
                }
            }
        };
        let proposal = match contest_proposal(ctx, state.charter, admin, entry, now_ms) {
            Ok(proposal) => proposal,
            Err(rejection) => return Ok(Err(rejection)),
        };
        let record = AuthorizationRecord {
            epoch: state.epoch,
            revision: state.revision + 1,
            previous_entry_hash: state.entry_hash,
            anchor_device_id: state.anchor_device_id.clone(),
            proposal,
            sequencing,
        };
        if record.validate().is_err() {
            return Ok(Err(ContestRejection::Rejected(
                ProposalRejection::Malformed,
            )));
        }
        if let Err(rejection) = state.check(&record) {
            return Ok(Err(ContestRejection::Rejected(rejection)));
        }
        let revision = record.revision;
        self.seal_record(&mut tx, ctx, state.charter, record, writer, now_ms)
            .await?;
        failpoint!("contest.before_commit");
        sql::set_applying(&mut tx, false).await?;
        tx.commit().await?;
        Ok(Ok(revision))
    }
}

/// Signs `entry` as a proposal of `admin` for the space founded by `charter`.
fn contest_proposal(
    ctx: &SpaceContext,
    charter: Digest32,
    admin: &AdminKeyPair,
    entry: Entry,
    now_ms: u64,
) -> Result<SignedProposal, ContestRejection> {
    let created_at_ms = i64::try_from(now_ms)
        .map_err(|_| ContestRejection::Rejected(ProposalRejection::Malformed))?;
    sign_proposal(
        &Proposal {
            space: ctx.space,
            charter,
            author: admin.public_key(),
            created_at_ms,
            entry,
        },
        admin,
    )
    .map_err(|_| ContestRejection::Rejected(ProposalRejection::Malformed))
}
