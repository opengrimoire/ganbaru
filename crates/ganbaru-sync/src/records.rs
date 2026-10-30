//! The authorization record store: every authorization record envelope, writer certificate, and
//! fork evidence envelope this replica stored, keyed by envelope hash.
//!
//! Every authority rule reads this set and nothing else, so two replicas that store the same
//! envelopes derive the same tree, whatever order the envelopes arrived in, which peer delivered
//! them, and which operation slots the replica kept. Envelopes are discarded only for failures
//! intrinsic to them: a decode failure, a wrong content kind, an entry hash that does not
//! recompute, or a signature that the writer key does not verify. An envelope whose writer has
//! no stored certificate yet is pending: stored, but read by no rule until a certificate arrives,
//! which verifies or purges every pending envelope of that writer in the same transaction. A
//! purged hash stays requested from peers until a verified copy is stored, since an honest copy
//! of the same hash may have been dropped while the purged one was held.
//!
//! The walk reads nodes: record bodies, deduplicated by entry hash, that some stored envelope
//! carries at a sequence below its writer's horizon (the lowest sequence at which two distinct
//! envelopes of the writer are stored), and their ancestors, once each one's chain to the charter
//! is stored completely. Fork evidence is provable when two envelopes of a writer at one sequence
//! continue from one stored envelope and neither claims a revision below it.

use std::collections::{BTreeMap, BTreeSet};

use ganbaru_sync_contracts::{
    AuthorizationRecord, Content, DevicePublicKey, Digest32, Envelope, OperationKind, Seq, SpaceId,
    WriterId, WriterPublicKey,
};
use sqlx::{Row, SqliteConnection};

use crate::error::{SyncResult, corrupt};
use crate::log::{self, clamp_ms, seq_i64, stored_u64};

/// What a stored envelope is to the record store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    /// A kind 4 authorization record.
    Record,
    /// A writer certificate, carried by its genesis operation.
    Certificate,
    /// Any other envelope stored as fork evidence or as the prefix a fork continues from.
    Evidence,
}

impl Role {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Record => "record",
            Self::Certificate => "certificate",
            Self::Evidence => "evidence",
        }
    }
}

/// One envelope that carries a node's record body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Carrier {
    /// Writer that sealed the envelope.
    pub(crate) writer: WriterId,
    /// Its sequence.
    pub(crate) seq: Seq,
    /// Its envelope hash.
    pub(crate) envelope_hash: Digest32,
}

/// A record body the walk reads, with every stored envelope that carries it.
#[derive(Debug, Clone)]
pub(crate) struct Node {
    /// Entry hash of the record.
    pub(crate) hash: Digest32,
    /// The record.
    pub(crate) record: AuthorizationRecord,
    /// Verified envelopes that carry the record, those below their writer's horizon first, then by
    /// envelope hash, lowest first.
    pub(crate) carriers: Vec<Carrier>,
}

/// What the stored envelope set derives for the walk.
#[derive(Debug, Default)]
pub(crate) struct Loaded {
    /// Every node, by revision and entry hash.
    pub(crate) nodes: Vec<Node>,
    /// The device id and device key each verified certificate of a writer names.
    pub(crate) certificates: BTreeMap<WriterId, Vec<(String, DevicePublicKey)>>,
    /// Lowest sequence of each writer with provable fork evidence.
    pub(crate) provable: BTreeMap<WriterId, Seq>,
    /// Highest revision of any verified record.
    pub(crate) max_revision: u64,
    /// Verified record bodies that are not nodes, by revision and entry hash: those above every
    /// carrier's horizon or without a complete chain. The walk never reads them.
    pub(crate) others: Vec<Node>,
}

/// What an envelope adds to the store, decided from its bytes alone.
struct Admission {
    role: Role,
    hash: Digest32,
    writer: WriterId,
    seq: Seq,
    previous_hash: Digest32,
    stamp: u64,
    entry: Option<(Digest32, Digest32)>,
    device: Option<(String, DevicePublicKey)>,
    /// The certified writer key, for a certificate.
    writer_key: Option<WriterPublicKey>,
}

