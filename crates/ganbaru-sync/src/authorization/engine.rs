//! The engine entry points of the authorization log: installing the charter, reading the
//! selected state, and the anchor sequencing proposals, handovers, and the key epochs that
//! removals and recovery changes wait for.

use ganbaru_sync_contracts::bounds::DISCOVERY_KEY_BYTES;
use ganbaru_sync_contracts::{
    AdminPublicKey, AuthorizationRecord, ChainLink, Digest32, Entry, KeyEnvelope, KeyEpoch,
    Proposal, ProposalRejection, Seq, Sequencing, SignedCharter, SignedProposal, SpaceId, SpaceKey,
    VersionVector, WriterId, sign_device_proposal,
};
use sqlx::{Connection, SqliteConnection};

use crate::apply::{self, Constrained};
use crate::error::{SyncError, SyncResult, corrupt, failpoint};
use crate::log::{self, stored_u64};
use crate::merge::Batch;
use crate::seal::{SealReport, SealSlot};
use crate::{Engine, LocalWriter, SpaceContext, SpaceKeyHolder, exclusion, sql};

#[cfg(doc)]
use super::ContestAction;
use super::selection::{handover_point, kit_point, select_for_sequencing};
use super::{
    AuthState, AuthorizationView, ContestEntry, EpochKeys, HandoverPoint, Signer, StoredSpaceKey,
    load_charter, rederive, refuse_legacy, select, stored_space_keys, write_space_keys,
};

/// Opens the key of `state`'s key epoch with `holder` from any envelope of that epoch in
/// `key_epochs`, the chain `state` heads: `None` before the first key epoch, and
/// [`ProposalRejection::KeyUnavailable`] when the holder opens none, as for an anchor that a
/// handover or the kit made before its own envelope reached it and that holds no recovery key.
fn open_current(
    state: &AuthState,
    key_epochs: &[EpochKeys],
    holder: Option<&dyn SpaceKeyHolder>,
) -> Result<Option<SpaceKey>, ProposalRejection> {
    if state.key_epoch == 0 {
        return Ok(None);
    }
    let holder = holder.ok_or(ProposalRejection::KeyUnavailable)?;
    key_epochs
        .iter()
        .rev()
        .find(|keys| keys.epoch == state.key_epoch)
        .and_then(|keys| {
            keys.envelopes
                .iter()
                .find_map(|envelope| holder.open(&state.space, state.key_epoch, envelope))
        })
        .map(Some)
        .ok_or(ProposalRejection::KeyUnavailable)
}

/// The key epoch entry that completes the rotation `state` waits for, sealed by the anchor
/// with a fresh space key: one envelope to each of [`AuthState::rotation_recipients`], the
/// previous key `current` wrapped under the new one, and the new relay access key. Refuses with
/// [`ProposalRejection::KeyUnavailable`] unless the holder is the anchor's device and holds the
/// encryption key the anchor published, so the anchor can always open what it seals, and
/// unless it opened the current key, so the new epoch never breaks the chain.
fn rotation_record(
    state: &AuthState,
    current: Option<&SpaceKey>,
    holder: Option<&dyn SpaceKeyHolder>,
    now_ms: u64,
) -> SyncResult<Result<(AuthorizationRecord, SpaceKey), ProposalRejection>> {
    let Some(pending) = &state.pending else {
        return Err(corrupt("a key epoch was sealed without a pending rotation"));
    };
    let Some(holder) = holder else {
        return Ok(Err(ProposalRejection::KeyUnavailable));
    };
    let device_key = holder.device_key().public_key();
    let held = state
        .devices
        .get(&state.anchor_device_id)
        .is_some_and(|anchor| {
            anchor.device_key == device_key
                && anchor.encryption_key == Some(holder.encryption_key())
        });
    if !held || (state.key_epoch > 0) != current.is_some() {
        return Ok(Err(ProposalRejection::KeyUnavailable));
    }
    let Some(epoch) = state.key_epoch.checked_add(1) else {
        return Ok(Err(ProposalRejection::Malformed));
    };
    let next = SpaceKey::generate(epoch).map_err(SyncError::Crypto)?;
    let wrapped_previous = current
        .map(|previous| ChainLink::seal(&state.space, &next, previous))
        .transpose()
        .map_err(SyncError::Crypto)?;
    let envelopes = state
        .rotation_recipients(pending)
        .values()
        .map(|recipient| KeyEnvelope::seal(&state.space, &next, recipient))
        .collect::<Result<Vec<_>, _>>()
        .map_err(SyncError::Crypto)?;
    let relay_access_key = next
        .relay_access_public_key(&state.space)
        .map_err(SyncError::Crypto)?;
    let proposal = Proposal {
        space: state.space,
        charter: state.charter,
        author: AdminPublicKey::from_bytes(*device_key.as_bytes()),
        created_at_ms: i64::try_from(now_ms).unwrap_or(i64::MAX),
        entry: Entry::KeyEpoch(KeyEpoch {
            key_epoch: epoch,
            wrapped_previous,
            envelopes,
            relay_access_key,
        }),
    };
    let signed = sign_device_proposal(&proposal, holder.device_key())
        .map_err(|error| corrupt(format!("key epoch does not sign: {error}")))?;
    let record = AuthorizationRecord {
        epoch: state.epoch,
        revision: state.revision + 1,
        previous_entry_hash: state.entry_hash,
        anchor_device_id: state.anchor_device_id.clone(),
        proposal: signed,
        sequencing: Sequencing::Empty,
    };
    Ok(Ok((record, next)))
}

