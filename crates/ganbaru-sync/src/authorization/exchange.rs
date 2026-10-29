//! Record exchange: the pages, lookups, and admissions that move the authorization record store
//! between linked devices ahead of the operation lanes.
//!
//! Which envelopes a page carries first, and how fast a peer may send them, is device-local
//! transport policy: it reads the local selection, but no authority rule reads it back. A page
//! set run to completion delivers the source's whole store, since the last class is a full pass
//! over every stored envelope in stored order.

use std::collections::{BTreeMap, BTreeSet};

use ganbaru_sync_contracts::{
    AdminPublicKey, Content, Digest32, Entry, Envelope, OperationKind, WriterId,
};
use sqlx::{Connection, Row, SqliteConnection};

use super::{Resolution, Stored};
use crate::error::{SyncResult, corrupt};
use crate::log::{self, stored_u64};
use crate::{Engine, SpaceContext, records};

/// Envelope classes of the exchange, in serving priority.
pub const RECORD_CLASSES: usize = 5;

/// Rows one page scans per class before it reports `more` and leaves the rest for the next.
const SCAN_ROWS: i64 = 4_096;

/// Why an envelope is served before others, see the R7.3 A6 rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RecordClass {
    /// Records the kit key at the local selected head signs, their ancestors, and the
    /// certificates of their writers.
    Kit,
    /// Yields and records that object to a handover.
    Contest,
    /// Ancestors and certificates a requester named as missing.
    Wanted,
    /// Records and certificates of devices live and not held at the local head.
    Live,
    /// Everything else, served through the full pass.
    Rest,
}

impl RecordClass {
    /// Every class, in serving priority.
    pub const ALL: [Self; RECORD_CLASSES] = [
        Self::Kit,
        Self::Contest,
        Self::Wanted,
        Self::Live,
        Self::Rest,
    ];

    /// Position of the class in [`RecordCursor`].
    pub const fn index(self) -> usize {
        match self {
            Self::Kit => 0,
            Self::Contest => 1,
            Self::Wanted => 2,
            Self::Live => 3,
            Self::Rest => 4,
        }
    }
}

/// What a stored envelope is to the record store.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecordRole {
    /// A kind 4 authorization record.
    Record,
    /// A genesis envelope, which carries a writer certificate.
    Certificate,
    /// Any other envelope stored as fork evidence.
    Evidence,
}

impl RecordRole {
    fn parse(text: &str) -> SyncResult<Self> {
        match text {
            "record" => Ok(Self::Record),
            "certificate" => Ok(Self::Certificate),
            "evidence" => Ok(Self::Evidence),
            _ => Err(corrupt("stored authorization envelope role is invalid")),
        }
    }
}

/// The stored order a requester reached in each class of one source's store. The source keeps
/// no state per requester: the requester sends the cursor back with each page request.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RecordCursor(pub [u64; RECORD_CLASSES]);

/// One served envelope.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordEnvelope {
    /// The envelope bytes.
    pub bytes: Vec<u8>,
    /// Its role in the source's store.
    pub role: RecordRole,
    /// Its class at the source's selected head.
    pub class: RecordClass,
}

/// A page of the record store past a cursor.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RecordPage {
    /// Served envelopes, in class priority and stored order.
    pub envelopes: Vec<RecordEnvelope>,
    /// Where the next page continues.
    pub cursor: RecordCursor,
    /// Whether some class has envelopes past the cursor.
    pub more: bool,
}

/// What a store lacks to complete the chains of what it holds.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RecordWants {
    /// Entry hashes of missing parent records, envelope hashes of missing chain prefixes, and
    /// envelope hashes whose pending copy was purged before a verified copy was stored.
    pub hashes: Vec<Digest32>,
    /// Writers with stored envelopes and no stored certificate.
    pub writers: Vec<WriterId>,
}

impl RecordWants {
    /// Whether nothing is wanted.
    pub fn is_empty(&self) -> bool {
        self.hashes.is_empty() && self.writers.is_empty()
    }
}

/// What admitting a pushed or pulled set of envelopes did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RecordAdmission {
    /// Envelopes the store did not hold before.
    pub admitted: usize,
    /// The part of the store's wanted set the admitted envelopes reference, and the hashes
    /// whose pending copies they purged.
    pub missing: RecordWants,
}