/// Admits a writer certificate or an authorization record of `space` to the record store. Other
/// envelopes and envelopes that fail their intrinsic checks are not stored. Returns whether the
/// store grew.
pub(crate) async fn admit(
    conn: &mut SqliteConnection,
    space: SpaceId,
    bytes: &[u8],
    now_ms: u64,
) -> SyncResult<bool> {
    let Ok(envelope) = Envelope::decode(bytes) else {
        return Ok(false);
    };
    if envelope.space != space {
        return Ok(false);
    }
    let Some(admission) = classify(&envelope, false) else {
        return Ok(false);
    };
    store(conn, space, &envelope, admission, bytes, now_ms).await
}

/// Admits envelopes that show a writer fork, and the envelope they continue from, to the record
/// store: records and certificates under their own role, anything else as evidence. Returns
/// whether the store grew.
pub(crate) async fn admit_evidence(
    conn: &mut SqliteConnection,
    space: SpaceId,
    envelopes: &[&[u8]],
    now_ms: u64,
) -> SyncResult<bool> {
    let mut grew = false;
    for bytes in envelopes {
        let Ok(envelope) = Envelope::decode(bytes) else {
            continue;
        };
        if envelope.space != space {
            continue;
        }
        let Some(admission) = classify(&envelope, true) else {
            continue;
        };
        grew |= store(conn, space, &envelope, admission, bytes, now_ms).await?;
    }
    Ok(grew)
}

/// Admits two envelopes that show `writer` forking at `seq`, with the stored operation they both
/// continue from when this replica holds it, see [`admit_evidence`]. Runs before any transport
/// refusal of the pair, so what the store keeps does not depend on slot state. Returns whether
/// the store grew.
pub(crate) async fn admit_fork(
    conn: &mut SqliteConnection,
    space: SpaceId,
    writer: WriterId,
    seq: Seq,
    pair: [&[u8]; 2],
    now_ms: u64,
) -> SyncResult<bool> {
    let prefix: Option<Vec<u8>> =
        match seq.checked_sub(1).filter(|prefix| *prefix >= 1) {
            Some(prefix) => sqlx::query_scalar(
                "SELECT envelope FROM sync_ops WHERE space_id = ? AND writer_id = ? AND seq = ?",
            )
            .bind(space.as_bytes().as_slice())
            .bind(writer.as_bytes().as_slice())
            .bind(seq_i64(prefix)?)
            .fetch_optional(&mut *conn)
            .await?,
            None => None,
        };
    let mut envelopes: Vec<&[u8]> = pair.to_vec();
    if let Some(prefix) = prefix.as_deref() {
        envelopes.push(prefix);
    }
    admit_evidence(conn, space, &envelopes, now_ms).await
}

/// The admission of an envelope, `None` when its intrinsic checks fail or it is not a record or
/// certificate and `evidence` is unset.
fn classify(envelope: &Envelope<'_>, evidence: bool) -> Option<Admission> {
    let base = |role| Admission {
        role,
        hash: envelope.hash(),
        writer: envelope.writer,
        seq: envelope.seq,
        previous_hash: envelope.previous_hash,
        stamp: 0,
        entry: None,
        device: None,
        writer_key: None,
    };
    match OperationKind::from_u8(envelope.kind) {
        Some(OperationKind::Genesis) => {
            let (operation, certificate) = envelope.genesis().ok()?;
            let body = certificate.certificate;
            Some(Admission {
                stamp: operation.header.authorization_revision,
                device: Some((body.device_id, body.device_key)),
                writer_key: Some(body.writer_key),
                ..base(Role::Certificate)
            })
        }
        Some(OperationKind::Authorization) => {
            let operation = envelope.operation().ok()?;
            let Content::Authorization(record) = &operation.content else {
                return None;
            };
            Some(Admission {
                stamp: operation.header.authorization_revision,
                entry: Some((record.hash(), record.previous_entry_hash)),
                ..base(Role::Record)
            })
        }
        _ if evidence => Some(Admission {
            stamp: envelope.operation().ok()?.header.authorization_revision,
            ..base(Role::Evidence)
        }),
        _ => None,
    }
}