/// Appends the key epoch that the last of `records`, which `state` admits as its parent, makes
/// pending, so the removal or recovery change and its key epoch seal as consecutive revisions
/// in one transaction. `current` is a key epoch already opened or sealed for `state`.
pub(super) fn follow_with_rotation(
    state: &AuthState,
    records: &mut Vec<AuthorizationRecord>,
    key_epochs: &[EpochKeys],
    current: Option<SpaceKey>,
    holder: Option<&dyn SpaceKeyHolder>,
    now_ms: u64,
) -> SyncResult<Result<(), ProposalRejection>> {
    let Some(last) = records.last() else {
        return Ok(Ok(()));
    };
    let mut next = state.clone();
    next.advance(last, last.hash());
    if next.pending.is_none() {
        return Ok(Ok(()));
    }
    let current = match current {
        Some(key) => Some(key),
        None => match open_current(&next, key_epochs, holder) {
            Ok(key) => key,
            Err(rejection) => return Ok(Err(rejection)),
        },
    };
    let (rotation, _) = match rotation_record(&next, current.as_ref(), holder, now_ms)? {
        Ok(sealed) => sealed,
        Err(rejection) => return Ok(Err(rejection)),
    };
    if let Err(rejection) = next.check(&rotation) {
        return Ok(Err(rejection));
    }
    records.push(rotation);
    Ok(Ok(()))
}

/// The revision at which the selected chain holds a proposal.
async fn selected_proposal_revision(
    conn: &mut SqliteConnection,
    space: SpaceId,
    proposal_hash: &Digest32,
) -> SyncResult<Option<u64>> {
    let revision: Option<i64> = sqlx::query_scalar(
        "SELECT revision FROM sync_auth_selected WHERE space_id = ? AND proposal_hash = ?",
    )
    .bind(space.as_bytes().as_slice())
    .bind(proposal_hash.as_slice())
    .fetch_optional(&mut *conn)
    .await?;
    revision
        .map(|revision| stored_u64(revision, "revision"))
        .transpose()
}

async fn selected_proposal_position(
    conn: &mut SqliteConnection,
    space: SpaceId,
    proposal_hash: &Digest32,
) -> SyncResult<Option<(u32, u64)>> {
    let row: Option<(i64, i64)> = sqlx::query_as(
        "SELECT epoch, revision FROM sync_auth_selected WHERE space_id = ? AND proposal_hash = ?",
    )
    .bind(space.as_bytes().as_slice())
    .bind(proposal_hash.as_slice())
    .fetch_optional(&mut *conn)
    .await?;
    row.map(|(epoch, revision)| {
        let epoch =
            u32::try_from(epoch).map_err(|_| corrupt("authorization epoch is out of range"))?;
        Ok((epoch, stored_u64(revision, "revision")?))
    })
    .transpose()
}