/// Stored envelope bytes and envelope count of the record store, in total and by the device
/// each writer's certificate names.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AuthorizationStorage {
    /// Bytes of every stored envelope and refetch hash.
    pub bytes: u64,
    /// Stored envelopes and refetch hashes, each refetch hash counted as one envelope.
    pub envelopes: u64,
    /// Bytes each certified device carries, largest first. Envelopes of writers without a
    /// stored certificate are counted in the totals only.
    pub by_device: Vec<(String, u64)>,
    /// Writers with more than one stored certificate or two envelopes at one sequence.
    pub forked_writers: Vec<ForkedWriter>,
}

/// A writer of the record store with two envelopes at one sequence or two certificates.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForkedWriter {
    /// The writer.
    pub writer: WriterId,
    /// The device a stored certificate of the writer names, the lowest device id when several
    /// do, or `None` without a stored certificate.
    pub device_id: Option<String>,
}

/// One stored envelope as the classes read it, without its bytes.
struct Light {
    order: u64,
    role: RecordRole,
    writer: WriterId,
    entry: Option<Digest32>,
}

/// Envelope classes at the local selected head.
#[derive(Default)]
struct Classes {
    /// Entry hashes of class 1 records.
    kit: BTreeSet<Digest32>,
    /// Entry hashes of class 2 records.
    contest: BTreeSet<Digest32>,
    /// Writers whose certificates are class 1 or class 2, with the better class.
    writers: BTreeMap<WriterId, RecordClass>,
    /// Writers of devices live and not held at the local head.
    live: BTreeSet<WriterId>,
    /// Kit key at the local selected head.
    kit_key: Option<AdminPublicKey>,
    /// Keys of the wanted set: entry and envelope hashes, and writers.
    wanted: BTreeSet<Vec<u8>>,
}

impl Classes {
    /// The class of envelope bytes a peer sends, which this store may not hold yet: a record
    /// is class 1 when the local kit key signs it or the store already classes it so, class 2
    /// when it is a yield, and class 3 when the store wants it and it is not class 1 or 2.
    fn incoming(&self, bytes: &[u8]) -> RecordClass {
        let Ok(envelope) = Envelope::decode(bytes) else {
            return RecordClass::Rest;
        };
        let class = self.intrinsic(&envelope);
        if class <= RecordClass::Contest {
            return class;
        }
        let entry = envelope
            .operation()
            .ok()
            .and_then(|operation| match &operation.content {
                Content::Authorization(record) => Some(record.hash()),
                _ => None,
            });
        let wanted = self.wanted.contains(envelope.hash().as_slice())
            || entry.is_some_and(|entry| self.wanted.contains(entry.as_slice()))
            || (OperationKind::from_u8(envelope.kind) == Some(OperationKind::Genesis)
                && self.wanted.contains(envelope.writer.as_bytes().as_slice()));
        if wanted { RecordClass::Wanted } else { class }
    }

    /// The class of a decoded envelope from its content and the local head alone.
    fn intrinsic(&self, envelope: &Envelope<'_>) -> RecordClass {
        match OperationKind::from_u8(envelope.kind) {
            Some(OperationKind::Authorization) => {
                let Ok(operation) = envelope.operation() else {
                    return RecordClass::Rest;
                };
                let Content::Authorization(record) = &operation.content else {
                    return RecordClass::Rest;
                };
                let author = record.proposal.proposal.author;
                if self.kit_key == Some(author) {
                    return RecordClass::Kit;
                }
                if matches!(record.proposal.proposal.entry, Entry::Yield(_)) {
                    return RecordClass::Contest;
                }
                self.of(RecordRole::Record, envelope.writer, Some(&record.hash()))
            }
            Some(OperationKind::Genesis) => self.of(RecordRole::Certificate, envelope.writer, None),
            _ => RecordClass::Rest,
        }
    }

    fn of(&self, role: RecordRole, writer: WriterId, entry: Option<&Digest32>) -> RecordClass {
        match (role, entry) {
            (RecordRole::Record, Some(entry)) if self.kit.contains(entry) => RecordClass::Kit,
            (RecordRole::Record, Some(entry)) if self.contest.contains(entry) => {
                RecordClass::Contest
            }
            (RecordRole::Certificate, _) if self.writers.contains_key(&writer) => {
                self.writers[&writer]
            }
            (RecordRole::Evidence, _) => RecordClass::Rest,
            _ if self.live.contains(&writer) => RecordClass::Live,
            _ => RecordClass::Rest,
        }
    }
}