/// Stores one admitted envelope, verifying it with a stored certificate of its writer or keeping
/// it pending, and verifies or purges the writer's pending envelopes when it is the writer's
/// first certificate.
async fn store(
    conn: &mut SqliteConnection,
    space: SpaceId,
    envelope: &Envelope<'_>,
    admission: Admission,
    bytes: &[u8],
    now_ms: u64,
) -> SyncResult<bool> {
    let known = writer_key(conn, space, admission.writer).await?;
    let verified = match (&admission.writer_key, &known) {
        (Some(_), _) => true,
        (None, Some(key)) => {
            if !envelope.verify(key) {
                return Ok(false);
            }
            true
        }
        (None, None) => false,
    };
    let (entry_hash, previous_entry_hash) = admission.entry.unzip();
    let (device_id, device_key) = admission.device.unzip();
    let inserted = sqlx::query(
        "INSERT INTO sync_auth_envelopes
            (space_id, role, envelope_hash, writer_id, seq, previous_hash, stamp, entry_hash,
             previous_entry_hash, device_id, device_key, envelope, verified, stored_order,
             stored_at_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
            (SELECT coalesce(max(stored_order), 0) + 1 FROM sync_auth_envelopes
             WHERE space_id = ?1), ?14)
         ON CONFLICT DO NOTHING",
    )
    .bind(space.as_bytes().as_slice())
    .bind(admission.role.as_str())
    .bind(admission.hash.as_slice())
    .bind(admission.writer.as_bytes().as_slice())
    .bind(seq_i64(admission.seq)?)
    .bind(admission.previous_hash.as_slice())
    .bind(seq_i64(admission.stamp)?)
    .bind(entry_hash.as_ref().map(<[u8; 32]>::as_slice))
    .bind(previous_entry_hash.as_ref().map(<[u8; 32]>::as_slice))
    .bind(device_id.as_deref())
    .bind(device_key.as_ref().map(|key| key.as_bytes().as_slice()))
    .bind(bytes)
    .bind(i64::from(verified))
    .bind(clamp_ms(now_ms))
    .execute(&mut *conn)
    .await?
    .rows_affected()
        > 0;
    if inserted && verified {
        sqlx::query("DELETE FROM sync_auth_refetch WHERE space_id = ? AND envelope_hash = ?")
            .bind(space.as_bytes().as_slice())
            .bind(admission.hash.as_slice())
            .execute(&mut *conn)
            .await?;
    }
    if inserted
        && known.is_none()
        && let Some(key) = &admission.writer_key
    {
        settle_pending(conn, space, admission.writer, key).await?;
    }
    Ok(inserted)
}

/// The writer key a stored certificate of `writer` certifies. Every certificate of one writer id
/// names the same key, since the id is the hash of the key.
pub(crate) async fn writer_key(
    conn: &mut SqliteConnection,
    space: SpaceId,
    writer: WriterId,
) -> SyncResult<Option<WriterPublicKey>> {
    let bytes: Option<Vec<u8>> = sqlx::query_scalar(
        "SELECT envelope FROM sync_auth_envelopes
         WHERE space_id = ? AND role = 'certificate' AND writer_id = ? LIMIT 1",
    )
    .bind(space.as_bytes().as_slice())
    .bind(writer.as_bytes().as_slice())
    .fetch_optional(&mut *conn)
    .await?;
    let Some(bytes) = bytes else {
        return Ok(None);
    };
    let (_, certificate) = Envelope::decode(&bytes)
        .ok()
        .and_then(|envelope| envelope.genesis().ok())
        .ok_or_else(|| corrupt("a stored writer certificate does not decode"))?;
    Ok(Some(certificate.certificate.writer_key))
}