impl Engine {
    /// Installs the space charter, revision 0 of the authorization log, after verifying it with
    /// the context's person key. Installing the same charter again does nothing; a different one
    /// is refused.
    pub async fn install_charter(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        charter: &SignedCharter,
    ) -> SyncResult<()> {
        if charter.charter.space != ctx.space || charter.verify(&ctx.person_key).is_err() {
            return Err(SyncError::InvalidCharter);
        }
        let mut tx = conn.begin().await?;
        log::ensure_space(&mut tx, ctx.space).await?;
        match load_charter(&mut tx, ctx.space).await? {
            Some(existing) if existing == *charter => return Ok(()),
            Some(_) => return Err(SyncError::InvalidCharter),
            None => {}
        }
        sqlx::query("UPDATE sync_spaces SET charter = ? WHERE space_id = ?")
            .bind(charter.encode())
            .bind(ctx.space.as_bytes().as_slice())
            .execute(&mut *tx)
            .await?;
        refuse_legacy(&mut tx, ctx.space).await?;
        rederive(&mut tx, ctx.space).await?;
        tx.commit().await?;
        Ok(())
    }

    /// The installed charter of the space.
    pub async fn charter(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
    ) -> SyncResult<Option<SignedCharter>> {
        log::ensure_space(conn, ctx.space).await?;
        load_charter(conn, ctx.space).await
    }

    /// The selected authorization state, or `None` without a charter.
    pub async fn authorization(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
    ) -> SyncResult<Option<AuthorizationView>> {
        log::ensure_space(conn, ctx.space).await?;
        Ok(select(conn, ctx.space)
            .await?
            .map(|selection| selection.view()))
    }

    /// The key epochs of the selected chain for the local device, in epoch order, after
    /// rebuilding `sync_space_keys` from the stored record set, so a snapshot copy, which
    /// carries none, gets them back at service start. The service opens the newest envelope it
    /// holds and walks the chain links back from it.
    pub async fn space_keys(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
    ) -> SyncResult<Vec<StoredSpaceKey>> {
        let mut tx = conn.begin().await?;
        log::ensure_space(&mut tx, ctx.space).await?;
        if let Some(selection) = select(&mut tx, ctx.space).await? {
            write_space_keys(&mut tx, ctx.space, &selection).await?;
        }
        let keys = stored_space_keys(&mut tx, ctx.space).await?;
        tx.commit().await?;
        Ok(keys)
    }

    /// The revision at which the selected chain holds the proposal with `proposal_hash`, if it
    /// does. An outbox drops a proposal once its own replica holds it.
    pub async fn proposal_revision(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        proposal_hash: &Digest32,
    ) -> SyncResult<Option<u64>> {
        log::ensure_space(conn, ctx.space).await?;
        selected_proposal_revision(conn, ctx.space, proposal_hash).await
    }

    /// The epoch and revision at which the selected chain holds the proposal with
    /// `proposal_hash`, if it does. An outbox keeps a held proposal until a later epoch kept it,
    /// because a handover from an earlier revision would supersede it.
    pub async fn proposal_position(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        proposal_hash: &Digest32,
    ) -> SyncResult<Option<(u32, u64)>> {
        log::ensure_space(conn, ctx.space).await?;
        selected_proposal_position(conn, ctx.space, proposal_hash).await
    }

    /// Fails with [`SyncError::ResetRequired`] when the space holds sync state from before the
    /// authorization log, which this build does not read.
    pub async fn refuse_legacy_state(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
    ) -> SyncResult<()> {
        log::ensure_space(conn, ctx.space).await?;
        refuse_legacy(conn, ctx.space).await
    }

    /// Orders an admin-signed proposal as the next revision and seals it in the local writer's
    /// chain. The local device must be the anchor of the selected chain. Returns the revision, or
    /// why the proposal was rejected; a proposal already in the selected chain returns its
    /// revision without sealing again. A removal takes `discovery_key` as the rotated key.
    ///
    /// This sequences `enroll`, `remove`, `set_role`, and `resolve_late`; handovers go through
    /// [`Self::seal_handover`] and unknown kinds are rejected as unsupported.
    pub async fn sequence_proposal(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        proposal: &SignedProposal,
        discovery_key: [u8; DISCOVERY_KEY_BYTES],
        writer: &mut LocalWriter<'_>,
        now_ms: u64,
    ) -> SyncResult<Result<u64, ProposalRejection>> {
        self.sequence_proposal_covering(
            conn,
            ctx,
            proposal,
            discovery_key,
            &VersionVector::new(),
            writer,
            now_ms,
        )
        .await
    }