impl Engine {
    /// A page of the record store past `cursor`, at most `max_bytes` of envelopes and
    /// `max_envelopes` envelopes, always at least one envelope when one is left. Classes are
    /// served in priority order, except that the full pass keeps an eighth of the page.
    pub async fn record_page(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        cursor: &RecordCursor,
        max_bytes: usize,
        max_envelopes: usize,
    ) -> SyncResult<RecordPage> {
        log::ensure_space(conn, ctx.space).await?;
        let classes = classes(conn, ctx).await?;
        let mut page = RecordPage {
            cursor: *cursor,
            ..RecordPage::default()
        };
        let mut budget = Budget {
            bytes: max_bytes,
            envelopes: max_envelopes.max(1),
        };
        // The full pass runs first on its floor, so no priority class can starve it, and again
        // on whatever the priority classes leave.
        let mut floor = budget.split(max_bytes / 8, (max_envelopes / 8).max(1));
        // Orders already on this page, which a later class pass skips.
        let mut served = BTreeSet::new();
        // The second pass continues from the first one's cursor, so its result stands for the
        // full class.
        fill(
            conn,
            ctx,
            &classes,
            RecordClass::Rest,
            &mut floor,
            &mut page,
            &mut served,
        )
        .await?;
        budget.absorb(floor);
        let mut more = false;
        for class in [
            RecordClass::Kit,
            RecordClass::Contest,
            RecordClass::Live,
            RecordClass::Rest,
        ] {
            more |= fill(
                conn,
                ctx,
                &classes,
                class,
                &mut budget,
                &mut page,
                &mut served,
            )
            .await?;
        }
        page.more = more;
        Ok(page)
    }

    /// Stored envelopes whose envelope hash or record entry hash is among `hashes`, records
    /// first, each with its class.
    pub async fn records_by_hash(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        hashes: &[Digest32],
    ) -> SyncResult<Vec<RecordEnvelope>> {
        log::ensure_space(conn, ctx.space).await?;
        let mut found = Vec::new();
        let mut seen = BTreeSet::new();
        for hash in hashes {
            let rows = sqlx::query(
                "SELECT envelope_hash, role, envelope FROM sync_auth_envelopes
                 WHERE space_id = ?1 AND (envelope_hash = ?2 OR entry_hash = ?2)
                 ORDER BY stored_order",
            )
            .bind(ctx.space.as_bytes().as_slice())
            .bind(hash.as_slice())
            .fetch_all(&mut *conn)
            .await?;
            for row in rows {
                let envelope_hash: Vec<u8> = row.try_get("envelope_hash")?;
                if !seen.insert(envelope_hash) {
                    continue;
                }
                let role = RecordRole::parse(row.try_get("role")?)?;
                found.push(RecordEnvelope {
                    bytes: row.try_get("envelope")?,
                    role,
                    class: RecordClass::Wanted,
                });
            }
        }
        Ok(found)
    }

    /// Every stored certificate envelope of each of `writers`.
    pub async fn certificates_of(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        writers: &[WriterId],
    ) -> SyncResult<Vec<RecordEnvelope>> {
        log::ensure_space(conn, ctx.space).await?;
        let mut found = Vec::new();
        for writer in writers {
            let rows: Vec<Vec<u8>> = sqlx::query_scalar(
                "SELECT envelope FROM sync_auth_envelopes
                 WHERE space_id = ? AND role = 'certificate' AND writer_id = ?
                 ORDER BY stored_order",
            )
            .bind(ctx.space.as_bytes().as_slice())
            .bind(writer.as_bytes().as_slice())
            .fetch_all(&mut *conn)
            .await?;
            found.extend(rows.into_iter().map(|bytes| RecordEnvelope {
                bytes,
                role: RecordRole::Certificate,
                class: RecordClass::Wanted,
            }));
        }
        Ok(found)
    }

    /// Up to `limit` hashes and `limit` writers of what the record store lacks.
    pub async fn record_wants(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        limit: usize,
    ) -> SyncResult<RecordWants> {
        let mut tx = conn.begin().await?;
        log::ensure_space(&mut tx, ctx.space).await?;
        refresh(&mut tx, ctx).await?;
        let wants = wanted(&mut tx, ctx, None, limit).await?;
        tx.commit().await?;
        Ok(wants)
    }