/// Verifies every pending envelope of `writer` with its newly stored key and deletes those the
/// key did not sign.
///
/// The envelope hash leaves out the signature, so a pending copy with a bad signature makes the
/// store drop an honest copy of the same hash that arrives while it is held. Arrival order alone
/// would then decide whether the honest envelope is stored, so every purged hash is recorded in
/// `sync_auth_refetch`, which the exchange keeps requesting until a verified copy is stored.
async fn settle_pending(
    conn: &mut SqliteConnection,
    space: SpaceId,
    writer: WriterId,
    key: &WriterPublicKey,
) -> SyncResult<()> {
    let rows: Vec<(String, Vec<u8>, Vec<u8>)> = sqlx::query_as(
        "SELECT role, envelope_hash, envelope FROM sync_auth_envelopes
         WHERE space_id = ? AND writer_id = ? AND verified = 0",
    )
    .bind(space.as_bytes().as_slice())
    .bind(writer.as_bytes().as_slice())
    .fetch_all(&mut *conn)
    .await?;
    for (role, hash, bytes) in rows {
        let signed = Envelope::decode(&bytes).is_ok_and(|envelope| envelope.verify(key));
        let statement = if signed {
            "UPDATE sync_auth_envelopes SET verified = 1
             WHERE space_id = ? AND role = ? AND envelope_hash = ?"
        } else {
            "DELETE FROM sync_auth_envelopes
             WHERE space_id = ? AND role = ? AND envelope_hash = ?"
        };
        sqlx::query(statement)
            .bind(space.as_bytes().as_slice())
            .bind(role)
            .bind(hash.as_slice())
            .execute(&mut *conn)
            .await?;
        if !signed {
            sqlx::query(
                "INSERT OR IGNORE INTO sync_auth_refetch (space_id, envelope_hash) VALUES (?, ?)",
            )
            .bind(space.as_bytes().as_slice())
            .bind(hash.as_slice())
            .execute(&mut *conn)
            .await?;
        }
    }
    prune_evidence(conn, space, writer).await
}

/// Deletes the evidence envelopes of `writer` that no longer show a fork: those without another
/// stored envelope at their sequence and that are not the envelope a stored pair at the next
/// sequence continues from. A purge in [`settle_pending`] can leave such envelopes behind, and
/// each one deleted can orphan the envelope it continues from, so it repeats until none is left.
async fn prune_evidence(
    conn: &mut SqliteConnection,
    space: SpaceId,
    writer: WriterId,
) -> SyncResult<()> {
    loop {
        let orphans: Vec<Vec<u8>> = sqlx::query_scalar(
            "SELECT e.envelope_hash FROM sync_auth_envelopes e
             WHERE e.space_id = ?1 AND e.writer_id = ?2 AND e.role = 'evidence'
               AND NOT EXISTS (SELECT 1 FROM sync_auth_envelopes t
                 WHERE t.space_id = e.space_id AND t.writer_id = e.writer_id AND t.seq = e.seq
                   AND t.envelope_hash <> e.envelope_hash)
               AND NOT (
                 (SELECT count(DISTINCT n.envelope_hash) FROM sync_auth_envelopes n
                   WHERE n.space_id = e.space_id AND n.writer_id = e.writer_id
                     AND n.seq = e.seq + 1) >= 2
                 AND EXISTS (SELECT 1 FROM sync_auth_envelopes n
                   WHERE n.space_id = e.space_id AND n.writer_id = e.writer_id
                     AND n.seq = e.seq + 1 AND n.previous_hash = e.envelope_hash))",
        )
        .bind(space.as_bytes().as_slice())
        .bind(writer.as_bytes().as_slice())
        .fetch_all(&mut *conn)
        .await?;
        if orphans.is_empty() {
            return Ok(());
        }
        for hash in &orphans {
            sqlx::query(
                "DELETE FROM sync_auth_envelopes
                 WHERE space_id = ? AND role = 'evidence' AND envelope_hash = ?",
            )
            .bind(space.as_bytes().as_slice())
            .bind(hash.as_slice())
            .execute(&mut *conn)
            .await?;
        }
    }
}

/// Whether envelopes were stored since the last walk read the store.
pub(crate) async fn grown(conn: &mut SqliteConnection, space: SpaceId) -> SyncResult<bool> {
    let unwalked: Option<i64> = sqlx::query_scalar(
        "SELECT 1 FROM sync_auth_envelopes WHERE space_id = ? AND walked = 0 LIMIT 1",
    )
    .bind(space.as_bytes().as_slice())
    .fetch_optional(&mut *conn)
    .await?;
    Ok(unwalked.is_some())
}

/// Marks every stored envelope read by a walk, and recomputes the wanted set when envelopes
/// arrived since the last walk.
pub(crate) async fn walked(
    conn: &mut SqliteConnection,
    space: SpaceId,
    charter_hash: &Digest32,
) -> SyncResult<()> {
    if !grown(conn, space).await? {
        return Ok(());
    }
    refresh_wanted(conn, space, charter_hash).await?;
    sqlx::query("UPDATE sync_auth_envelopes SET walked = 1 WHERE space_id = ? AND walked = 0")
        .bind(space.as_bytes().as_slice())
        .execute(&mut *conn)
        .await?;
    Ok(())
}