    /// [`Self::sequence_proposal`] on an anchor that may store less than other replicas, such
    /// as a linked anchor that quarantined a device: a removal's cutoff of each writer is at
    /// least what `covered` holds, the stored vector the coordinator last reported, so
    /// operations the coordinator and members already applied do not become late changes.
    #[allow(clippy::too_many_arguments)]
    pub async fn sequence_proposal_covering(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        proposal: &SignedProposal,
        discovery_key: [u8; DISCOVERY_KEY_BYTES],
        covered: &VersionVector,
        writer: &mut LocalWriter<'_>,
        now_ms: u64,
    ) -> SyncResult<Result<u64, ProposalRejection>> {
        self.sequence(
            conn,
            ctx,
            proposal,
            Ordering::Anchor {
                discovery_key,
                covered,
            },
            writer,
            now_ms,
        )
        .await
    }

    /// Seals the key epoch the selected head waits for when the local writer is its anchor, as
    /// after a handover that continued past a removal or recovery change. Sequencing seals it
    /// too before the next proposal; this seals it without one. Returns its revision, `None`
    /// when no rotation is pending, or why the anchor cannot seal it, such as
    /// [`ProposalRejection::KeyUnavailable`] while it opens no envelope of the current epoch.
    pub async fn complete_rotation(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        writer: &mut LocalWriter<'_>,
        now_ms: u64,
    ) -> SyncResult<Result<Option<u64>, ProposalRejection>> {
        let mut tx = conn.begin().await?;
        log::ensure_space(&mut tx, ctx.space).await?;
        let Some(selection) = select(&mut tx, ctx.space).await? else {
            return Err(SyncError::ResetRequired("the space has no charter"));
        };
        let state = &selection.state;
        if state.pending.is_none() {
            return Ok(Ok(None));
        }
        if writer.certificate.certificate.device_id != state.anchor_device_id {
            return Ok(Err(ProposalRejection::NotAnchor));
        }
        if selection.handed_over {
            return Ok(Err(ProposalRejection::HandoverConflict));
        }
        let current = match open_current(state, &selection.key_epochs, writer.keys) {
            Ok(key) => key,
            Err(rejection) => return Ok(Err(rejection)),
        };
        let (record, _) = match rotation_record(state, current.as_ref(), writer.keys, now_ms)? {
            Ok(sealed) => sealed,
            Err(rejection) => return Ok(Err(rejection)),
        };
        if let Err(rejection) = state.check(&record) {
            return Ok(Err(rejection));
        }
        let revision = record.revision;
        self.seal_records(&mut tx, ctx, state.charter, vec![record], writer, now_ms)
            .await?;
        failpoint!("complete_rotation.before_commit");
        sql::set_applying(&mut tx, false).await?;
        tx.commit().await?;
        Ok(Ok(Some(revision)))
    }

    /// Where a handover that `signer` signs for the desktop `target` continues from: the
    /// selected head when `signer` is its kit key and `target` a live desktop there, or, while
    /// the head's instance is frozen, the best such entry on a branch past the freeze, so a kit
    /// can recover from a stolen anchor that removed its target; else the selected head. `None`
    /// without a charter.
    pub async fn handover_point(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        signer: &AdminPublicKey,
        target: &str,
    ) -> SyncResult<Option<HandoverPoint>> {
        log::ensure_space(conn, ctx.space).await?;
        handover_point(conn, ctx.space, signer, target).await
    }

    /// The kit point at `entry` for a handover the kit key `signer` signs for the desktop
    /// `target`, when `entry` is one of the kit points, see [`ContestAction::UseKit`]. `None`
    /// without a charter or when `entry` is not a kit point.
    pub async fn kit_handover_point(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        signer: &AdminPublicKey,
        target: &str,
        entry: &ContestEntry,
    ) -> SyncResult<Option<HandoverPoint>> {
        log::ensure_space(conn, ctx.space).await?;
        Ok(
            kit_point(conn, ctx.space, signer, target, entry.revision, &entry.hash)
                .await?
                .map(|state| HandoverPoint::of(&state, true)),
        )
    }