    /// Admits `envelopes` to the record store: records and certificates by their intrinsic
    /// checks, and anything else as fork evidence. Returns how many it did not hold, and the
    /// part of its wanted set those envelopes reference. The next apply selects again.
    pub async fn admit_records(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        envelopes: &[Vec<u8>],
        now_ms: u64,
    ) -> SyncResult<RecordAdmission> {
        let mut tx = conn.begin().await?;
        log::ensure_space(&mut tx, ctx.space).await?;
        let mut admitted = 0;
        let mut referenced = BTreeSet::new();
        let refetched = refetch_hashes(&mut tx, ctx).await?;
        for bytes in envelopes {
            let Ok(envelope) = Envelope::decode(bytes) else {
                continue;
            };
            let grew = match OperationKind::from_u8(envelope.kind) {
                Some(OperationKind::Genesis | OperationKind::Authorization) => {
                    records::admit(&mut tx, ctx.space, bytes, now_ms).await?
                }
                _ => records::admit_evidence(&mut tx, ctx.space, &[bytes], now_ms).await?,
            };
            if grew {
                admitted += 1;
                referenced.insert(envelope.previous_hash.to_vec());
                referenced.insert(envelope.writer.as_bytes().to_vec());
                if let Ok(operation) = envelope.operation()
                    && let Content::Authorization(record) = &operation.content
                {
                    referenced.insert(record.previous_entry_hash.to_vec());
                }
            }
        }
        referenced.extend(
            refetch_hashes(&mut tx, ctx)
                .await?
                .difference(&refetched)
                .cloned(),
        );
        refresh(&mut tx, ctx).await?;
        let missing = wanted(&mut tx, ctx, Some(&referenced), usize::MAX).await?;
        tx.commit().await?;
        Ok(RecordAdmission { admitted, missing })
    }

    /// The class each of `envelopes` has at the local selected head, for ingress budgets.
    /// Bytes that do not decode are [`RecordClass::Rest`].
    pub async fn record_classes(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        envelopes: &[Vec<u8>],
    ) -> SyncResult<Vec<RecordClass>> {
        log::ensure_space(conn, ctx.space).await?;
        let classes = classes(conn, ctx).await?;
        Ok(envelopes
            .iter()
            .map(|bytes| classes.incoming(bytes))
            .collect())
    }

    /// Stored bytes and envelopes of the record store, for the storage backstop and growth
    /// alerts.
    pub async fn authorization_storage(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
    ) -> SyncResult<AuthorizationStorage> {
        log::ensure_space(conn, ctx.space).await?;
        let space = ctx.space.as_bytes().as_slice();
        let (bytes, envelopes): (i64, i64) = sqlx::query_as(
            "SELECT coalesce(sum(bytes), 0), coalesce(sum(rows), 0) FROM (
                 SELECT sum(length(envelope)) AS bytes, count(*) AS rows
                 FROM sync_auth_envelopes WHERE space_id = ?1
                 UNION ALL
                 SELECT sum(length(envelope_hash)), count(*)
                 FROM sync_auth_refetch WHERE space_id = ?1)",
        )
        .bind(space)
        .fetch_one(&mut *conn)
        .await?;
        let by_device: Vec<(String, i64)> = sqlx::query_as(
            "SELECT c.device_id, sum(length(e.envelope)) AS carried
             FROM sync_auth_envelopes e
             JOIN (SELECT DISTINCT writer_id, device_id FROM sync_auth_envelopes
                   WHERE space_id = ?1 AND role = 'certificate') c
               ON c.writer_id = e.writer_id
             WHERE e.space_id = ?1
             GROUP BY c.device_id ORDER BY carried DESC, c.device_id",
        )
        .bind(space)
        .fetch_all(&mut *conn)
        .await?;
        let forked: Vec<(Vec<u8>, Option<String>)> = sqlx::query_as(
            "SELECT f.writer_id,
                    (SELECT min(c.device_id) FROM sync_auth_envelopes c
                     WHERE c.space_id = ?1 AND c.role = 'certificate' AND c.writer_id = f.writer_id)
             FROM (SELECT DISTINCT writer_id FROM sync_auth_envelopes WHERE space_id = ?1
                   GROUP BY writer_id, seq HAVING count(*) > 1) f
             ORDER BY f.writer_id",
        )
        .bind(space)
        .fetch_all(&mut *conn)
        .await?;
        Ok(AuthorizationStorage {
            bytes: stored_u64(bytes, "authorization storage bytes")?,
            envelopes: stored_u64(envelopes, "authorization envelope count")?,
            by_device: by_device
                .into_iter()
                .map(|(device, carried)| {
                    Ok((device, stored_u64(carried, "authorization device bytes")?))
                })
                .collect::<SyncResult<_>>()?,
            forked_writers: forked
                .into_iter()
                .map(|(writer, device_id)| {
                    Ok(ForkedWriter {
                        writer: log::writer_id(&writer)?,
                        device_id,
                    })
                })
                .collect::<SyncResult<_>>()?,
        })
    }
}

