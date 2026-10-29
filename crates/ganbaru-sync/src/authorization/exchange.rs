//! Record exchange: the pages, lookups, and admissions that move the authorization record store
//! between linked devices ahead of the operation lanes.
//!
//! Which envelopes a page carries first, and how fast a peer may send them, is device-local
//! transport policy: it reads the local selection, but no authority rule reads it back. A page
//! set run to completion delivers the source's whole store, since the last class is a full pass
//! over every stored envelope in stored order.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use ganbaru_sync_contracts::{
    AdminPublicKey, AuthorizationRecord, Content, DevicePublicKey, Digest32, Entry, Envelope,
    OperationKind, Seq, WriterId, WriterPublicKey,
};
use sqlx::{Connection, Row, SqliteConnection};

use super::{AuthState, Resolution, Signer, Stored};
use crate::error::{SyncResult, corrupt};
use crate::log::{self, seq_i64, stored_u64};
use crate::{Engine, SpaceContext, records};

/// Envelope classes of the exchange, in serving priority.
pub const RECORD_CLASSES: usize = 5;

/// Rows one page scans per class before it reports `more` and leaves the rest for the next.
const SCAN_ROWS: i64 = 4_096;

/// Most recently stored records a wanted lookup reads to order the ancestors of kit records and
/// yields first, see [`priority_keys`].
const PRIORITY_SCAN_ROWS: i64 = 1_024;

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
    /// Entry hashes of class 1 records, each with the writers whose envelopes of it are class 1:
    /// the carriers [`Binding`] binds to a record the kit key signs, and the carriers the walk
    /// binds to each of its ancestors.
    kit: BTreeMap<Digest32, BTreeSet<WriterId>>,
    /// Entry hashes of class 2 records, each with the carriers the walk binds to it.
    contest: BTreeMap<Digest32, BTreeSet<WriterId>>,
    /// Writers whose certificates are class 1 or class 2, with the better class: the class of
    /// the best record the writer carries bound.
    writers: BTreeMap<WriterId, RecordClass>,
    /// Writers of devices live and not held at the local head.
    live: BTreeSet<WriterId>,
    /// Keys of the wanted set: entry and envelope hashes, and writers.
    wanted: BTreeSet<Vec<u8>>,
    /// What binds records the kit key signs to their carriers, `None` without a charter.
    binding: Option<Binding>,
}

impl Classes {
    fn of(&self, role: RecordRole, writer: WriterId, entry: Option<&Digest32>) -> RecordClass {
        let carries = |set: &BTreeMap<Digest32, BTreeSet<WriterId>>| {
            entry.is_some_and(|entry| set.get(entry).is_some_and(|set| set.contains(&writer)))
        };
        match role {
            RecordRole::Record if carries(&self.kit) => RecordClass::Kit,
            RecordRole::Record if carries(&self.contest) => RecordClass::Contest,
            RecordRole::Certificate if self.writers.contains_key(&writer) => self.writers[&writer],
            RecordRole::Evidence => RecordClass::Rest,
            _ if self.live.contains(&writer) => RecordClass::Live,
            _ => RecordClass::Rest,
        }
    }

    fn wants(&self, key: &[u8]) -> bool {
        self.wanted.contains(key)
    }

    /// The class of a record a peer sends that is not class 1: [`RecordClass::Rest`] when the
    /// store or an earlier envelope of the set holds its entry, class 2 for a yield its parent
    /// on the selected chain admits, class 3 when the store wants it, and otherwise the class of
    /// its writer.
    fn record_class(
        &self,
        envelope: &Envelope<'_>,
        record: &AuthorizationRecord,
        facts: &Facts,
        seen_entries: &mut BTreeSet<Digest32>,
    ) -> RecordClass {
        let entry = record.hash();
        if facts.entries.contains(&entry) || !seen_entries.insert(entry) {
            return RecordClass::Rest;
        }
        if matches!(record.proposal.proposal.entry, Entry::Yield(_))
            && self
                .binding
                .as_ref()
                .is_some_and(|binding| binding.admits(record))
        {
            return RecordClass::Contest;
        }
        if self.wants(entry.as_slice()) || self.wants(envelope.hash().as_slice()) {
            return RecordClass::Wanted;
        }
        self.of(RecordRole::Record, envelope.writer, Some(&entry))
    }
}

/// A state the binding reads at an entry hash, see [`Binding`].
#[derive(Clone)]
struct BoundState {
    state: AuthState,
    /// Whether the state's epoch instance was opened by a handover the kit key signed, so the
    /// anchor of the instance is the device that handover names.
    kit_instance: bool,
}