    /// Seals an admin-signed handover as the first entry of the epoch it opens, in the local
    /// writer's chain. The local device must be the handover's new anchor and the selected head
    /// the revision the handover continues from, or, for the kit key, one of its kit points,
    /// see [`Self::handover_point`]; `old_anchor_vector` is the old anchor's stored vector when
    /// known. A frozen epoch does not stop a handover, but a target refuses a take out of an
    /// instance that another handover already leaves. Returns the revision, or why the handover
    /// was rejected; one already in the selected chain returns its revision without sealing
    /// again.
    pub async fn seal_handover(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        proposal: &SignedProposal,
        old_anchor_vector: Option<VersionVector>,
        writer: &mut LocalWriter<'_>,
        now_ms: u64,
    ) -> SyncResult<Result<u64, ProposalRejection>> {
        self.sequence(
            conn,
            ctx,
            proposal,
            Ordering::Handover { old_anchor_vector },
            writer,
            now_ms,
        )
        .await
    }

    async fn sequence(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        proposal: &SignedProposal,
        ordering: Ordering<'_>,
        writer: &mut LocalWriter<'_>,
        now_ms: u64,
    ) -> SyncResult<Result<u64, ProposalRejection>> {
        let mut tx = conn.begin().await?;
        log::ensure_space(&mut tx, ctx.space).await?;
        if let Some(revision) =
            selected_proposal_revision(&mut tx, ctx.space, &proposal.hash()).await?
        {
            return Ok(Ok(revision));
        }
        let anchor_ordering = matches!(ordering, Ordering::Anchor { .. });
        let Some((selection, reversed)) =
            select_for_sequencing(&mut tx, ctx.space, anchor_ordering.then_some(proposal)).await?
        else {
            return Err(SyncError::ResetRequired("the space has no charter"));
        };
        let handed_over = selection.handed_over;
        let key_epochs = selection.key_epochs;
        let mut state = selection.state;
        let holder = writer.keys;
        let local_device = &writer.certificate.certificate.device_id;
        // Replaying one entry of a stored branch that lost must not restore what a later entry
        // of that branch reversed, whether or not the reversal reached this chain.
        if anchor_ordering && *local_device == state.anchor_device_id && !handed_over && reversed {
            return Ok(Err(ProposalRejection::Malformed));
        }
        // Records sealed before the proposal's: the key epoch a handover left pending.
        let mut records = Vec::new();
        // The key of the current key epoch, once opened or sealed here.
        let mut current: Option<SpaceKey> = None;
        if anchor_ordering && *local_device == state.anchor_device_id && !handed_over {
            if matches!(proposal.proposal.entry, Entry::KeyEpoch(_)) {
                return Ok(Err(ProposalRejection::AnchorOnly));
            }
            // The anchor's own device key goes first, and the key epoch follows it.
            if state.pending.is_some() && !state.admits_while_pending(&proposal.proposal.entry) {
                let opened = match open_current(&state, &key_epochs, holder) {
                    Ok(key) => key,
                    Err(rejection) => return Ok(Err(rejection)),
                };
                let (rotation, next) =
                    match rotation_record(&state, opened.as_ref(), holder, now_ms)? {
                        Ok(sealed) => sealed,
                        Err(rejection) => return Ok(Err(rejection)),
                    };
                if let Err(rejection) = state.check(&rotation) {
                    return Ok(Err(rejection));
                }
                state.advance(&rotation, rotation.hash());
                records.push(rotation);
                current = Some(next);
            }
        }
        let record = match (ordering, &proposal.proposal.entry) {
            (Ordering::Anchor { .. }, _) if *local_device != state.anchor_device_id => {
                return Ok(Err(ProposalRejection::NotAnchor));
            }
            // An anchor that stores a handover out of its instance stops sequencing there.
            (Ordering::Anchor { .. }, _) if handed_over => {
                return Ok(Err(ProposalRejection::HandoverConflict));
            }
            (
                Ordering::Anchor {
                    discovery_key,
                    covered,
                },
                entry,
            ) => {
                let sequencing = match entry {
                    Entry::Enroll(_)
                    | Entry::SetRole(_)
                    | Entry::ResolveLate(_)
                    | Entry::Yield(_) => Sequencing::Empty,
                    Entry::Remove(remove) => {
                        let Some(cutoffs) = removal_cutoffs(
                            &mut tx,
                            ctx.space,
                            &remove.device_id,
                            state.catch_up.as_deref(),
                            covered,
                        )
                        .await?
                        else {
                            return Ok(Err(ProposalRejection::CatchingUp));
                        };
                        Sequencing::Removal {
                            cutoffs,
                            discovery_key,
                        }
                    }
                    Entry::RecoveryKey(_) | Entry::ServerConfig(_) => Sequencing::Empty,
                    // The anchor seals the current key to a device's new encryption key.
                    Entry::DeviceKey(device_key) => {
                        if current.is_none() {
                            current = match open_current(&state, &key_epochs, holder) {
                                Ok(key) => key,
                                Err(rejection) => return Ok(Err(rejection)),
                            };
                        }
                        let envelope = current
                            .as_ref()
                            .map(|key| {
                                KeyEnvelope::seal(&state.space, key, &device_key.encryption_key)
                            })
                            .transpose()
                            .map_err(SyncError::Crypto)?;
                        Sequencing::DeviceKey { envelope }
                    }
                    Entry::KeyEpoch(_) => return Ok(Err(ProposalRejection::AnchorOnly)),
                    Entry::Handover(_) | Entry::Unknown { .. } => {
                        return Ok(Err(ProposalRejection::UnsupportedEntry));
                    }
                };
                AuthorizationRecord {
                    epoch: state.epoch,
                    revision: state.revision + 1,
                    previous_entry_hash: state.entry_hash,
                    anchor_device_id: state.anchor_device_id.clone(),
                    proposal: proposal.clone(),
                    sequencing,
                }
            }
            (Ordering::Handover { old_anchor_vector }, Entry::Handover(handover)) => {
                if handover.new_anchor_device_id != *local_device {
                    return Ok(Err(ProposalRejection::NotAnchor));
                }
                if handover.continues_from_revision != state.revision
                    || handover.continues_from_hash != state.entry_hash
                {
                    // The kit key may continue from a branch past a freeze, such as one before
                    // a stolen anchor removed its target.
                    let point = kit_point(
                        &mut tx,
                        ctx.space,
                        &proposal.proposal.author,
                        &handover.new_anchor_device_id,
                        handover.continues_from_revision,
                        &handover.continues_from_hash,
                    )
                    .await?;
                    let Some(point) = point else {
                        return Ok(Err(ProposalRejection::HandoverConflict));
                    };
                    state = point;
                }
                AuthorizationRecord {
                    epoch: handover.epoch,
                    revision: state.revision + 1,
                    previous_entry_hash: state.entry_hash,
                    anchor_device_id: handover.new_anchor_device_id.clone(),
                    proposal: proposal.clone(),
                    sequencing: Sequencing::Handover { old_anchor_vector },
                }
            }
            (Ordering::Handover { .. }, _) => {
                return Ok(Err(ProposalRejection::UnsupportedEntry));
            }
        };
        if record.validate().is_err() {
            return Ok(Err(ProposalRejection::Malformed));
        }
        // A take out of an instance that another handover already leaves would only contest it;
        // the kit alone continues there.
        if handed_over
            && state
                .signer(&record)
                .is_some_and(|signer| signer != Signer::Kit)
        {
            return Ok(Err(ProposalRejection::HandoverConflict));
        }
        if let Err(rejection) = state.check(&record) {
            return Ok(Err(rejection));
        }
        let revision = record.revision;
        records.push(record);
        if anchor_ordering
            && let Err(rejection) =
                follow_with_rotation(&state, &mut records, &key_epochs, current, holder, now_ms)?
        {
            return Ok(Err(rejection));
        }
        self.seal_records(&mut tx, ctx, state.charter, records, writer, now_ms)
            .await?;
        failpoint!("sequence.before_commit");
        sql::set_applying(&mut tx, false).await?;
        tx.commit().await?;
        Ok(Ok(revision))
    }