/// Recomputes what the store lacks: the entry each record continues from, the envelope each
/// fork continues from, and a certificate of each stored writer.
pub(crate) async fn refresh_wanted(
    conn: &mut SqliteConnection,
    space: SpaceId,
    charter_hash: &Digest32,
) -> SyncResult<()> {
    let space_bytes = space.as_bytes().as_slice();
    sqlx::query("DELETE FROM sync_auth_wanted WHERE space_id = ?")
        .bind(space_bytes)
        .execute(&mut *conn)
        .await?;
    sqlx::query(
        "INSERT OR IGNORE INTO sync_auth_wanted (space_id, kind, key)
         SELECT ?1, 'entry', r.previous_entry_hash FROM sync_auth_envelopes r
         WHERE r.space_id = ?1 AND r.role = 'record' AND r.previous_entry_hash != ?2
           AND NOT EXISTS (SELECT 1 FROM sync_auth_envelopes p
                           WHERE p.space_id = ?1 AND p.role = 'record'
                             AND p.entry_hash = r.previous_entry_hash)",
    )
    .bind(space_bytes)
    .bind(charter_hash.as_slice())
    .execute(&mut *conn)
    .await?;
    sqlx::query(
        "INSERT OR IGNORE INTO sync_auth_wanted (space_id, kind, key)
         SELECT ?1, 'envelope', e.previous_hash FROM sync_auth_envelopes e
         WHERE e.space_id = ?1 AND e.seq > 1
           AND (e.role = 'evidence'
                OR EXISTS (SELECT 1 FROM sync_auth_envelopes t
                           WHERE t.space_id = ?1 AND t.writer_id = e.writer_id
                             AND t.seq = e.seq AND t.envelope_hash != e.envelope_hash))
           AND NOT EXISTS (SELECT 1 FROM sync_auth_envelopes p
                           WHERE p.space_id = ?1 AND p.envelope_hash = e.previous_hash)",
    )
    .bind(space_bytes)
    .execute(&mut *conn)
    .await?;
    sqlx::query(
        "INSERT OR IGNORE INTO sync_auth_wanted (space_id, kind, key)
         SELECT DISTINCT ?1, 'writer', e.writer_id FROM sync_auth_envelopes e
         WHERE e.space_id = ?1
           AND NOT EXISTS (SELECT 1 FROM sync_auth_envelopes c
                           WHERE c.space_id = ?1 AND c.role = 'certificate'
                             AND c.writer_id = e.writer_id)",
    )
    .bind(space_bytes)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// One verified envelope as the horizon and the fork evidence read it.
struct Slot {
    previous_hash: Digest32,
    stamp: u64,
}

/// Derives the nodes, certificates, and provable forks of the verified envelopes `space` stores,
/// whose chains start at `charter_hash`.
pub(crate) async fn load(
    conn: &mut SqliteConnection,
    space: SpaceId,
    charter_hash: &Digest32,
) -> SyncResult<Loaded> {
    let rows = sqlx::query(
        "SELECT role, envelope_hash, writer_id, seq, previous_hash, stamp, device_id, device_key,
            envelope
         FROM sync_auth_envelopes WHERE space_id = ? AND verified = 1",
    )
    .bind(space.as_bytes().as_slice())
    .fetch_all(&mut *conn)
    .await?;
    let mut loaded = Loaded::default();
    let mut slots: BTreeMap<WriterId, BTreeMap<Seq, Vec<Slot>>> = BTreeMap::new();
    let mut stamps: BTreeMap<Digest32, u64> = BTreeMap::new();
    let mut bodies: BTreeMap<Digest32, (AuthorizationRecord, Vec<Carrier>)> = BTreeMap::new();
    for row in &rows {
        let role: String = row.try_get(0)?;
        let hash = log::digest(&row.try_get::<Vec<u8>, _>(1)?)?;
        let writer = log::writer_id(&row.try_get::<Vec<u8>, _>(2)?)?;
        let seq = stored_u64(row.try_get(3)?, "sequence")?;
        let previous_hash = log::digest(&row.try_get::<Vec<u8>, _>(4)?)?;
        let stamp = stored_u64(row.try_get(5)?, "stamp")?;
        if stamps.insert(hash, stamp).is_some() {
            continue;
        }
        slots
            .entry(writer)
            .or_default()
            .entry(seq)
            .or_default()
            .push(Slot {
                previous_hash,
                stamp,
            });
        match role.as_str() {
            "certificate" => {
                let device_id: String = row.try_get(6)?;
                let key: Vec<u8> = row.try_get(7)?;
                let key = DevicePublicKey::from_bytes(log::digest(&key)?);
                loaded
                    .certificates
                    .entry(writer)
                    .or_default()
                    .push((device_id, key));
            }
            "record" => {
                let envelope: Vec<u8> = row.try_get(8)?;
                let record = crate::authorization::record_of(&envelope)
                    .ok_or_else(|| corrupt("a stored authorization record does not decode"))?;
                loaded.max_revision = loaded.max_revision.max(record.revision);
                let carrier = Carrier {
                    writer,
                    seq,
                    envelope_hash: hash,
                };
                bodies
                    .entry(record.hash())
                    .or_insert_with(|| (record, Vec::new()))
                    .1
                    .push(carrier);
            }
            _ => {}
        }
    }
    let mut horizons = BTreeMap::new();
    for (writer, by_seq) in &slots {
        if let Some(seq) = by_seq
            .iter()
            .find(|(_, slots)| slots.len() > 1)
            .map(|(seq, _)| *seq)
        {
            horizons.insert(*writer, seq);
        }
        if let Some(seq) = by_seq
            .iter()
            .find(|(_, slots)| provable(slots, &stamps))
            .map(|(seq, _)| *seq)
        {
            loaded.provable.insert(*writer, seq);
        }
    }
    let complete = complete_chains(&bodies, charter_hash);
    let mut included = BTreeSet::new();
    let anchors = bodies.iter().filter(|(hash, (_, carriers))| {
        complete.contains(*hash)
            && carriers.iter().any(|carrier| {
                horizons
                    .get(&carrier.writer)
                    .is_none_or(|horizon| carrier.seq < *horizon)
            })
    });
    for (hash, _) in anchors {
        let mut at = *hash;
        while at != *charter_hash && included.insert(at) {
            at = bodies[&at].0.previous_entry_hash;
        }
    }
    for hash in included {
        if let Some((record, mut carriers)) = bodies.remove(&hash) {
            carriers.sort_by_key(|carrier| carrier_rank(carrier, &horizons));
            loaded.nodes.push(Node {
                hash,
                record,
                carriers,
            });
        }
    }
    for (hash, (record, mut carriers)) in bodies {
        carriers.sort_by_key(|carrier| carrier_rank(carrier, &horizons));
        loaded.others.push(Node {
            hash,
            record,
            carriers,
        });
    }
    loaded
        .nodes
        .sort_by_key(|node| (node.record.revision, node.hash));
    loaded
        .others
        .sort_by_key(|node| (node.record.revision, node.hash));
    Ok(loaded)
}

/// Order of a node's carriers: those below their writer's horizon first, then lowest envelope
/// hash. Envelopes of a forked writer past its fork do not reach every replica, so the selected
/// chain names an envelope of one only when no other envelope carries the entry, and replicas
/// that store different forked branches still name the same envelope.
fn carrier_rank(carrier: &Carrier, horizons: &BTreeMap<WriterId, Seq>) -> (bool, Digest32) {
    let past_horizon = horizons
        .get(&carrier.writer)
        .is_some_and(|horizon| carrier.seq >= *horizon);
    (past_horizon, carrier.envelope_hash)
}

/// Whether two distinct envelopes of one sequence continue from one stored envelope and neither
/// claims a revision below it.
fn provable(slots: &[Slot], stamps: &BTreeMap<Digest32, u64>) -> bool {
    slots.iter().enumerate().any(|(index, a)| {
        slots[index + 1..].iter().any(|b| {
            a.previous_hash == b.previous_hash
                && stamps
                    .get(&a.previous_hash)
                    .is_some_and(|prefix| a.stamp >= *prefix && b.stamp >= *prefix)
        })
    })
}

/// The record bodies whose chain to the charter is stored completely.
fn complete_chains(
    bodies: &BTreeMap<Digest32, (AuthorizationRecord, Vec<Carrier>)>,
    charter_hash: &Digest32,
) -> BTreeSet<Digest32> {
    let mut complete = BTreeSet::new();
    let mut broken = BTreeSet::new();
    for start in bodies.keys() {
        let mut path = Vec::new();
        let mut at = *start;
        let reaches = loop {
            if at == *charter_hash || complete.contains(&at) {
                break true;
            }
            // An entry hash covers its previous entry hash, so chains cannot cycle; the length
            // bound only keeps a corrupt store from looping.
            if broken.contains(&at) || path.len() > bodies.len() {
                break false;
            }
            let Some((record, _)) = bodies.get(&at) else {
                break false;
            };
            path.push(at);
            at = record.previous_entry_hash;
        };
        if reaches {
            complete.extend(path);
        } else {
            broken.extend(path);
        }
    }
    complete
}

/// Why the local device stops sealing with a writer and seals with a successor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriterRetirement {
    /// The record store holds two distinct verified envelopes of the writer at one sequence,
    /// provable or not, certificate twins included, so its key may be in other hands.
    Forked,
    /// The selected chain removed the writer's device while the writer belonged to it, so a
    /// device enrolled again under the same key seals with a new writer.
    Removed,
}