/// What binds a record the head's kit key signs to the envelopes that carry it.
///
/// The kit key signs only the proposal. The record's parent, revision, anchor field, and
/// sequencing, and the writer that carries it, are free, so anyone holding a copy can wrap it
/// anew. A carrier is bound only when the certificate of its writer names the device key that
/// signed content or local state the sender cannot choose fixes:
///
/// - An enrollment: the device key it enrolls.
/// - A handover: the key its target holds at the record's parent, or, when the parent is not
///   stored yet, a key a stored enrollment the kit key signs names for the target. The parent of
///   a handover is fixed by its signed fields.
/// - Any other record: the key of the anchor device at the record's parent.
///
/// The parent of an enrollment or any other record must be on the local selected chain, or be
/// reached from it through bound records of an epoch instance a kit handover opened, and the
/// record must pass its rules there. Otherwise nothing binds it yet and it takes the sender's
/// class, which only delays it. Each parent and proposal pair binds one entry: an envelope of
/// another entry with the same pair is not class 1.
struct Binding {
    kit_key: AdminPublicKey,
    /// States by entry hash: those the local selected chain reaches and records need, and those
    /// bound records extend.
    states: BTreeMap<Digest32, BoundState>,
    /// Device keys stored enrollments the kit key signs name, by device id.
    enrolled: BTreeMap<String, BTreeSet<DevicePublicKey>>,
    /// Device keys each stored certificate of a writer names.
    certificates: BTreeMap<WriterId, Vec<DevicePublicKey>>,
    /// Entries stored with a bound carrier.
    bound_entries: BTreeSet<Digest32>,
    /// Parent and proposal hash pairs stored with a bound carrier.
    bound_pairs: BTreeSet<(Digest32, Digest32)>,
}

impl Binding {
    /// The device keys a carrier's certificate must name for `record` to be class 1, empty when
    /// the kit key does not sign it or nothing binds it yet.
    fn keys(&self, record: &AuthorizationRecord) -> BTreeSet<DevicePublicKey> {
        let proposal = &record.proposal.proposal;
        if proposal.author != self.kit_key {
            return BTreeSet::new();
        }
        let Some(parent) = self.states.get(&record.previous_entry_hash) else {
            return match &proposal.entry {
                Entry::Handover(handover) => self
                    .enrolled
                    .get(&handover.new_anchor_device_id)
                    .cloned()
                    .unwrap_or_default(),
                _ => BTreeSet::new(),
            };
        };
        let state = &parent.state;
        if state.check(record).is_err() {
            return BTreeSet::new();
        }
        let device = match &proposal.entry {
            Entry::Enroll(enroll) => return BTreeSet::from([enroll.device_key]),
            Entry::Handover(handover) => state.devices.get(&handover.new_anchor_device_id),
            _ => state.devices.get(&state.anchor_device_id),
        };
        device.map(|device| device.device_key).into_iter().collect()
    }

    /// Whether a state the binding reads at the parent of `record` admits it.
    fn admits(&self, record: &AuthorizationRecord) -> bool {
        self.states
            .get(&record.previous_entry_hash)
            .is_some_and(|parent| parent.state.check(record).is_ok())
    }

    /// Adds the state after `record`, bound at its stored parent, when it opens an epoch
    /// instance by a kit handover or continues one.
    fn extend(&mut self, record: &AuthorizationRecord, hash: Digest32) {
        let Some(parent) = self.states.get(&record.previous_entry_hash) else {
            return;
        };
        let handover = matches!(record.proposal.proposal.entry, Entry::Handover(_));
        if (!handover && !parent.kit_instance) || parent.state.check(record).is_err() {
            return;
        }
        let mut next = parent.clone();
        if handover {
            next.kit_instance = next.state.signer(record) == Some(Signer::Kit);
        }
        next.state.advance(record, hash);
        self.states.entry(hash).or_insert(next);
    }

    /// Records a bound entry, returning whether it is the first of its parent and proposal.
    fn bind(&mut self, record: &AuthorizationRecord, hash: Digest32) -> bool {
        let pair = (record.previous_entry_hash, record.proposal.hash());
        if self.bound_entries.contains(&hash) || self.bound_pairs.contains(&pair) {
            return false;
        }
        self.bound_entries.insert(hash);
        self.bound_pairs.insert(pair);
        true
    }
}

/// A certificate of a writer: its writer key and the device key it names.
type CarrierKey = (WriterPublicKey, DevicePublicKey);

/// The record a decoded envelope carries, `None` for other kinds.
fn record_in(envelope: &Envelope<'_>) -> Option<AuthorizationRecord> {
    if OperationKind::from_u8(envelope.kind) != Some(OperationKind::Authorization) {
        return None;
    }
    match envelope.operation().ok()?.content {
        Content::Authorization(record) => Some(*record),
        _ => None,
    }
}