    /// Seals `records`, consecutive revisions that the rules at each one's parent admit, as the
    /// next operations of the local writer, rederives the selection, and applies what the sealed
    /// operations make ready. The reservation records the first, so a rolled back seal of the
    /// pair stays ahead by one. The caller commits.
    pub(super) async fn seal_records(
        &self,
        tx: &mut SqliteConnection,
        ctx: &SpaceContext,
        charter: Digest32,
        records: Vec<AuthorizationRecord>,
        writer: &mut LocalWriter<'_>,
        now_ms: u64,
    ) -> SyncResult<()> {
        let Some(first) = records.first() else {
            return Ok(());
        };
        let reserved = (first.epoch, first.revision, first.hash());
        sql::begin_engine_write(&mut *tx).await?;
        let mut batch = Batch::load(&mut *tx, ctx.space).await?;
        exclusion::note_local_device(
            self,
            &mut *tx,
            ctx,
            &mut batch,
            &writer.certificate.certificate.device_id,
        )
        .await?;
        let mut slot = SealSlot::new(Some(writer));
        let mut report = SealReport::default();
        let (sealer, writer) = slot
            .open(self, &mut *tx, ctx, &mut batch, now_ms)
            .await?
            .ok_or_else(|| corrupt("sealer slot has no writer"))?;
        writer
            .reservation
            .reserve_entry(charter, reserved.0, reserved.1, reserved.2)
            .map_err(SyncError::Reservation)?;
        for record in records {
            sealer
                .seal_authorization(
                    self,
                    &mut *tx,
                    ctx,
                    &mut batch,
                    writer,
                    record,
                    now_ms,
                    &mut report,
                )
                .await?;
            failpoint!("seal_records.between");
        }
        if let Some(selection) = rederive(&mut *tx, ctx.space).await? {
            exclusion::settle(self, &mut *tx, ctx, &mut batch, &selection).await?;
        }
        apply::finish_sealed(
            self,
            &mut *tx,
            ctx,
            &mut batch,
            &mut slot,
            Constrained::Seal,
            now_ms,
            &mut report,
        )
        .await?;
        Ok(())
    }
}

