//! Storing remote operations: authentication, chain continuity, and hold classification.
//!
//! Storing never touches domain tables. An operation is stored when it is signed by a known
//! writer and continues that writer's chain; whether it can be applied is decided separately, so
//! an operation this version cannot read is kept, forwarded to peers, and held.

use std::collections::{BTreeMap, BTreeSet};

use ganbaru_sync_contracts::bounds::{MAX_WRITERS_PER_DEVICE, MAX_WRITERS_PER_SPACE};
use ganbaru_sync_contracts::op::FORMAT_VERSION;
use ganbaru_sync_contracts::{
    Content, Envelope, Hlc, Operation, OperationError, OperationKind, PayloadError, Seq,
    SignedCertificate, VersionVector, WriterId,
};
use sqlx::{Connection, SqliteConnection};

use crate::authorization::{self, Enrollment};
use crate::error::{SyncResult, corrupt, failpoint};
use crate::exclusion;
use crate::log::{self, WriterRow, WriterState, clamp_ms, seq_i64};
use crate::{Engine, SpaceContext, fork, records, validate};

/// Why a stored operation cannot be applied yet or ever.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HoldReason {
    /// The envelope format or operation kind is newer than this version reads.
    NewerFormat,
    /// The changes use a newer manifest version.
    NewerManifest,
    /// The signed content breaks the operation or manifest rules.
    Invalid,
}

impl HoldReason {
    /// Stored name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NewerFormat => "newer_format",
            Self::NewerManifest => "newer_manifest",
            Self::Invalid => "invalid",
        }
    }

    pub(crate) fn parse(text: &str) -> SyncResult<Self> {
        match text {
            "newer_format" => Ok(Self::NewerFormat),
            "newer_manifest" => Ok(Self::NewerManifest),
            "invalid" => Ok(Self::Invalid),
            _ => Err(corrupt(format!("unknown hold reason {text}"))),
        }
    }
}

/// Result of storing one operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreOutcome {
    /// The operation was stored, held when it cannot be applied.
    Stored {
        /// Hold reason, when held.
        held: Option<HoldReason>,
    },
    /// The same operation is already stored.
    Duplicate,
    /// The operation was not stored.
    Refused(StoreRefusal),
}

/// Why an operation was not stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreRefusal {
    /// The bytes are not an operation envelope, or the unsigned body does not match the header.
    Malformed,
    /// The operation belongs to another space.
    WrongSpace,
    /// The writer is unknown and the operation is not its genesis.
    UnknownWriter,
    /// The genesis of an unknown writer uses a newer format, so its certificate cannot be read.
    NewerFormatGenesis,
    /// The writer key did not sign the operation.
    BadSignature,
    /// The genesis certificate names an enrolled device but not its enrolled device key.
    Untrusted,
    /// The genesis names a device the selected authorization chain has not enrolled, or the
    /// space has no charter yet. The writer may be accepted once its enrollment arrives.
    Unenrolled,
    /// A different operation is stored at `seq` in the writer's chain.
    Fork {
        /// First sequence where the stored chain and the offered chain differ.
        seq: Seq,
    },
    /// The writer was forked or re-sealed and the operation is above its cutoff, its device was
    /// removed and the operation is more than
    /// [`MAX_EXCLUDED_PER_WRITER`](crate::exclusion::MAX_EXCLUDED_PER_WRITER) past the removal's
    /// cutoff (an authorization entry, up to
    /// [`MAX_EXCLUDED_ENTRIES_PER_WRITER`](crate::exclusion::MAX_EXCLUDED_ENTRIES_PER_WRITER)
    /// further, contiguous from it), or the writer is one the removal does not name outside the
    /// [`MAX_UNNAMED_WRITERS_PER_REMOVED_DEVICE`](crate::exclusion::MAX_UNNAMED_WRITERS_PER_REMOVED_DEVICE)
    /// lowest writer ids of its device: its genesis while that many lower ones are stored, and
    /// its later operations other than contiguous entries once lower ones displaced it.
    Revoked,
    /// The genesis would exceed the writers its device or the space may have.
    WriterLimit,
    /// Earlier operations of the writer are missing.
    Gap {
        /// Next sequence the writer's chain needs.
        expected: Seq,
    },
    /// This device quarantined the device that certified the writer. Nothing is excluded: the
    /// operation is stored once the quarantine is lifted, and dependents wait for it.
    Quarantined,
}