/// The certificate a decoded genesis envelope carries for its own writer.
fn certificate_in(envelope: &Envelope<'_>) -> Option<CarrierKey> {
    if OperationKind::from_u8(envelope.kind) != Some(OperationKind::Genesis) {
        return None;
    }
    let (_, signed) = envelope.genesis().ok()?;
    let certificate = signed.certificate;
    (certificate.writer_id() == envelope.writer)
        .then_some((certificate.writer_key, certificate.device_key))
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
        let classes = classes(conn, ctx, &BTreeSet::new()).await?;
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

    /// Stored envelopes whose envelope hash or record entry hash is among `hashes`, at most
    /// `max_bytes` of envelopes and `max_envelopes` envelopes, always at least one when one is
    /// stored. The carriers of one entry come verified first, then lowest envelope hash first.
    pub async fn records_by_hash(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        hashes: &[Digest32],
        max_bytes: usize,
        max_envelopes: usize,
    ) -> SyncResult<Vec<RecordEnvelope>> {
        log::ensure_space(conn, ctx.space).await?;
        let mut budget = Budget {
            bytes: max_bytes,
            envelopes: max_envelopes.max(1),
        };
        let mut found = Vec::new();
        let mut seen = BTreeSet::new();
        for hash in hashes {
            let rows = sqlx::query(
                "SELECT envelope_hash, role, length(envelope) AS size FROM sync_auth_envelopes
                 WHERE space_id = ?1 AND (envelope_hash = ?2 OR entry_hash = ?2)
                 ORDER BY verified DESC, envelope_hash LIMIT ?3",
            )
            .bind(ctx.space.as_bytes().as_slice())
            .bind(hash.as_slice())
            .bind(row_limit(budget.envelopes))
            .fetch_all(&mut *conn)
            .await?;
            for row in rows {
                let envelope_hash: Vec<u8> = row.try_get("envelope_hash")?;
                if seen.contains(&envelope_hash) {
                    continue;
                }
                let role: String = row.try_get("role")?;
                if !take(&mut budget, &row, found.is_empty())? {
                    return Ok(found);
                }
                let bytes = envelope_bytes(conn, ctx, &envelope_hash, &role).await?;
                seen.insert(envelope_hash);
                found.push(RecordEnvelope {
                    bytes,
                    role: RecordRole::parse(&role)?,
                    class: RecordClass::Wanted,
                });
            }
        }
        Ok(found)
    }

    /// Stored certificate envelopes of each of `writers`, at most `max_bytes` of envelopes and
    /// `max_envelopes` envelopes, always at least one when one is stored, lowest envelope hash
    /// first for each writer.
    pub async fn certificates_of(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        writers: &[WriterId],
        max_bytes: usize,
        max_envelopes: usize,
    ) -> SyncResult<Vec<RecordEnvelope>> {
        log::ensure_space(conn, ctx.space).await?;
        let mut budget = Budget {
            bytes: max_bytes,
            envelopes: max_envelopes.max(1),
        };
        let mut found = Vec::new();
        for writer in writers {
            let rows = sqlx::query(
                "SELECT envelope_hash, length(envelope) AS size FROM sync_auth_envelopes
                 WHERE space_id = ? AND role = 'certificate' AND writer_id = ?
                 ORDER BY envelope_hash LIMIT ?",
            )
            .bind(ctx.space.as_bytes().as_slice())
            .bind(writer.as_bytes().as_slice())
            .bind(row_limit(budget.envelopes))
            .fetch_all(&mut *conn)
            .await?;
            for row in rows {
                if !take(&mut budget, &row, found.is_empty())? {
                    return Ok(found);
                }
                let envelope_hash: Vec<u8> = row.try_get("envelope_hash")?;
                found.push(RecordEnvelope {
                    bytes: envelope_bytes(conn, ctx, &envelope_hash, "certificate").await?,
                    role: RecordRole::Certificate,
                    class: RecordClass::Wanted,
                });
            }
        }
        Ok(found)
    }

    /// Up to `limit` hashes and `limit` writers of what the record store lacks, leaving out the
    /// keys in `skip` before the limit applies. Refetch hashes come first, then the missing
    /// ancestors of recent records the kit key signs and of recent yields, then the rest in key
    /// order.
    pub async fn record_wants(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        limit: usize,
        skip: &BTreeSet<Vec<u8>>,
    ) -> SyncResult<RecordWants> {
        let mut tx = conn.begin().await?;
        log::ensure_space(&mut tx, ctx.space).await?;
        refresh(&mut tx, ctx).await?;
        let wants = wanted_first(&mut tx, ctx, limit, skip).await?;
        tx.commit().await?;
        Ok(wants)
    }

    /// Admits `envelopes` to the record store: records and certificates by their intrinsic
    /// checks, and any other envelope as fork evidence only when it is a twin of a stored
    /// envelope at its writer and sequence, or the envelope a stored twin pair continues from.
    /// Returns how many it did not hold, and the part of its wanted set those envelopes
    /// reference. The next apply selects again.
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
        let decoded: Vec<(&[u8], Envelope<'_>)> = envelopes
            .iter()
            .filter_map(|bytes| Some((bytes.as_slice(), Envelope::decode(bytes).ok()?)))
            .collect();
        for (bytes, envelope) in &decoded {
            let grew = match OperationKind::from_u8(envelope.kind) {
                Some(OperationKind::Genesis | OperationKind::Authorization) => {
                    records::admit(&mut tx, ctx.space, bytes, now_ms).await?
                }
                _ if is_evidence(&mut tx, ctx, envelope).await? => {
                    records::admit_evidence(&mut tx, ctx.space, &[bytes], now_ms).await?
                }
                _ => false,
            };
            if grew {
                admitted += 1;
                referenced.insert(envelope.previous_hash.to_vec());
                referenced.insert(envelope.writer.as_bytes().to_vec());
                if let Some(record) = record_in(envelope) {
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
    ///
    /// An envelope the store holds, or one sent twice, is [`RecordClass::Rest`], and so is a
    /// record whose entry the store holds verified. A record the kit key signs is class 1 only
    /// when [`Binding`] binds its carrier, and then only for the first entry of its parent and
    /// proposal; otherwise it takes the class any other record of its sender would. A
    /// certificate is class 1 only when its writer carries a class 1 record of the same set.
    /// Bytes that do not decode are [`RecordClass::Rest`].
    pub async fn record_classes(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        envelopes: &[Vec<u8>],
    ) -> SyncResult<Vec<RecordClass>> {
        log::ensure_space(conn, ctx.space).await?;
        let decoded: Vec<Option<Envelope<'_>>> = envelopes
            .iter()
            .map(|bytes| Envelope::decode(bytes).ok())
            .collect();
        let records: Vec<Option<AuthorizationRecord>> = decoded
            .iter()
            .map(|envelope| envelope.as_ref().and_then(record_in))
            .collect();
        let needed = records
            .iter()
            .flatten()
            .map(|record| record.previous_entry_hash)
            .collect();
        let mut classes = classes(conn, ctx, &needed).await?;
        let facts = Facts::load(conn, ctx, &decoded, &records).await?;
        let mut kit = vec![false; envelopes.len()];
        let mut kit_writers = BTreeSet::new();
        if let Some(binding) = classes.binding.as_mut() {
            let mut order: Vec<usize> = (0..envelopes.len())
                .filter(|index| {
                    records[*index]
                        .as_ref()
                        .is_some_and(|record| record.proposal.proposal.author == binding.kit_key)
                })
                .collect();
            order.sort_by_key(|index| (records[*index].as_ref().map(|r| r.revision), *index));
            let mut seen = BTreeSet::new();
            for index in order {
                let (Some(envelope), Some(record)) = (&decoded[index], &records[index]) else {
                    continue;
                };
                let hash = envelope.hash();
                let entry = record.hash();
                if facts.held.contains(&hash)
                    || !seen.insert(hash)
                    || facts.entries.contains(&entry)
                {
                    continue;
                }
                let keys = binding.keys(record);
                let bound = !keys.is_empty()
                    && facts.carrier_keys(envelope.writer, binding).iter().any(
                        |(writer_key, device_key)| {
                            keys.contains(device_key) && envelope.verify(writer_key)
                        },
                    );
                if bound && binding.bind(record, entry) {
                    binding.extend(record, entry);
                    kit[index] = true;
                    kit_writers.insert(envelope.writer);
                }
            }
        }
        let mut seen_hashes = BTreeSet::new();
        let mut seen_entries: BTreeSet<Digest32> = (0..envelopes.len())
            .filter(|index| kit[*index])
            .filter_map(|index| records[index].as_ref().map(AuthorizationRecord::hash))
            .collect();
        let mut seen_writers = BTreeSet::new();
        let mut out = Vec::with_capacity(envelopes.len());
        for (index, envelope) in decoded.iter().enumerate() {
            let Some(envelope) = envelope else {
                out.push(RecordClass::Rest);
                continue;
            };
            let hash = envelope.hash();
            if facts.held.contains(&hash) || !seen_hashes.insert(hash) {
                out.push(RecordClass::Rest);
                continue;
            }
            let class = if kit[index] {
                RecordClass::Kit
            } else if let Some(record) = &records[index] {
                classes.record_class(envelope, record, &facts, &mut seen_entries)
            } else if OperationKind::from_u8(envelope.kind) == Some(OperationKind::Genesis) {
                let writer = envelope.writer;
                if facts.writer_keys.contains_key(&writer) || !seen_writers.insert(writer) {
                    RecordClass::Rest
                } else if kit_writers.contains(&writer) {
                    RecordClass::Kit
                } else if classes.wants(writer.as_bytes().as_slice()) {
                    RecordClass::Wanted
                } else {
                    RecordClass::Rest
                }
            } else if classes.wants(hash.as_slice()) {
                RecordClass::Wanted
            } else {
                RecordClass::Rest
            };
            out.push(class);
        }
        Ok(out)
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

/// The classes of the local selected head: the records the kit key signs that [`Binding`] binds
/// and their ancestors, yields and objections, and the writers of live devices that are not
/// held. `needed` names further entry hashes whose state on the selected chain the binding should
/// read, the parents of records a peer sends.
async fn classes(
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    needed: &BTreeSet<Digest32>,
) -> SyncResult<Classes> {
    let mut classes = Classes {
        wanted: sqlx::query_scalar(
            "SELECT key FROM sync_auth_wanted WHERE space_id = ?1
             UNION SELECT envelope_hash FROM sync_auth_refetch WHERE space_id = ?1",
        )
        .bind(ctx.space.as_bytes().as_slice())
        .fetch_all(&mut *conn)
        .await?
        .into_iter()
        .collect(),
        ..Classes::default()
    };
    let Some(stored) = Stored::load(conn, ctx.space).await? else {
        return Ok(classes);
    };
    let resolution = Resolution::new(&stored);
    let walk = &resolution.walk;
    let mut binding = binding(&stored, &resolution, needed);
    let visits: BTreeMap<Digest32, usize> = walk
        .visits
        .iter()
        .enumerate()
        .skip(1)
        .map(|(index, visit)| (visit.hash, index))
        .collect();
    let bound_writers = |visit: usize| walk.visits[visit].bound.iter().map(|(writer, _)| *writer);
    // Records the kit key signs, in revision order so a bound record's state is there for its
    // children, and the walked chain each continues from.
    let mut signed: Vec<&records::Node> = stored
        .nodes
        .iter()
        .filter(|node| node.record.proposal.proposal.author == binding.kit_key)
        .collect();
    signed.sort_by_key(|node| (node.record.revision, node.hash));
    for node in signed {
        let keys = binding.keys(&node.record);
        let bound: BTreeSet<WriterId> = node
            .carriers
            .iter()
            .map(|carrier| carrier.writer)
            .filter(|writer| {
                binding
                    .certificates
                    .get(writer)
                    .is_some_and(|named| named.iter().any(|key| keys.contains(key)))
            })
            .collect();
        if bound.is_empty() || !binding.bind(&node.record, node.hash) {
            continue;
        }
        binding.extend(&node.record, node.hash);
        classes.kit.entry(node.hash).or_default().extend(bound);
        let mut at = visits.get(&node.record.previous_entry_hash).copied();
        while let Some(visit) = at {
            let set = classes.kit.entry(walk.visits[visit].hash).or_default();
            let before = set.len();
            set.extend(bound_writers(visit));
            if set.len() == before {
                break;
            }
            at = walk.visits[visit].parent.filter(|parent| *parent != 0);
        }
    }
    let mut contest: Vec<usize> = (1..walk.visits.len())
        .filter(|visit| walk.visits[*visit].yields_to.is_some())
        .collect();
    for candidates in resolution.candidates.values() {
        for candidate in candidates {
            contest.extend(walk.objections(*candidate));
        }
    }
    for visit in contest {
        classes
            .contest
            .entry(walk.visits[visit].hash)
            .or_default()
            .extend(bound_writers(visit));
    }
    for (set, class) in [
        (&classes.kit, RecordClass::Kit),
        (&classes.contest, RecordClass::Contest),
    ] {
        for writer in set.values().flatten() {
            classes
                .writers
                .entry(*writer)
                .and_modify(|best| *best = (*best).min(class))
                .or_insert(class);
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
    classes.binding = Some(binding);
    Ok(classes)
}

/// The binding at the local selected head: the states of the selected chain at the parents of
/// stored records the kit key signs and at `needed`, and the enrollments the kit key signs.
fn binding(stored: &Stored, resolution: &Resolution<'_>, needed: &BTreeSet<Digest32>) -> Binding {
    let walk = &resolution.walk;
    let kit_key = walk.visits[resolution.best].kit_key;
    let mut at_hashes = needed.clone();
    let mut enrolled: BTreeMap<String, BTreeSet<DevicePublicKey>> = BTreeMap::new();
    for node in &stored.nodes {
        let proposal = &node.record.proposal.proposal;
        if proposal.author != kit_key {
            continue;
        }
        at_hashes.insert(node.record.previous_entry_hash);
        if let Entry::Enroll(enroll) = &proposal.entry {
            enrolled
                .entry(enroll.device_id.clone())
                .or_default()
                .insert(enroll.device_key);
        }
    }
    let mut path = Vec::new();
    let mut at = resolution.best;
    while at != 0 {
        path.push(at);
        at = walk.parent(at);
    }
    let mut state = AuthState::from_charter(&stored.charter, Arc::clone(&walk.vetoes));
    let mut kit_instance = false;
    let mut states = BTreeMap::new();
    if at_hashes.contains(&state.entry_hash) {
        states.insert(
            state.entry_hash,
            BoundState {
                state: state.clone(),
                kit_instance,
            },
        );
    }
    for visit in path.into_iter().rev() {
        let visit = &walk.visits[visit];
        let Some(node) = visit.node else {
            continue;
        };
        if let Some((signer, _)) = &visit.handover {
            kit_instance = *signer == Signer::Kit;
        }
        state.advance(&stored.nodes[node].record, visit.hash);
        if at_hashes.contains(&visit.hash) {
            states.insert(
                visit.hash,
                BoundState {
                    state: state.clone(),
                    kit_instance,
                },
            );
        }
    }
    Binding {
        kit_key,
        states,
        enrolled,
        certificates: stored
            .certificates
            .iter()
            .map(|(writer, names)| (*writer, names.iter().map(|(_, key)| *key).collect()))
            .collect(),
        bound_entries: BTreeSet::new(),
        bound_pairs: BTreeSet::new(),
    }
}

/// What the record store holds of a set of envelopes a peer sends.
struct Facts {
    /// Envelope hashes the store holds.
    held: BTreeSet<Digest32>,
    /// Entry hashes the store holds a verified carrier of.
    entries: BTreeSet<Digest32>,
    /// Writer keys of the writers with a stored certificate.
    writer_keys: BTreeMap<WriterId, WriterPublicKey>,
    /// Certificates the set carries for their own writers.
    sent: BTreeMap<WriterId, Vec<CarrierKey>>,
}

impl Facts {
    async fn load(
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        decoded: &[Option<Envelope<'_>>],
        records: &[Option<AuthorizationRecord>],
    ) -> SyncResult<Self> {
        let space = ctx.space.as_bytes();
        let mut facts = Self {
            held: BTreeSet::new(),
            entries: BTreeSet::new(),
            writer_keys: BTreeMap::new(),
            sent: BTreeMap::new(),
        };
        for (envelope, record) in decoded.iter().zip(records) {
            let Some(envelope) = envelope else {
                continue;
            };
            let hash = envelope.hash();
            let held: Option<i64> = sqlx::query_scalar(
                "SELECT 1 FROM sync_auth_envelopes WHERE space_id = ? AND envelope_hash = ? LIMIT 1",
            )
            .bind(space.as_slice())
            .bind(hash.as_slice())
            .fetch_optional(&mut *conn)
            .await?;
            if held.is_some() {
                facts.held.insert(hash);
            }
            if let Some(record) = record {
                let entry = record.hash();
                let stored: Option<i64> = sqlx::query_scalar(
                    "SELECT 1 FROM sync_auth_envelopes
                     WHERE space_id = ? AND entry_hash = ? AND verified <> 0 LIMIT 1",
                )
                .bind(space.as_slice())
                .bind(entry.as_slice())
                .fetch_optional(&mut *conn)
                .await?;
                if stored.is_some() {
                    facts.entries.insert(entry);
                }
            }
            let writer = envelope.writer;
            if !facts.writer_keys.contains_key(&writer)
                && let Some(key) = records::writer_key(conn, ctx.space, writer).await?
            {
                facts.writer_keys.insert(writer, key);
            }
            if let Some(certificate) = certificate_in(envelope) {
                facts.sent.entry(writer).or_default().push(certificate);
            }
        }
        Ok(facts)
    }

    /// The certificates that name a writer key and device key for `writer`: those the set
    /// carries, and the stored ones.
    fn carrier_keys(&self, writer: WriterId, binding: &Binding) -> Vec<CarrierKey> {
        let mut keys = self.sent.get(&writer).cloned().unwrap_or_default();
        if let Some(writer_key) = self.writer_keys.get(&writer) {
            for device_key in binding.certificates.get(&writer).into_iter().flatten() {
                keys.push((*writer_key, *device_key));
            }
        }
        keys
    }
}

/// A row count for a SQL `LIMIT`.
fn row_limit(rows: usize) -> i64 {
    i64::try_from(rows).unwrap_or(i64::MAX)
}

/// Spends the `size` column of `row` from `budget`, returning whether it fit.
fn take(budget: &mut Budget, row: &sqlx::sqlite::SqliteRow, first: bool) -> SyncResult<bool> {
    let size = stored_u64(row.try_get("size")?, "authorization envelope size")?;
    let size = usize::try_from(size).map_err(|_| corrupt("envelope size overflows"))?;
    if !budget.fits(size, first) {
        return Ok(false);
    }
    budget.bytes = budget.bytes.saturating_sub(size);
    budget.envelopes -= 1;
    Ok(true)
}

/// The bytes of a stored envelope.
async fn envelope_bytes(
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    hash: &[u8],
    role: &str,
) -> SyncResult<Vec<u8>> {
    Ok(sqlx::query_scalar(
        "SELECT envelope FROM sync_auth_envelopes
         WHERE space_id = ? AND role = ? AND envelope_hash = ?",
    )
    .bind(ctx.space.as_bytes().as_slice())
    .bind(role)
    .bind(hash)
    .fetch_one(&mut *conn)
    .await?)
}

/// Whether `envelope`, of a kind the store keeps only as fork evidence, shows a fork with what
/// the store already holds: another stored envelope at its writer and sequence, or a stored twin
/// pair at the next sequence of which one continues from it, see the R7.3 A2 rule. Envelopes of
/// the same batch do not count, so a peer cannot store a data envelope by sending its own twin
/// beside it; an authorization record admitted earlier in the batch is stored and counts.
async fn is_evidence(
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    envelope: &Envelope<'_>,
) -> SyncResult<bool> {
    if envelope.space != ctx.space {
        return Ok(false);
    }
    let hash = envelope.hash();
    let twins = envelopes_at(conn, ctx, envelope.writer, envelope.seq).await?;
    if twins.keys().any(|twin| *twin != hash) {
        return Ok(true);
    }
    let Some(next) = envelope.seq.checked_add(1) else {
        return Ok(false);
    };
    let pair = envelopes_at(conn, ctx, envelope.writer, next).await?;
    Ok(pair.len() >= 2 && pair.values().any(|previous| *previous == hash))
}

/// Stored envelope hashes at `writer` and `seq`, each with its previous hash.
async fn envelopes_at(
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    writer: WriterId,
    seq: Seq,
) -> SyncResult<BTreeMap<Digest32, Digest32>> {
    let rows: Vec<(Vec<u8>, Vec<u8>)> = sqlx::query_as(
        "SELECT envelope_hash, previous_hash FROM sync_auth_envelopes
         WHERE space_id = ? AND writer_id = ? AND seq = ?",
    )
    .bind(ctx.space.as_bytes().as_slice())
    .bind(writer.as_bytes().as_slice())
    .bind(seq_i64(seq)?)
    .fetch_all(&mut *conn)
    .await?;
    let mut found = BTreeMap::new();
    for (hash, previous) in rows {
        found.insert(log::digest(&hash)?, log::digest(&previous)?);
    }
    Ok(found)
}

/// Keys of the wanted set read per query while [`wanted_first`] fills its limit.
const WANTED_PAGE: i64 = 512;

/// Up to `limit` hashes and `limit` writers of the wanted set and the refetch hashes, leaving out
/// `skip` before the limit applies: refetch hashes first, then [`priority_keys`] when the rest
/// does not fit, then every other key in kind and key order.
async fn wanted_first(
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    limit: usize,
    skip: &BTreeSet<Vec<u8>>,
) -> SyncResult<RecordWants> {
    let mut wants = RecordWants::default();
    let mut taken = BTreeSet::new();
    let mut offer = |wants: &mut RecordWants, kind: &str, key: Vec<u8>| -> SyncResult<()> {
        if skip.contains(&key) || taken.contains(&key) {
            return Ok(());
        }
        match kind {
            "writer" if wants.writers.len() < limit => wants.writers.push(log::writer_id(&key)?),
            "entry" | "envelope" if wants.hashes.len() < limit => {
                wants.hashes.push(log::digest(&key)?);
            }
            "writer" | "entry" | "envelope" => return Ok(()),
            _ => return Err(corrupt("stored wanted kind is invalid")),
        }
        taken.insert(key);
        Ok(())
    };
    let full = |wants: &RecordWants, kind: &str| match kind {
        "writer" => wants.writers.len() >= limit,
        _ => wants.hashes.len() >= limit,
    };
    let space = ctx.space.as_bytes();
    let mut after = Vec::new();
    while !full(&wants, "envelope") {
        let keys: Vec<Vec<u8>> = sqlx::query_scalar(
            "SELECT envelope_hash FROM sync_auth_refetch
             WHERE space_id = ? AND envelope_hash > ? ORDER BY envelope_hash LIMIT ?",
        )
        .bind(space.as_slice())
        .bind(after.as_slice())
        .bind(WANTED_PAGE)
        .fetch_all(&mut *conn)
        .await?;
        let Some(last) = keys.last().cloned() else {
            break;
        };
        for key in keys {
            offer(&mut wants, "envelope", key)?;
        }
        after = last;
    }
    let total: i64 = sqlx::query_scalar("SELECT count(*) FROM sync_auth_wanted WHERE space_id = ?")
        .bind(space.as_slice())
        .fetch_one(&mut *conn)
        .await?;
    if usize::try_from(total).map_or(true, |total| total > limit) {
        for (kind, key) in priority_keys(conn, ctx).await? {
            let present: Option<i64> = sqlx::query_scalar(
                "SELECT 1 FROM sync_auth_wanted WHERE space_id = ? AND kind = ? AND key = ?",
            )
            .bind(space.as_slice())
            .bind(kind)
            .bind(key.as_slice())
            .fetch_optional(&mut *conn)
            .await?;
            if present.is_some() {
                offer(&mut wants, kind, key)?;
            }
        }
    }
    for kind in ["entry", "envelope", "writer"] {
        let mut after = Vec::new();
        while !full(&wants, kind) {
            let keys: Vec<Vec<u8>> = sqlx::query_scalar(
                "SELECT key FROM sync_auth_wanted
                 WHERE space_id = ? AND kind = ? AND key > ? ORDER BY key LIMIT ?",
            )
            .bind(space.as_slice())
            .bind(kind)
            .bind(after.as_slice())
            .bind(WANTED_PAGE)
            .fetch_all(&mut *conn)
            .await?;
            let Some(last) = keys.last().cloned() else {
                break;
            };
            for key in keys {
                offer(&mut wants, kind, key)?;
            }
            after = last;
        }
    }
    Ok(wants)
}

/// Wanted keys that complete the chains of the most recently stored records the kit key at the
/// local selected head signs and of the most recent yields: the parent entry, writer, and
/// previous envelope of each, then the same of each stored ancestor, within
/// [`PRIORITY_SCAN_ROWS`] reads. Best effort: it orders requests and decides nothing.
async fn priority_keys(
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
) -> SyncResult<Vec<(&'static str, Vec<u8>)>> {
    let Some(stored) = Stored::load(conn, ctx.space).await? else {
        return Ok(Vec::new());
    };
    let resolution = Resolution::new(&stored);
    let kit_key = resolution.walk.visits[resolution.best].kit_key;
    let recent: Vec<Vec<u8>> = sqlx::query_scalar(
        "SELECT envelope FROM sync_auth_envelopes WHERE space_id = ? AND role = 'record'
         ORDER BY stored_order DESC LIMIT ?",
    )
    .bind(ctx.space.as_bytes().as_slice())
    .bind(PRIORITY_SCAN_ROWS)
    .fetch_all(&mut *conn)
    .await?;
    let mut keys = Vec::new();
    let mut chase = Vec::new();
    let note = |keys: &mut Vec<(&'static str, Vec<u8>)>, envelope: &Envelope<'_>| {
        keys.push(("writer", envelope.writer.as_bytes().to_vec()));
        keys.push(("envelope", envelope.previous_hash.to_vec()));
    };
    for bytes in &recent {
        let Ok(envelope) = Envelope::decode(bytes) else {
            continue;
        };
        let Some(record) = record_in(&envelope) else {
            continue;
        };
        let proposal = &record.proposal.proposal;
        if proposal.author == kit_key || matches!(proposal.entry, Entry::Yield(_)) {
            note(&mut keys, &envelope);
            chase.push(record.previous_entry_hash);
        }
    }
    let mut seen = BTreeSet::new();
    let mut reads = 0;
    while let Some(entry) = chase.pop() {
        if reads >= PRIORITY_SCAN_ROWS {
            break;
        }
        if !seen.insert(entry) {
            continue;
        }
        keys.push(("entry", entry.to_vec()));
        reads += 1;
        let bytes: Option<Vec<u8>> = sqlx::query_scalar(
            "SELECT envelope FROM sync_auth_envelopes WHERE space_id = ? AND entry_hash = ?
             ORDER BY verified DESC, envelope_hash LIMIT 1",
        )
        .bind(ctx.space.as_bytes().as_slice())
        .bind(entry.as_slice())
        .fetch_optional(&mut *conn)
        .await?;
        let Some(envelope) = bytes
            .as_deref()
            .and_then(|bytes| Envelope::decode(bytes).ok())
        else {
            continue;
        };
        if let Some(record) = record_in(&envelope) {
            note(&mut keys, &envelope);
            chase.push(record.previous_entry_hash);
        }
    }
    Ok(keys)
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
