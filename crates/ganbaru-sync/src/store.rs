//! Storing remote operations: authentication, chain continuity, and hold classification.
//!
//! Storing never touches domain tables. An operation is stored when it is signed by a known
//! writer and continues that writer's chain; whether it can be applied is decided separately, so
//! an operation this version cannot read is kept, forwarded to peers, and held.

use ganbaru_sync_contracts::op::FORMAT_VERSION;
use ganbaru_sync_contracts::{
    Content, Envelope, Hlc, Operation, OperationError, PayloadError, Seq, VersionVector, WriterId,
};
use sqlx::{Connection, SqliteConnection};

use crate::error::{SyncResult, corrupt, failpoint};
use crate::log::{self, WriterRow, clamp_ms, seq_i64};
use crate::{Engine, SpaceContext, validate};

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
    /// The writer is not certified by the space anchor.
    Untrusted,
    /// A different operation is stored at `seq` in the writer's chain.
    Fork {
        /// First sequence where the stored chain and the offered chain differ.
        seq: Seq,
    },
    /// The writer was revoked, forked, or re-sealed and the operation is above its cutoff.
    Revoked,
    /// Earlier operations of the writer are missing.
    Gap {
        /// Next sequence the writer's chain needs.
        expected: Seq,
    },
}

/// How a stored operation enters the log.
#[derive(Debug)]
pub(crate) enum Classified {
    /// Waiting to be applied.
    Waiting {
        clock: Hlc,
        dependencies: Vec<(WriterId, Seq)>,
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
            } => Some((*clock, dependencies)),
            Self::Held {
                header: Some((clock, dependencies)),
                ..
            } => Some((*clock, dependencies)),
            Self::Held { header: None, .. } | Self::Malformed => None,
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
        Content::Genesis(_) | Content::Revoke(_) => None,
    };
    let dependencies = operation.header.dependencies;
    match reason {
        Some(reason) => Classified::Held {
            reason,
            header: Some((clock, dependencies)),
        },
        None => Classified::Waiting {
            clock,
            dependencies,
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

#[allow(clippy::too_many_arguments)]
async fn insert_op(
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    envelope: &Envelope<'_>,
    bytes: &[u8],
    clock: Hlc,
    context: Option<&VersionVector>,
    held: Option<HoldReason>,
    now_ms: u64,
) -> SyncResult<()> {
    let state = if held.is_some() { "held" } else { "waiting" };
    sqlx::query(
        "INSERT INTO sync_ops
            (space_id, writer_id, seq, kind, hash, clock, context, envelope, state, hold_reason,
             stored_at_ms)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(ctx.space.as_bytes().as_slice())
    .bind(envelope.writer.as_bytes().as_slice())
    .bind(seq_i64(envelope.seq)?)
    .bind(i64::from(envelope.kind))
    .bind(envelope.hash().as_slice())
    .bind(clamp_ms(clock.as_u64()))
    .bind(log::encode_context(context))
    .bind(bytes)
    .bind(state)
    .bind(held.map(HoldReason::as_str))
    .bind(clamp_ms(now_ms))
    .execute(&mut *conn)
    .await?;
    Ok(())
}

async fn store_genesis(
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    envelope: &Envelope<'_>,
    bytes: &[u8],
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
    if certificate.certificate.person_key != ctx.anchor {
        return Ok(StoreOutcome::Refused(StoreRefusal::Untrusted));
    }
    let clock = operation.header.clock;
    if !storable_clock(clock) {
        return Ok(StoreOutcome::Refused(StoreRefusal::Malformed));
    }
    let cert = &certificate.certificate;
    sqlx::query(
        "INSERT INTO sync_writers
            (space_id, writer_id, public_key, device_id, certificate, state, predecessor,
             cutoff_seq, stored_seq, applied_seq, head_hash, head_clock, created_at_ms)
         VALUES (?, ?, ?, ?, ?, 'active', ?, NULL, 1, 0, ?, ?, ?)",
    )
    .bind(ctx.space.as_bytes().as_slice())
    .bind(envelope.writer.as_bytes().as_slice())
    .bind(cert.writer_key.as_bytes().as_slice())
    .bind(cert.device_id.as_str())
    .bind(certificate.encode())
    .bind(
        cert.predecessor
            .map(|predecessor| predecessor.as_bytes().to_vec()),
    )
    .bind(envelope.hash().as_slice())
    .bind(clamp_ms(clock.as_u64()))
    .bind(cert.created_at_ms.max(0))
    .execute(&mut *conn)
    .await?;
    insert_op(
        conn,
        ctx,
        envelope,
        bytes,
        clock,
        Some(&VersionVector::new()),
        None,
        now_ms,
    )
    .await?;
    Ok(StoreOutcome::Stored { held: None })
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
        let stored: Option<Vec<u8>> = sqlx::query_scalar(
            "SELECT hash FROM sync_ops WHERE space_id = ? AND writer_id = ? AND seq = ?",
        )
        .bind(ctx.space.as_bytes().as_slice())
        .bind(writer.id.as_bytes().as_slice())
        .bind(seq_i64(envelope.seq)?)
        .fetch_optional(&mut *conn)
        .await?;
        return Ok(match stored {
            Some(hash) if hash != envelope.hash() => {
                StoreOutcome::Refused(StoreRefusal::Fork { seq: envelope.seq })
            }
            _ => StoreOutcome::Duplicate,
        });
    }
    if writer.cutoff.is_some_and(|cutoff| envelope.seq > cutoff) {
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
    let held = classified.hold();
    insert_op(
        conn,
        ctx,
        envelope,
        bytes,
        clock,
        context.as_ref(),
        held,
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
    now_ms: u64,
) -> SyncResult<StoreOutcome> {
    let Ok(envelope) = Envelope::decode(bytes) else {
        return Ok(StoreOutcome::Refused(StoreRefusal::Malformed));
    };
    if envelope.space != ctx.space {
        return Ok(StoreOutcome::Refused(StoreRefusal::WrongSpace));
    }
    match log::writer(conn, ctx.space, envelope.writer).await? {
        None => store_genesis(conn, ctx, &envelope, bytes, now_ms).await,
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
        let mut tx = conn.begin().await?;
        log::ensure_space(&mut tx, ctx.space).await?;
        let mut outcomes = Vec::with_capacity(operations.len());
        for bytes in operations {
            let outcome = store_one(self, &mut tx, ctx, bytes, now_ms).await?;
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