/// How a stored operation enters the log.
#[derive(Debug)]
pub(crate) enum Classified {
    /// Waiting to be applied.
    Waiting {
        clock: Hlc,
        dependencies: Vec<(WriterId, Seq)>,
        /// Authorization revision the operation was sealed against.
        revision: u64,
        /// Whether the writer's next operation belongs to the same apply group.
        continues: bool,
    },
    /// Held, with the signed header fields when they could be read.
    Held {
        reason: HoldReason,
        header: Option<(Hlc, Vec<(WriterId, Seq)>)>,
    },
    /// The unsigned body does not match the signed header.
    Malformed,
}

impl Classified {
    /// Clock and dependencies, when known.
    pub(crate) fn header(&self) -> Option<(Hlc, &[(WriterId, Seq)])> {
        match self {
            Self::Waiting {
                clock,
                dependencies,
                ..
            } => Some((*clock, dependencies)),
            Self::Held {
                header: Some((clock, dependencies)),
                ..
            } => Some((*clock, dependencies)),
            Self::Held { header: None, .. } | Self::Malformed => None,
        }
    }

    /// Authorization revision of a waiting operation; 0 for one that is held or malformed.
    pub(crate) fn revision(&self) -> u64 {
        match self {
            Self::Waiting { revision, .. } => *revision,
            Self::Held { .. } | Self::Malformed => 0,
        }
    }

    /// Whether a waiting operation's apply group continues; false for one that is held or
    /// malformed, which blocks its writer's chain anyway.
    pub(crate) fn continues(&self) -> bool {
        match self {
            Self::Waiting { continues, .. } => *continues,
            Self::Held { .. } | Self::Malformed => false,
        }
    }

    pub(crate) fn hold(&self) -> Option<HoldReason> {
        match self {
            Self::Held { reason, .. } => Some(*reason),
            Self::Waiting { .. } | Self::Malformed => None,
        }
    }
}

/// Clocks are stored as SQLite integers; a signed clock above that range cannot be ordered.
fn storable_clock(clock: Hlc) -> bool {
    i64::try_from(clock.as_u64()).is_ok()
}

/// Classifies an authenticated envelope against this version's format and manifest.
pub(crate) fn classify(engine: &Engine, envelope: &Envelope<'_>) -> Classified {
    let operation = match envelope.operation() {
        Ok(operation) => operation,
        Err(PayloadError::NewerFormat | PayloadError::UnknownKind) => {
            return Classified::Held {
                reason: HoldReason::NewerFormat,
                header: None,
            };
        }
        Err(PayloadError::Invalid(_)) => {
            return Classified::Held {
                reason: HoldReason::Invalid,
                header: None,
            };
        }
        Err(PayloadError::Body(_)) => return Classified::Malformed,
    };
    classify_operation(engine, operation)
}

fn classify_operation(engine: &Engine, operation: Operation) -> Classified {
    let clock = operation.header.clock;
    if !storable_clock(clock) {
        return Classified::Held {
            reason: HoldReason::Invalid,
            header: None,
        };
    }
    let reason = match &operation.content {
        Content::Changes(changes) => {
            let version = operation.header.manifest_version;
            if version == 0 {
                Some(HoldReason::Invalid)
            } else if version > u32::from(engine.manifest().version) {
                Some(HoldReason::NewerManifest)
            } else if changes
                .iter()
                .any(|change| validate::change(engine, change).is_err())
            {
                Some(HoldReason::Invalid)
            } else {
                None
            }
        }
        Content::Genesis(_) | Content::Acknowledgement | Content::Authorization(_) => None,
    };
    let revision = operation.header.authorization_revision;
    let continues = operation.header.group_continues;
    let dependencies = operation.header.dependencies;
    match reason {
        Some(reason) => Classified::Held {
            reason,
            header: Some((clock, dependencies)),
        },
        None => Classified::Waiting {
            clock,
            dependencies,
            revision,
            continues,
        },
    }
}