/// How far past what the anchor stored a removal cutoff may follow the covered vector the
/// coordinator reports: operations other replicas stored that have not reached the anchor yet.
pub(crate) const MAX_COVERED_AHEAD: Seq = 1024;

/// How a proposal is ordered into the log.
enum Ordering<'a> {
    /// By the selected anchor, in its epoch; a removal takes the rotated discovery key, and its
    /// cutoffs cover at least `covered`.
    Anchor {
        discovery_key: [u8; DISCOVERY_KEY_BYTES],
        covered: &'a VersionVector,
    },
    /// By the new anchor a handover names, as the first entry of the next epoch.
    Handover {
        old_anchor_vector: Option<VersionVector>,
    },
}

/// Cutoffs of a removal of `device_id`: the stored sequence of each of its writers. In an epoch
/// a handover opened with the old anchor's stored vector, a writer's cutoff is at least what the
/// old anchor stored, because those operations are final, and it is at least what `covered`
/// holds, which other replicas already store. `None` while a writer of the old anchor vector is
/// unknown here: it may belong to the removed device, whose cutoff could not be set. A writer
/// `covered` names but this replica never stored has no row and gets no cutoff. `covered` comes
/// from the coordinator, so it raises a cutoff at most [`MAX_COVERED_AHEAD`] past what this
/// replica stored or the old anchor finalized: a coordinator cannot keep a removal from
/// excluding anything by reporting an unbounded vector.
pub(super) async fn removal_cutoffs(
    conn: &mut SqliteConnection,
    space: SpaceId,
    device_id: &str,
    catch_up: Option<&VersionVector>,
    covered: &VersionVector,
) -> SyncResult<Option<Vec<(WriterId, Seq)>>> {
    if let Some(old_anchor) = catch_up {
        let rows: Vec<Vec<u8>> =
            sqlx::query_scalar("SELECT writer_id FROM sync_writers WHERE space_id = ?")
                .bind(space.as_bytes().as_slice())
                .fetch_all(&mut *conn)
                .await?;
        let mut writers = std::collections::BTreeSet::new();
        for writer in &rows {
            writers.insert(log::writer_id(writer)?);
        }
        if old_anchor
            .iter()
            .any(|(writer, seq)| seq > 0 && !writers.contains(&writer))
        {
            return Ok(None);
        }
    }
    let rows: Vec<(Vec<u8>, i64)> = sqlx::query_as(
        "SELECT writer_id, stored_seq FROM sync_writers
         WHERE space_id = ? AND device_id = ? ORDER BY writer_id",
    )
    .bind(space.as_bytes().as_slice())
    .bind(device_id)
    .fetch_all(&mut *conn)
    .await?;
    rows.iter()
        .map(|(writer, seq)| {
            let writer = log::writer_id(writer)?;
            let stored = stored_u64(*seq, "stored sequence")?;
            let finalized = catch_up.map_or(0, |old_anchor| old_anchor.get(&writer));
            let known = stored.max(finalized);
            let reported = covered
                .get(&writer)
                .min(known.saturating_add(MAX_COVERED_AHEAD));
            Ok((writer, known.max(reported)))
        })
        .collect::<SyncResult<_>>()
        .map(Some)
}