impl crate::Engine {
    /// Whether the local device must retire `writer`, see [`WriterRetirement`]. The successor's
    /// certificate is signed by the unchanged device key and names `writer` as predecessor, so
    /// a copy of the old writer key alone cannot mint it. No authority rule reads this: it only
    /// chooses which writer the local device seals with.
    pub async fn writer_retirement(
        &self,
        conn: &mut SqliteConnection,
        ctx: &crate::SpaceContext,
        writer: WriterId,
    ) -> SyncResult<Option<WriterRetirement>> {
        log::ensure_space(conn, ctx.space).await?;
        let forked: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM sync_auth_envelopes
             WHERE space_id = ? AND writer_id = ? AND verified = 1
             GROUP BY seq HAVING COUNT(DISTINCT envelope_hash) > 1)",
        )
        .bind(ctx.space.as_bytes().as_slice())
        .bind(writer.as_bytes().as_slice())
        .fetch_one(&mut *conn)
        .await?;
        if forked {
            return Ok(Some(WriterRetirement::Forked));
        }
        let removed: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM sync_writers
             WHERE space_id = ? AND writer_id = ? AND removal_cutoff_seq IS NOT NULL)",
        )
        .bind(ctx.space.as_bytes().as_slice())
        .bind(writer.as_bytes().as_slice())
        .fetch_one(&mut *conn)
        .await?;
        Ok(removed.then_some(WriterRetirement::Removed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn carrier(writer: u8, seq: Seq, hash: u8) -> Carrier {
        Carrier {
            writer: WriterId::from_bytes([writer; 16]),
            seq,
            envelope_hash: [hash; 32],
        }
    }

    #[test]
    fn carriers_past_a_fork_rank_after_every_other_carrier() {
        let horizons = BTreeMap::from([(WriterId::from_bytes([1; 16]), 3)]);
        let mut carriers = vec![
            carrier(1, 4, 0x10),
            carrier(2, 4, 0x90),
            carrier(1, 2, 0x50),
            carrier(3, 9, 0x40),
        ];
        carriers.sort_by_key(|carrier| carrier_rank(carrier, &horizons));
        let order: Vec<u8> = carriers
            .iter()
            .map(|carrier| carrier.envelope_hash[0])
            .collect();
        assert_eq!(order, [0x40, 0x50, 0x90, 0x10]);
    }

    #[test]
    fn carriers_all_past_a_fork_rank_by_lowest_hash() {
        let horizons = BTreeMap::from([(WriterId::from_bytes([1; 16]), 3)]);
        let mut carriers = vec![carrier(1, 5, 0x80), carrier(1, 3, 0x30)];
        carriers.sort_by_key(|carrier| carrier_rank(carrier, &horizons));
        assert_eq!(carriers, [carrier(1, 3, 0x30), carrier(1, 5, 0x80)]);
    }
}