/// What a page may still carry.
struct Budget {
    bytes: usize,
    envelopes: usize,
}

impl Budget {
    /// Splits up to `bytes` and `envelopes` off this budget and returns them.
    fn split(&mut self, bytes: usize, envelopes: usize) -> Self {
        let taken = Self {
            bytes: bytes.min(self.bytes),
            envelopes: envelopes.min(self.envelopes),
        };
        self.bytes -= taken.bytes;
        self.envelopes -= taken.envelopes;
        taken
    }

    /// Takes back what `other` did not spend.
    fn absorb(&mut self, other: Self) {
        self.bytes += other.bytes;
        self.envelopes += other.envelopes;
    }

    fn fits(&self, size: usize, first: bool) -> bool {
        self.envelopes > 0 && (size <= self.bytes || first)
    }
}

/// Adds envelopes of `class` past its cursor to `page` while `budget` allows. Envelopes in
/// `served` are already on the page and only move the cursor. Returns whether
/// the class has envelopes left past the cursor it reached.
async fn fill(
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    classes: &Classes,
    class: RecordClass,
    budget: &mut Budget,
    page: &mut RecordPage,
    served: &mut BTreeSet<u64>,
) -> SyncResult<bool> {
    let slot = class.index();
    let rows = sqlx::query(
        "SELECT stored_order, role, writer_id, entry_hash, length(envelope) AS size
         FROM sync_auth_envelopes WHERE space_id = ? AND stored_order > ?
         ORDER BY stored_order LIMIT ?",
    )
    .bind(ctx.space.as_bytes().as_slice())
    .bind(log::seq_i64(page.cursor.0[slot])?)
    .bind(SCAN_ROWS + 1)
    .fetch_all(&mut *conn)
    .await?;
    for (index, row) in rows.into_iter().enumerate() {
        if i64::try_from(index).is_ok_and(|index| index == SCAN_ROWS) {
            return Ok(true);
        }
        let light = light(&row)?;
        let own = classes.of(light.role, light.writer, light.entry.as_ref());
        if (class != RecordClass::Rest && own != class) || served.contains(&light.order) {
            page.cursor.0[slot] = light.order;
            continue;
        }
        let size = stored_u64(row.try_get("size")?, "authorization envelope size")?;
        let size = usize::try_from(size).map_err(|_| corrupt("envelope size overflows"))?;
        if !budget.fits(size, page.envelopes.is_empty()) {
            return Ok(true);
        }
        let bytes: Vec<u8> = sqlx::query_scalar(
            "SELECT envelope FROM sync_auth_envelopes WHERE space_id = ? AND stored_order = ?",
        )
        .bind(ctx.space.as_bytes().as_slice())
        .bind(log::seq_i64(light.order)?)
        .fetch_one(&mut *conn)
        .await?;
        budget.bytes = budget.bytes.saturating_sub(size);
        budget.envelopes -= 1;
        served.insert(light.order);
        page.cursor.0[slot] = light.order;
        page.envelopes.push(RecordEnvelope {
            bytes,
            role: light.role,
            class: own,
        });
    }
    Ok(false)
}

fn light(row: &sqlx::sqlite::SqliteRow) -> SyncResult<Light> {
    let entry: Option<Vec<u8>> = row.try_get("entry_hash")?;
    Ok(Light {
        order: stored_u64(row.try_get("stored_order")?, "authorization stored order")?,
        role: RecordRole::parse(row.try_get("role")?)?,
        writer: log::writer_id(&row.try_get::<Vec<u8>, _>("writer_id")?)?,
        entry: entry.map(|hash| log::digest(&hash)).transpose()?,
    })
}