/// The context of a newly stored operation, when its predecessor's context and its own
/// dependencies are known.
pub(crate) async fn derive_context(
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    writer: WriterId,
    seq: Seq,
    dependencies: Option<&[(WriterId, Seq)]>,
) -> SyncResult<Option<VersionVector>> {
    let Some(dependencies) = dependencies else {
        return Ok(None);
    };
    if seq == 1 {
        return Ok(Some(VersionVector::new()));
    }
    let previous = log::load_context(conn, ctx.space, writer, seq - 1)
        .await?
        .ok_or_else(|| corrupt("previous operation of a stored chain is missing"))?;
    Ok(previous.map(|previous| log::next_context(&previous, writer, seq, dependencies)))
}

/// Signed header fields a stored operation copies into its log row.
struct StoredHeader {
    clock: Hlc,
    revision: u64,
    continues: bool,
}

#[allow(clippy::too_many_arguments)]
async fn insert_op(
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    envelope: &Envelope<'_>,
    bytes: &[u8],
    header: StoredHeader,
    context: Option<&VersionVector>,
    held: Option<HoldReason>,
    excluded: Option<u64>,
    now_ms: u64,
) -> SyncResult<()> {
    let StoredHeader {
        clock,
        revision,
        continues,
    } = header;
    let state = if held.is_some() { "held" } else { "waiting" };
    let order = log::next_stored_order(conn, &ctx.space).await?;
    sqlx::query(
        "INSERT INTO sync_ops
            (space_id, writer_id, seq, kind, hash, clock, authorization_revision, context,
             envelope, state, hold_reason, stored_at_ms, stored_order, excluded_revision,
             group_continues)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(ctx.space.as_bytes().as_slice())
    .bind(envelope.writer.as_bytes().as_slice())
    .bind(seq_i64(envelope.seq)?)
    .bind(i64::from(envelope.kind))
    .bind(envelope.hash().as_slice())
    .bind(clamp_ms(clock.as_u64()))
    .bind(seq_i64(revision)?)
    .bind(log::encode_context(context))
    .bind(bytes)
    .bind(state)
    .bind(held.map(HoldReason::as_str))
    .bind(clamp_ms(now_ms))
    .bind(order)
    .bind(excluded.map(seq_i64).transpose()?)
    .bind(continues)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

async fn store_genesis(
    engine: &Engine,
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    envelope: &Envelope<'_>,
    bytes: &[u8],
    quarantined: &BTreeSet<String>,
    now_ms: u64,
) -> SyncResult<StoreOutcome> {
    if envelope.seq != 1 {
        return Ok(StoreOutcome::Refused(StoreRefusal::UnknownWriter));
    }
    if envelope.format_version > FORMAT_VERSION {
        return Ok(StoreOutcome::Refused(StoreRefusal::NewerFormatGenesis));
    }
    let (operation, certificate) = match envelope.genesis() {
        Ok(genesis) => genesis,
        Err(PayloadError::Invalid(OperationError::Signature)) => {
            return Ok(StoreOutcome::Refused(StoreRefusal::BadSignature));
        }
        Err(_) => return Ok(StoreOutcome::Refused(StoreRefusal::Malformed)),
    };
    let cert = &certificate.certificate;
    if quarantined.contains(&cert.device_id) {
        return Ok(StoreOutcome::Refused(StoreRefusal::Quarantined));
    }
    // A writer of a removed device that this replica never stored takes the cutoff the removal
    // lists for it, 0 when the removal does not name it. Its excluded operations are stored up
    // to the bound, so the late changes other replicas applied before the removal are derived
    // here too, but only the few unnamed writers per removed device with the lowest ids keep
    // operations past their genesis.
    let enrollment =
        authorization::enrollment(conn, ctx.space, &cert.device_id, &cert.device_key).await?;
    let selected = !matches!(enrollment, Enrollment::Absent);
    let removal = match enrollment {
        Enrollment::Enrolled => None,
        Enrollment::WrongKey => return Ok(StoreOutcome::Refused(StoreRefusal::Untrusted)),
        Enrollment::Removed => Some((
            exclusion::removal_revision(conn, ctx.space, &cert.device_id).await?,
            authorization::removal_cutoff(conn, ctx.space, &cert.device_id, envelope.writer)
                .await?,
        )),
        // A chain this replica did not select may enroll the device, and its entries can be the
        // evidence that vetoes the selected one. The writer's operations still apply only once
        // the selected chain enrolls the device, and until then they are bounded as excluded
        // ones are and the writer counts toward the unselected writers of the space.
        Enrollment::Absent
            if authorization::enrolled_on_any_chain(
                conn,
                &engine.enrolled,
                ctx.space,
                &cert.device_id,
                &cert.device_key,
            )
            .await? =>
        {
            None
        }
        Enrollment::Absent => return Ok(StoreOutcome::Refused(StoreRefusal::Unenrolled)),
    };
    if matches!(removal, Some((_, 0)))
        && highest_kept_unnamed(conn, ctx, &cert.device_id)
            .await?
            .is_some_and(|highest| envelope.writer.as_bytes().as_slice() > highest.as_slice())
    {
        return Ok(StoreOutcome::Refused(StoreRefusal::Revoked));
    }
    let clock = operation.header.clock;
    if !storable_clock(clock) {
        return Ok(StoreOutcome::Refused(StoreRefusal::Malformed));
    }
    if !within_writer_limits(conn, ctx, &cert.device_id, selected).await? {
        return Ok(StoreOutcome::Refused(StoreRefusal::WriterLimit));
    }
    sqlx::query(
        "INSERT INTO sync_writers
            (space_id, writer_id, public_key, device_id, certificate, state, predecessor,
             cutoff_seq, stored_seq, applied_seq, head_hash, head_clock, created_at_ms,
             removal_cutoff_seq)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, 1, 0, ?, ?, ?, ?)",
    )
    .bind(ctx.space.as_bytes().as_slice())
    .bind(envelope.writer.as_bytes().as_slice())
    .bind(cert.writer_key.as_bytes().as_slice())
    .bind(cert.device_id.as_str())
    .bind(certificate.encode())
    .bind(if removal.is_some() {
        "revoked"
    } else {
        "active"
    })
    .bind(
        cert.predecessor
            .map(|predecessor| predecessor.as_bytes().to_vec()),
    )
    .bind(removal.map(|(_, cutoff)| seq_i64(cutoff)).transpose()?)
    .bind(envelope.hash().as_slice())
    .bind(clamp_ms(clock.as_u64()))
    .bind(cert.created_at_ms.max(0))
    .bind(removal.map(|(_, cutoff)| seq_i64(cutoff)).transpose()?)
    .execute(&mut *conn)
    .await?;
    // The genesis is past only a cutoff of 0, and the removed device never excludes its own.
    let excluded = match removal {
        Some((revision, 0))
            if exclusion::local_device(conn, ctx.space).await?.as_deref()
                != Some(cert.device_id.as_str()) =>
        {
            Some(revision)
        }
        _ => None,
    };
    let held = excluded.map(|_| HoldReason::Invalid);
    insert_op(
        conn,
        ctx,
        envelope,
        bytes,
        StoredHeader {
            clock,
            revision: operation.header.authorization_revision,
            continues: false,
        },
        Some(&VersionVector::new()),
        held,
        excluded,
        now_ms,
    )
    .await?;
    Ok(StoreOutcome::Stored { held })
}

async fn store_next(
    engine: &Engine,
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    writer: &WriterRow,
    envelope: &Envelope<'_>,
    bytes: &[u8],
    now_ms: u64,
) -> SyncResult<StoreOutcome> {
    if !envelope.verify(&writer.public_key) {
        return Ok(StoreOutcome::Refused(StoreRefusal::BadSignature));
    }
    if envelope.seq <= writer.stored {
        let stored: Option<(Vec<u8>, Vec<u8>)> = sqlx::query_as(
            "SELECT hash, envelope FROM sync_ops WHERE space_id = ? AND writer_id = ? AND seq = ?",
        )
        .bind(ctx.space.as_bytes().as_slice())
        .bind(writer.id.as_bytes().as_slice())
        .bind(seq_i64(envelope.seq)?)
        .fetch_optional(&mut *conn)
        .await?;
        return Ok(match stored {
            Some((hash, stored)) if hash != envelope.hash() => {
                if !stored.is_empty() {
                    records::admit_fork(
                        conn,
                        ctx.space,
                        writer.id,
                        envelope.seq,
                        [&stored, bytes],
                        now_ms,
                    )
                    .await?;
                }
                // Both operations are signed by the writer key: the pair is fork evidence,
                // unless compaction dropped the stored body or the pair proves no revision.
                if !stored.is_empty()
                    && let Ok(previous) = Envelope::decode(&stored)
                    && fork::unprovable(
                        conn,
                        ctx.space,
                        writer.id,
                        envelope.seq,
                        [&previous, envelope],
                    )
                    .await?
                    .is_none()
                {
                    fork::record(
                        conn,
                        ctx.space,
                        writer.id,
                        envelope.seq,
                        [(&stored, log::digest(&hash)?), (bytes, envelope.hash())],
                        now_ms,
                    )
                    .await?;
                }
                StoreOutcome::Refused(StoreRefusal::Fork { seq: envelope.seq })
            }
            _ => StoreOutcome::Duplicate,
        });
    }
    // A removed device's writer is revoked at its removal cutoff, but the operations past it are
    // still stored, up to a bound, so their late changes can be derived and forwarded. Its
    // authorization entries have a bound of their own past that one: on a chain this replica
    // did not select the device may be an anchor whose entries veto the selected chain.
    // A surplus unnamed writer keeps only its genesis and the entries contiguous from it.
    let bounds = match writer.removal_cutoff {
        Some(0) if exclusion::is_surplus_unnamed(conn, ctx.space, writer.id).await? => {
            Some((exclusion::SURPLUS_LIMIT, exclusion::SURPLUS_ENTRY_LIMIT))
        }
        Some(cutoff) => Some((
            exclusion::excluded_limit(cutoff),
            exclusion::entry_limit(cutoff),
        )),
        // A writer whose device the selected chain does not name with the key of its certificate,
        // which a chain it did not select enrolls, is bounded as an unnamed writer of a removed
        // device is.
        None if envelope.seq > exclusion::excluded_limit(0)
            && !names_writer_device(conn, ctx, writer.id).await? =>
        {
            Some((exclusion::excluded_limit(0), exclusion::entry_limit(0)))
        }
        None => None,
    };
    let beyond_removal = bounds.is_some_and(|(limit, entry_limit)| {
        envelope.seq > limit
            && (envelope.kind != OperationKind::Authorization.as_u8() || envelope.seq > entry_limit)
    });
    let beyond_cutoff = writer.state != WriterState::Revoked
        && writer.cutoff.is_some_and(|cutoff| envelope.seq > cutoff);
    if beyond_removal || beyond_cutoff {
        return Ok(StoreOutcome::Refused(StoreRefusal::Revoked));
    }
    if envelope.seq > writer.stored + 1 {
        return Ok(StoreOutcome::Refused(StoreRefusal::Gap {
            expected: writer.stored + 1,
        }));
    }
    if envelope.previous_hash != writer.head_hash {
        return Ok(StoreOutcome::Refused(StoreRefusal::Fork {
            seq: writer.stored,
        }));
    }
    let classified = classify(engine, envelope);
    if matches!(classified, Classified::Malformed) {
        return Ok(StoreOutcome::Refused(StoreRefusal::Malformed));
    }
    let header = classified.header();
    let context = derive_context(
        conn,
        ctx,
        writer.id,
        envelope.seq,
        header.map(|(_, dependencies)| dependencies),
    )
    .await?;
    let clock = header.map_or(Hlc::ZERO, |(clock, _)| clock);
    let mut held = classified.hold();
    // The removed device itself never excludes its own operations.
    let excluded = match writer.removal_cutoff {
        Some(cutoff)
            if envelope.seq > cutoff
                && exclusion::local_device(conn, ctx.space).await?.as_deref()
                    != Some(writer.device_id.as_str()) =>
        {
            let removal = exclusion::removal_revision(conn, ctx.space, &writer.device_id).await?;
            if let Ok(operation) = envelope.operation() {
                exclusion::fold(
                    engine,
                    conn,
                    removal,
                    writer.id,
                    envelope.seq,
                    clock,
                    &operation.content,
                )
                .await?;
                exclusion::mark_stale(engine, conn, &operation.content).await?;
                // An earlier anchor may have stored the operation, which released what it
                // displaced, so the anchor seals the groups it writes again.
                if fork::reseals_excluded(conn, ctx.space).await? {
                    let mut groups = BTreeMap::new();
                    fork::written_groups(engine, &operation.content, &mut groups);
                    fork::capture_groups(engine, conn, groups, now_ms).await?;
                }
            }
            held = Some(HoldReason::Invalid);
            Some(removal)
        }
        _ => None,
    };
    insert_op(
        conn,
        ctx,
        envelope,
        bytes,
        StoredHeader {
            clock,
            revision: classified.revision(),
            continues: classified.continues(),
        },
        context.as_ref(),
        held,
        excluded,
        now_ms,
    )
    .await?;
    sqlx::query(
        "UPDATE sync_writers SET stored_seq = ?, head_hash = ?, head_clock = max(head_clock, ?)
         WHERE space_id = ? AND writer_id = ?",
    )
    .bind(seq_i64(envelope.seq)?)
    .bind(envelope.hash().as_slice())
    .bind(clamp_ms(clock.as_u64()))
    .bind(ctx.space.as_bytes().as_slice())
    .bind(writer.id.as_bytes().as_slice())
    .execute(&mut *conn)
    .await?;
    Ok(StoreOutcome::Stored { held })
}

async fn store_one(
    engine: &Engine,
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    bytes: &[u8],
    quarantined: &BTreeSet<String>,
    now_ms: u64,
) -> SyncResult<StoreOutcome> {
    let Ok(envelope) = Envelope::decode(bytes) else {
        return Ok(StoreOutcome::Refused(StoreRefusal::Malformed));
    };
    if envelope.space != ctx.space {
        return Ok(StoreOutcome::Refused(StoreRefusal::WrongSpace));
    }
    match log::writer(conn, ctx.space, envelope.writer).await? {
        None => store_genesis(engine, conn, ctx, &envelope, bytes, quarantined, now_ms).await,
        Some(writer) if quarantined.contains(&writer.device_id) => {
            Ok(StoreOutcome::Refused(StoreRefusal::Quarantined))
        }
        Some(writer) => store_next(engine, conn, ctx, &writer, &envelope, bytes, now_ms).await,
    }
}

impl Engine {
    /// Stores one remote operation. Stored operations are applied by [`Engine::apply`].
    pub async fn store(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        bytes: &[u8],
        now_ms: u64,
    ) -> SyncResult<StoreOutcome> {
        let mut outcomes = self
            .store_many(conn, ctx, std::slice::from_ref(&bytes.to_vec()), now_ms)
            .await?;
        outcomes
            .pop()
            .ok_or_else(|| corrupt("store produced no outcome"))
    }

    /// Stores operations in one transaction, in order. Storing stops after the first fork so
    /// the caller can act on the evidence; the result then has fewer outcomes than inputs.
    pub async fn store_many(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        operations: &[Vec<u8>],
        now_ms: u64,
    ) -> SyncResult<Vec<StoreOutcome>> {
        self.store_many_quarantined(conn, ctx, operations, &BTreeSet::new(), now_ms)
            .await
    }

    /// Like [`Self::store_many`], refusing operations of writers certified by a device in
    /// `quarantined` with [`StoreRefusal::Quarantined`], which does not stop the page.
    pub async fn store_many_quarantined(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        operations: &[Vec<u8>],
        quarantined: &BTreeSet<String>,
        now_ms: u64,
    ) -> SyncResult<Vec<StoreOutcome>> {
        let mut tx = conn.begin().await?;
        log::ensure_space(&mut tx, ctx.space).await?;
        let mut outcomes = Vec::with_capacity(operations.len());
        // Writers refused as quarantined in this page. A refused genesis stores no writer row,
        // so the writer's later operations would otherwise read as unknown.
        let mut refused_writers = BTreeSet::new();
        for bytes in operations {
            // The record store keeps every certificate and record that passes its intrinsic
            // checks, whatever the slot and transport rules below refuse.
            records::admit(&mut tx, ctx.space, bytes, now_ms).await?;
            let writer = Envelope::decode(bytes).ok().map(|envelope| envelope.writer);
            if writer.is_some_and(|writer| refused_writers.contains(&writer)) {
                outcomes.push(StoreOutcome::Refused(StoreRefusal::Quarantined));
                continue;
            }
            let outcome = store_one(self, &mut tx, ctx, bytes, quarantined, now_ms).await?;
            if let (StoreOutcome::Refused(StoreRefusal::Quarantined), Some(writer)) =
                (outcome, writer)
            {
                refused_writers.insert(writer);
            }
            outcomes.push(outcome);
            if matches!(outcome, StoreOutcome::Refused(StoreRefusal::Fork { .. })) {
                break;
            }
        }
        failpoint!("store.before_commit");
        tx.commit().await?;
        Ok(outcomes)
    }
}

/// The highest writer id among the stored writers of the removed device `device_id` its removal
/// does not name that keep excluded operations, once there are
/// [`MAX_UNNAMED_WRITERS_PER_REMOVED_DEVICE`](exclusion::MAX_UNNAMED_WRITERS_PER_REMOVED_DEVICE)
/// of them, `None` while there are fewer.
async fn highest_kept_unnamed(
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    device_id: &str,
) -> SyncResult<Option<Vec<u8>>> {
    Ok(sqlx::query_scalar(
        "SELECT writer_id FROM sync_writers
         WHERE space_id = ? AND device_id = ? AND removal_cutoff_seq = 0
         ORDER BY writer_id LIMIT 1 OFFSET ?",
    )
    .bind(ctx.space.as_bytes().as_slice())
    .bind(device_id)
    .bind(exclusion::MAX_UNNAMED_WRITERS_PER_REMOVED_DEVICE - 1)
    .fetch_optional(&mut *conn)
    .await?)
}

/// Most stored writers of devices the selected chain does not enroll, which another stored chain
/// enrolls. They never count toward the writers of devices the selected chain names, which may
/// fill [`MAX_WRITERS_PER_SPACE`] less this many, so the space never stores more than it.
pub(crate) const MAX_UNSELECTED_WRITERS_PER_SPACE: usize = 2 * MAX_WRITERS_PER_DEVICE;

/// Whether the space can admit one more writer of `device_id` within
/// [`MAX_WRITERS_PER_DEVICE`], and within [`MAX_UNSELECTED_WRITERS_PER_SPACE`] when `selected` is
/// false, or the rest of [`MAX_WRITERS_PER_SPACE`] when it is true: whether the selected chain
/// names the device.
async fn within_writer_limits(
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    device_id: &str,
    selected: bool,
) -> SyncResult<bool> {
    let (device, total, named): (i64, i64, i64) = sqlx::query_as(
        "SELECT COALESCE(SUM(w.device_id = ?1), 0), COUNT(*), COALESCE(SUM(EXISTS (
            SELECT 1 FROM sync_devices d
            WHERE d.space_id = w.space_id AND d.device_id = w.device_id
         )), 0)
         FROM sync_writers w WHERE w.space_id = ?2",
    )
    .bind(device_id)
    .bind(ctx.space.as_bytes().as_slice())
    .fetch_one(&mut *conn)
    .await?;
    let below = |count: i64, limit: usize| usize::try_from(count).is_ok_and(|count| count < limit);
    let space = if selected {
        below(
            named,
            MAX_WRITERS_PER_SPACE - MAX_UNSELECTED_WRITERS_PER_SPACE,
        )
    } else {
        below(total - named, MAX_UNSELECTED_WRITERS_PER_SPACE)
    };
    Ok(below(device, MAX_WRITERS_PER_DEVICE) && space)
}

/// Whether the selected chain names the device of `writer`, enrolled or removed, with the device
/// key its certificate carries. Another chain may enroll the same device id with another key.
async fn names_writer_device(
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    writer: WriterId,
) -> SyncResult<bool> {
    let row: Option<(Vec<u8>, Vec<u8>)> = sqlx::query_as(
        "SELECT d.device_key, w.certificate
         FROM sync_writers w
         JOIN sync_devices d ON d.space_id = w.space_id AND d.device_id = w.device_id
         WHERE w.space_id = ? AND w.writer_id = ?",
    )
    .bind(ctx.space.as_bytes().as_slice())
    .bind(writer.as_bytes().as_slice())
    .fetch_optional(&mut *conn)
    .await?;
    let Some((device_key, certificate)) = row else {
        return Ok(false);
    };
    let certificate = SignedCertificate::decode(&certificate)
        .map_err(|_| corrupt("a stored writer certificate does not decode"))?;
    Ok(certificate.certificate.device_key.as_bytes().as_slice() == device_key.as_slice())
}