/// The classes of the local selected head: the kit key's records and their ancestors, yields
/// and objections, and the writers of live devices that are not held.
async fn classes(conn: &mut SqliteConnection, ctx: &SpaceContext) -> SyncResult<Classes> {
    let mut classes = Classes::default();
    let Some(stored) = Stored::load(conn, ctx.space).await? else {
        return Ok(classes);
    };
    let resolution = Resolution::new(&stored);
    let walk = &resolution.walk;
    let kit_key = walk.visits[resolution.best].kit_key;
    let mut carriers: BTreeMap<Digest32, Vec<WriterId>> = BTreeMap::new();
    for node in &stored.nodes {
        carriers.insert(
            node.hash,
            node.carriers.iter().map(|carrier| carrier.writer).collect(),
        );
    }
    // Records the kit key signs, and the chain each continues from.
    let parents: BTreeMap<Digest32, Digest32> = stored
        .nodes
        .iter()
        .map(|node| (node.hash, node.record.previous_entry_hash))
        .collect();
    for node in &stored.nodes {
        if node.record.proposal.proposal.author != kit_key {
            continue;
        }
        let mut at = Some(node.hash);
        while let Some(hash) = at {
            if !classes.kit.insert(hash) {
                break;
            }
            at = parents.get(&hash).copied();
        }
    }
    for node in &stored.nodes {
        if matches!(node.record.proposal.proposal.entry, Entry::Yield(_)) {
            classes.contest.insert(node.hash);
        }
    }
    for candidates in resolution.candidates.values() {
        for candidate in candidates {
            for objection in walk.objections(*candidate) {
                classes.contest.insert(walk.visits[objection].hash);
            }
        }
    }
    for (set, class) in [
        (&classes.kit, RecordClass::Kit),
        (&classes.contest, RecordClass::Contest),
    ] {
        for hash in set {
            for writer in carriers.get(hash).into_iter().flatten() {
                classes
                    .writers
                    .entry(*writer)
                    .and_modify(|best| *best = (*best).min(class))
                    .or_insert(class);
            }
        }
    }
    let live: BTreeSet<String> = sqlx::query_scalar(
        "SELECT device_id FROM sync_devices
         WHERE space_id = ? AND removed_revision IS NULL AND held = 0",
    )
    .bind(ctx.space.as_bytes().as_slice())
    .fetch_all(&mut *conn)
    .await?
    .into_iter()
    .collect();
    for (writer, names) in &stored.certificates {
        if names.iter().any(|(device, _)| live.contains(device)) {
            classes.live.insert(*writer);
        }
    }
    classes.kit_key = Some(kit_key);
    classes.wanted = sqlx::query_scalar(
        "SELECT key FROM sync_auth_wanted WHERE space_id = ?1
         UNION SELECT envelope_hash FROM sync_auth_refetch WHERE space_id = ?1",
    )
    .bind(ctx.space.as_bytes().as_slice())
    .fetch_all(&mut *conn)
    .await?
    .into_iter()
    .collect();
    Ok(classes)
}

/// Recomputes the wanted set from the stored envelopes when they grew since it was computed.
async fn refresh(conn: &mut SqliteConnection, ctx: &SpaceContext) -> SyncResult<()> {
    if let Some(charter) = super::load_charter(conn, ctx.space).await? {
        records::refresh_wanted(conn, ctx.space, &charter.hash()).await?;
    }
    Ok(())
}

/// Envelope hashes whose pending copy was purged and that no verified copy has replaced yet.
async fn refetch_hashes(
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
) -> SyncResult<BTreeSet<Vec<u8>>> {
    Ok(
        sqlx::query_scalar("SELECT envelope_hash FROM sync_auth_refetch WHERE space_id = ?")
            .bind(ctx.space.as_bytes().as_slice())
            .fetch_all(&mut *conn)
            .await?
            .into_iter()
            .collect(),
    )
}

/// Up to `limit` hashes and writers of the wanted set and the refetch hashes, only those among
/// `among` when set.
async fn wanted(
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    among: Option<&BTreeSet<Vec<u8>>>,
    limit: usize,
) -> SyncResult<RecordWants> {
    let rows: Vec<(String, Vec<u8>)> = sqlx::query_as(
        "SELECT kind, key FROM sync_auth_wanted WHERE space_id = ?1
         UNION SELECT 'envelope', envelope_hash FROM sync_auth_refetch WHERE space_id = ?1
         ORDER BY 1, 2",
    )
    .bind(ctx.space.as_bytes().as_slice())
    .fetch_all(&mut *conn)
    .await?;
    let mut wants = RecordWants::default();
    for (kind, key) in rows {
        if among.is_some_and(|among| !among.contains(&key)) {
            continue;
        }
        match kind.as_str() {
            "writer" if wants.writers.len() < limit => {
                wants.writers.push(log::writer_id(&key)?);
            }
            "entry" | "envelope" if wants.hashes.len() < limit => {
                wants.hashes.push(log::digest(&key)?);
            }
            "writer" | "entry" | "envelope" => {}
            _ => return Err(corrupt("stored wanted kind is invalid")),
        }
    }
    Ok(wants)
}
