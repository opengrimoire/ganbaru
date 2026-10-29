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
    /// Indexes of the data envelopes of the space in the set that the store does not hold
    /// verified afterwards. A fork pair whose halves arrive in different sets pairs once a
    /// later set carries them again together with the other half.
    pub unpaired: Vec<usize>,
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
    /// Keys of the wanted set among those of the envelopes being classed: entry and envelope
    /// hashes, and writers. Empty outside [`Engine::record_classes`], which reads it per key so
    /// the cost of a set does not grow with the wanted set.
    wanted: BTreeSet<Vec<u8>>,
    /// What binds records the kit key signs to their carriers, `None` without a charter.
    binding: Option<Binding>,
    /// Revisions and entry hashes of the stored records whose chains [`priority_keys`] completes
    /// first: the records the kit key signs that bound as class 1, and, when asked for, the
    /// yields its parent on the selected chain admits whose author is an admin at the head.
    priority: Vec<(u64, Digest32)>,
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
    /// The entry that opened the state's epoch instance: the charter hash, or a handover.
    instance: Digest32,
    /// Whether the state is the local selected head.
    head: bool,
}

/// How a carrier of a record the kit key signs must be certified, see [`Binding::keys`].
struct Bound {
    /// The device keys a carrier's certificate must name.
    keys: BTreeSet<DevicePublicKey>,
    /// Whether a carrier those keys certify makes the record class 1. Otherwise it only spends
    /// the record's slots.
    kit: bool,
    /// Where the record spends its proposal slot, `None` when its parent is not read.
    instance: Option<SlotScope>,
}

/// Where a bound record spends the slot of its proposal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum SlotScope {
    /// Inside the epoch instance a kit handover opened, which the handover names. Kit handovers
    /// bound how many such instances there are.
    Kit(Digest32),
    /// Anywhere outside such instances: one slot per proposal in total, so a chain of takes that
    /// opens new epoch instances cannot mint new slots.
    Outside,
}

impl SlotScope {
    /// The scope of a record whose parent reads `parent`.
    fn at(parent: &BoundState) -> Self {
        if parent.kit_instance {
            Self::Kit(parent.instance)
        } else {
            Self::Outside
        }
    }
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
/// - Any other record: the key of the anchor device at the record's parent, and only at a
///   parent inside an epoch instance a kit handover opened, or at the local selected head. The
///   kit seals its other records there, and a device that was anchor at an older parent cannot
///   rewrap them at its own historical parents.
///
/// The parent of an enrollment or any other record must be on the local selected chain, or be
/// reached from it through bound records of an epoch instance a kit handover opened, and the
/// record must pass its rules there. Otherwise nothing binds it yet and it takes the sender's
/// class, which only delays it.
///
/// A record whose carrier the parent's key certifies spends its slots even where it is not
/// class 1: its entry, its parent and proposal pair, and its proposal slot: one per epoch
/// instance a kit handover opened, and one in total outside them ([`SlotScope`]), which goes to
/// the first stored record that binds there, or else the first one a set sends.
/// Each slot takes one entry, so another entry that needs a spent slot is not class 1. The
/// slots are derived from every verified stored record, nodes or not, so a slot stays spent as
/// the store grows; only a change of the local selected chain can read them anew.
struct Binding {
    kit_key: AdminPublicKey,
    /// The state of the local selected head.
    head: AuthState,
    /// States by entry hash: those the local selected chain reaches and records need, and those
    /// bound records extend.
    states: BTreeMap<Digest32, BoundState>,
    /// Device keys stored enrollments the kit key signs name, by device id.
    enrolled: BTreeMap<String, BTreeSet<DevicePublicKey>>,
    /// Device keys each stored certificate of a writer names.
    certificates: BTreeMap<WriterId, Vec<DevicePublicKey>>,
    /// Entries whose slots are spent.
    bound_entries: BTreeSet<Digest32>,
    /// Spent parent and proposal hash pairs.
    bound_pairs: BTreeSet<(Digest32, Digest32)>,
    /// Spent proposal slots, by scope and proposal hash.
    bound_instances: BTreeSet<(SlotScope, Digest32)>,
}

impl Binding {
    /// How a carrier of `record` must be certified, `None` when the kit key does not sign it or
    /// nothing binds it yet.
    fn keys(&self, record: &AuthorizationRecord) -> Option<Bound> {
        let proposal = &record.proposal.proposal;
        if proposal.author != self.kit_key {
            return None;
        }
        let Some(parent) = self.states.get(&record.previous_entry_hash) else {
            return match &proposal.entry {
                Entry::Handover(handover) => Some(Bound {
                    keys: self.enrolled.get(&handover.new_anchor_device_id)?.clone(),
                    kit: true,
                    instance: None,
                }),
                _ => None,
            };
        };
        let state = &parent.state;
        if state.check(record).is_err() {
            return None;
        }
        let (device, kit) = match &proposal.entry {
            Entry::Enroll(enroll) => {
                return Some(Bound {
                    keys: BTreeSet::from([enroll.device_key]),
                    kit: true,
                    instance: Some(SlotScope::at(parent)),
                });
            }
            Entry::Handover(handover) => (state.devices.get(&handover.new_anchor_device_id), true),
            _ => (
                state.devices.get(&state.anchor_device_id),
                parent.kit_instance || parent.head,
            ),
        };
        Some(Bound {
            keys: device.map(|device| device.device_key).into_iter().collect(),
            kit,
            instance: Some(SlotScope::at(parent)),
        })
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
            next.instance = hash;
        }
        next.head = false;
        next.state.advance(record, hash);
        self.states.entry(hash).or_insert(next);
    }

    /// Spends the slots of a record bound at `instance`, returning whether none was spent.
    /// `outside` names the entry the [`SlotScope::Outside`] slot of the record's proposal goes
    /// to, the first stored or first sent; another entry neither takes nor spends that slot.
    fn spend(
        &mut self,
        record: &AuthorizationRecord,
        hash: Digest32,
        instance: Option<SlotScope>,
        outside: Option<Digest32>,
    ) -> bool {
        let proposal = record.proposal.hash();
        let pair = (record.previous_entry_hash, proposal);
        let owner =
            instance != Some(SlotScope::Outside) || outside.is_none_or(|first| first == hash);
        let in_instance = instance
            .filter(|_| owner)
            .map(|instance| (instance, proposal));
        let fresh = owner
            && !self.bound_entries.contains(&hash)
            && !self.bound_pairs.contains(&pair)
            && in_instance.is_none_or(|slot| !self.bound_instances.contains(&slot));
        self.bound_entries.insert(hash);
        self.bound_pairs.insert(pair);
        if let Some(slot) = in_instance {
            self.bound_instances.insert(slot);
        }
        fresh
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
        let classes = classes(conn, ctx, &BTreeSet::new(), false).await?;
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

    /// Up to `limit` hashes and `limit` writers of what the record store lacks and of the refetch
    /// hashes, leaving out the keys in `skip` before the limit applies. When they do not fit, the
    /// missing ancestors of stored records the kit key binds as class 1 and of stored yields an
    /// admin at the selected head signs come first, then the rest in key order from the lowest key.
    pub async fn record_wants(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        limit: usize,
        skip: &BTreeSet<Vec<u8>>,
    ) -> SyncResult<RecordWants> {
        Ok(self
            .record_wants_after(conn, ctx, limit, skip, &[], true)
            .await?
            .0)
    }

    /// [`Self::record_wants`] with the rotation through the rest starting after the key `after`
    /// and wrapping around, and the key the next request continues after. A caller that passes
    /// the returned key back once a peer answered the request asks every wanted key and refetch
    /// hash within a number of answered requests bounded by their count over half of `limit`.
    /// The leading ancestors are read only when `lead` is set, since reading them walks the
    /// record store: a caller sets it for its first request and after one that stored records.
    pub async fn record_wants_after(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        limit: usize,
        skip: &BTreeSet<Vec<u8>>,
        after: &[u8],
        lead: bool,
    ) -> SyncResult<(RecordWants, Vec<u8>)> {
        let mut tx = conn.begin().await?;
        log::ensure_space(&mut tx, ctx.space).await?;
        refresh(&mut tx, ctx).await?;
        let wants = wanted_first(&mut tx, ctx, limit, skip, after, lead).await?;
        tx.commit().await?;
        Ok(wants)
    }

    /// Admits `envelopes` to the record store: records and certificates by their intrinsic
    /// checks, then the lowest data fork pair of each writer the batch carries (see
    /// [`fork_pairs`]), then any other envelope as fork evidence only when it is a twin of a
    /// stored envelope at its writer and sequence, or the envelope a stored twin pair continues
    /// from. Returns how many it did not hold, the part of its wanted set those envelopes
    /// reference, and the data envelopes it does not hold, which a caller may add to a later
    /// batch from the same peer so a fork pair split across pages still pairs. The next apply
    /// selects again.
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
        let mut note = |envelope: &Envelope<'_>| {
            admitted += 1;
            referenced.insert(envelope.previous_hash.to_vec());
            referenced.insert(envelope.writer.as_bytes().to_vec());
            if let Some(record) = record_in(envelope) {
                referenced.insert(record.previous_entry_hash.to_vec());
            }
        };
        let mut data = Vec::new();
        for (bytes, envelope) in &decoded {
            match OperationKind::from_u8(envelope.kind) {
                Some(OperationKind::Genesis | OperationKind::Authorization) => {
                    if records::admit(&mut tx, ctx.space, bytes, now_ms).await? {
                        note(envelope);
                    }
                }
                _ => data.push((*bytes, envelope)),
            }
        }
        let paired = fork_pairs(&mut tx, ctx, &data).await?;
        // The two halves of a pair take consecutive stored orders, whatever their batch order,
        // so a page of the store splits at most the one pair at its boundary.
        let mut halves: Vec<&(&[u8], &Envelope<'_>)> = data
            .iter()
            .filter(|(_, envelope)| paired.contains(&envelope.hash()))
            .collect();
        halves.sort_by_key(|(_, envelope)| (envelope.writer, envelope.seq, envelope.hash()));
        for (bytes, envelope) in halves {
            if records::admit_evidence(&mut tx, ctx.space, &[bytes], now_ms).await? {
                note(envelope);
            }
        }
        for (bytes, envelope) in &data {
            if !paired.contains(&envelope.hash())
                && is_evidence(&mut tx, ctx, envelope).await?
                && records::admit_evidence(&mut tx, ctx.space, &[bytes], now_ms).await?
            {
                note(envelope);
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
        let mut unpaired = Vec::new();
        for (index, bytes) in envelopes.iter().enumerate() {
            let Ok(envelope) = Envelope::decode(bytes) else {
                continue;
            };
            let data = !matches!(
                OperationKind::from_u8(envelope.kind),
                Some(OperationKind::Genesis | OperationKind::Authorization)
            );
            if data
                && envelope.space == ctx.space
                && !holds_verified(&mut tx, ctx, envelope.hash()).await?
            {
                unpaired.push(index);
            }
        }
        tx.commit().await?;
        Ok(RecordAdmission {
            admitted,
            missing,
            unpaired,
        })
    }

    /// The class each of `envelopes` has at the local selected head, for ingress budgets.
    ///
    /// An envelope the store holds, or one sent twice, is [`RecordClass::Rest`], and so is a
    /// record whose entry the store holds verified. A record the kit key signs is class 1 only
    /// when [`Binding`] binds its carrier, and then only when none of its slots is spent by the
    /// store or an earlier record of the set; otherwise it takes the class any other record of
    /// its sender would. A
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
        let mut classes = classes(conn, ctx, &needed, false).await?;
        for (envelope, record) in decoded.iter().zip(&records) {
            let Some(envelope) = envelope else {
                continue;
            };
            let mut keys = vec![(WantedKind::Envelope, envelope.hash().to_vec())];
            if OperationKind::from_u8(envelope.kind) == Some(OperationKind::Genesis) {
                keys.push((WantedKind::Writer, envelope.writer.as_bytes().to_vec()));
            }
            if let Some(record) = record {
                keys.push((WantedKind::Entry, record.hash().to_vec()));
            }
            for (kind, key) in keys {
                if !classes.wanted.contains(&key) && is_wanted(conn, ctx, kind, &key).await? {
                    classes.wanted.insert(key);
                }
            }
        }
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
            // The Outside slot of a proposal goes to the first record of the set that binds
            // there, as it went to the first stored one, so a rewrap at an older parent sent
            // behind the record the kit sealed at the head does not take it.
            let mut outside: BTreeMap<Digest32, Digest32> = BTreeMap::new();
            for &index in &order {
                let (Some(envelope), Some(record)) = (&decoded[index], &records[index]) else {
                    continue;
                };
                if facts.held.contains(&envelope.hash()) || facts.entries.contains(&record.hash()) {
                    continue;
                }
                let Some(bound) = binding.keys(record) else {
                    continue;
                };
                let certified = bound.instance == Some(SlotScope::Outside)
                    && facts.carrier_keys(envelope.writer, binding).iter().any(
                        |(writer_key, device_key)| {
                            bound.keys.contains(device_key) && envelope.verify(writer_key)
                        },
                    );
                if certified {
                    outside
                        .entry(record.proposal.hash())
                        .or_insert_with(|| record.hash());
                }
            }
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
                let Some(bound) = binding.keys(record) else {
                    continue;
                };
                let certified = facts.carrier_keys(envelope.writer, binding).iter().any(
                    |(writer_key, device_key)| {
                        bound.keys.contains(device_key) && envelope.verify(writer_key)
                    },
                );
                let first = outside.get(&record.proposal.hash()).copied();
                if certified && binding.spend(record, entry, bound.instance, first) && bound.kit {
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
/// read, the parents of records a peer sends. With `yields` set, the stored yields whose parent
/// on the selected chain admits them and whose author is an admin at the selected head join
/// [`Classes::priority`].
async fn classes(
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    needed: &BTreeSet<Digest32>,
    yields: bool,
) -> SyncResult<Classes> {
    let mut classes = Classes::default();
    let Some(stored) = Stored::load(conn, ctx.space).await? else {
        return Ok(classes);
    };
    let resolution = Resolution::new(&stored);
    let walk = &resolution.walk;
    let is_yield =
        |node: &&records::Node| matches!(node.record.proposal.proposal.entry, Entry::Yield(_));
    let stored_yields: Vec<&records::Node> = if yields {
        stored
            .nodes
            .iter()
            .chain(&stored.others)
            .filter(is_yield)
            .collect()
    } else {
        Vec::new()
    };
    let mut needed = needed.clone();
    needed.extend(
        stored_yields
            .iter()
            .map(|node| node.record.previous_entry_hash),
    );
    let mut binding = binding(&stored, &resolution, &needed);
    for node in stored_yields {
        if binding.admits(&node.record)
            && binding.head.is_admin(&node.record.proposal.proposal.author)
        {
            classes.priority.push((node.record.revision, node.hash));
        }
    }
    let visits: BTreeMap<Digest32, usize> = walk
        .visits
        .iter()
        .enumerate()
        .skip(1)
        .map(|(index, visit)| (visit.hash, index))
        .collect();
    let bound_writers = |visit: usize| walk.visits[visit].bound.iter().map(|(writer, _)| *writer);
    // Every verified stored record the kit key signs, nodes or not, in revision order so a bound
    // record's state is there for its children and the slots it spends are spent before a later
    // record needs them, and the walked chain each node continues from.
    let mut signed: Vec<&records::Node> = stored
        .nodes
        .iter()
        .chain(&stored.others)
        .filter(|node| node.record.proposal.proposal.author == binding.kit_key)
        .collect();
    signed.sort_by_key(|node| (node.record.revision, node.hash));
    let outside = outside_owners(conn, ctx, &binding, &signed).await?;
    for node in signed {
        let Some(slot) = binding.keys(&node.record) else {
            continue;
        };
        let bound: BTreeSet<WriterId> = bound_carriers(&binding, node, &slot)
            .map(|carrier| carrier.writer)
            .collect();
        let first = outside.get(&node.record.proposal.hash()).copied();
        if bound.is_empty()
            || !binding.spend(&node.record, node.hash, slot.instance, first)
            || !slot.kit
        {
            continue;
        }
        binding.extend(&node.record, node.hash);
        classes.kit.entry(node.hash).or_default().extend(bound);
        classes.priority.push((node.record.revision, node.hash));
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

/// The carriers of `node` whose writer's certificate names a key `slot` binds.
fn bound_carriers<'a>(
    binding: &'a Binding,
    node: &'a records::Node,
    slot: &'a Bound,
) -> impl Iterator<Item = &'a records::Carrier> {
    node.carriers.iter().filter(|carrier| {
        binding
            .certificates
            .get(&carrier.writer)
            .is_some_and(|named| named.iter().any(|key| slot.keys.contains(key)))
    })
}

/// The entry each proposal's [`SlotScope::Outside`] slot goes to among the stored records in
/// `signed` that bind there: the one with the first stored bound carrier. Stored order is
/// local, which classes may read since they only order transport, and it lets a record the kit
/// sealed at the head keep its slot against a rewrap at an older parent stored after it.
async fn outside_owners(
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    binding: &Binding,
    signed: &[&records::Node],
) -> SyncResult<BTreeMap<Digest32, Digest32>> {
    let mut first: BTreeMap<Digest32, (i64, Digest32)> = BTreeMap::new();
    for node in signed {
        let Some(slot) = binding.keys(&node.record) else {
            continue;
        };
        if slot.instance != Some(SlotScope::Outside) {
            continue;
        }
        let mut order: Option<i64> = None;
        for carrier in bound_carriers(binding, node, &slot) {
            let stored: Option<i64> = sqlx::query_scalar(
                "SELECT stored_order FROM sync_auth_envelopes
                 WHERE space_id = ? AND envelope_hash = ? AND verified = 1",
            )
            .bind(ctx.space.as_bytes().as_slice())
            .bind(carrier.envelope_hash.as_slice())
            .fetch_optional(&mut *conn)
            .await?;
            order = match (order, stored) {
                (Some(order), Some(stored)) => Some(order.min(stored)),
                (order, stored) => order.or(stored),
            };
        }
        let Some(order) = order else {
            continue;
        };
        let candidate = (order, node.hash);
        first
            .entry(node.record.proposal.hash())
            .and_modify(|best| *best = (*best).min(candidate))
            .or_insert(candidate);
    }
    Ok(first
        .into_iter()
        .map(|(proposal, (_, entry))| (proposal, entry))
        .collect())
}

/// The kinds of key the wanted set holds.
#[derive(Clone, Copy)]
enum WantedKind {
    Entry,
    Envelope,
    Writer,
}

impl WantedKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Entry => "entry",
            Self::Envelope => "envelope",
            Self::Writer => "writer",
        }
    }
}

/// Whether the wanted set holds `key` of `kind`, an envelope hash also counting when a
/// refetch names it.
async fn is_wanted(
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    kind: WantedKind,
    key: &[u8],
) -> SyncResult<bool> {
    Ok(sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM sync_auth_wanted
                        WHERE space_id = ?1 AND kind = ?2 AND key = ?3)
             OR (?2 = 'envelope'
                 AND EXISTS (SELECT 1 FROM sync_auth_refetch
                             WHERE space_id = ?1 AND envelope_hash = ?3))",
    )
    .bind(ctx.space.as_bytes().as_slice())
    .bind(kind.as_str())
    .bind(key)
    .fetch_one(&mut *conn)
    .await?)
}

/// The binding at the local selected head: the states of the selected chain at the parents of
/// stored records the kit key signs and at `needed`, and the enrollments the kit key signs.
fn binding(stored: &Stored, resolution: &Resolution<'_>, needed: &BTreeSet<Digest32>) -> Binding {
    let walk = &resolution.walk;
    let kit_key = walk.visits[resolution.best].kit_key;
    let mut at_hashes = needed.clone();
    let mut enrolled: BTreeMap<String, BTreeSet<DevicePublicKey>> = BTreeMap::new();
    for node in stored.nodes.iter().chain(&stored.others) {
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
    let mut instance = state.entry_hash;
    let mut states = BTreeMap::new();
    if at_hashes.contains(&state.entry_hash) {
        states.insert(
            state.entry_hash,
            BoundState {
                state: state.clone(),
                kit_instance,
                instance,
                head: resolution.best == 0,
            },
        );
    }
    for index in path.into_iter().rev() {
        let visit = &walk.visits[index];
        let Some(node) = visit.node else {
            continue;
        };
        if let Some((signer, _)) = &visit.handover {
            kit_instance = *signer == Signer::Kit;
            instance = visit.hash;
        }
        state.advance(&stored.nodes[node].record, visit.hash);
        if at_hashes.contains(&visit.hash) {
            states.insert(
                visit.hash,
                BoundState {
                    state: state.clone(),
                    kit_instance,
                    instance,
                    head: index == resolution.best,
                },
            );
        }
    }
    Binding {
        kit_key,
        head: state,
        states,
        enrolled,
        certificates: stored
            .certificates
            .iter()
            .map(|(writer, names)| (*writer, names.iter().map(|(_, key)| *key).collect()))
            .collect(),
        bound_entries: BTreeSet::new(),
        bound_pairs: BTreeSet::new(),
        bound_instances: BTreeSet::new(),
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
/// the same batch do not count, except through [`fork_pairs`]; the authorization records of the
/// batch are stored first and count. Only verified envelopes count, so a pending forgery of a
/// writer without a stored certificate makes nothing evidence.
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

/// Hashes of the data envelopes among `data` that form the lowest fork pair of their writer.
///
/// Two distinct data envelopes of one batch at one writer and sequence, both signed by the key
/// a stored certificate of the writer certifies, prove the writer forked, so a peer the
/// operation lanes do not reach still learns the fork a member's store holds. Only the lowest
/// such sequence of each writer counts, and only when it is below every sequence at which the
/// store already holds two verified envelopes of the writer, since only the lowest pair sets
/// the writer's horizon; of more than two envelopes there, the two lowest hashes count. Each
/// pair admitted lowers that bound, so a writer adds a pair only below its last one.
async fn fork_pairs(
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    data: &[(&[u8], &Envelope<'_>)],
) -> SyncResult<BTreeSet<Digest32>> {
    let mut by_slot: BTreeMap<WriterId, BTreeMap<Seq, BTreeSet<Digest32>>> = BTreeMap::new();
    for (_, envelope) in data {
        if envelope.space == ctx.space {
            by_slot
                .entry(envelope.writer)
                .or_default()
                .entry(envelope.seq)
                .or_default()
                .insert(envelope.hash());
        }
    }
    let mut paired = BTreeSet::new();
    for (writer, slots) in by_slot {
        let Some((seq, _)) = slots.iter().find(|(_, hashes)| hashes.len() >= 2) else {
            continue;
        };
        let lowest: Option<i64> = sqlx::query_scalar(
            "SELECT min(seq) FROM (SELECT seq FROM sync_auth_envelopes
               WHERE space_id = ? AND writer_id = ? AND verified = 1
               GROUP BY seq HAVING count(DISTINCT envelope_hash) >= 2)",
        )
        .bind(ctx.space.as_bytes().as_slice())
        .bind(writer.as_bytes().as_slice())
        .fetch_one(&mut *conn)
        .await?;
        if lowest.is_some_and(|lowest| seq_i64(*seq).is_ok_and(|seq| seq >= lowest)) {
            continue;
        }
        let Some(key) = records::writer_key(conn, ctx.space, writer).await? else {
            continue;
        };
        let signed: Vec<Digest32> = data
            .iter()
            .filter(|(_, envelope)| envelope.writer == writer && envelope.seq == *seq)
            .filter(|(_, envelope)| envelope.verify(&key))
            .map(|(_, envelope)| envelope.hash())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .take(2)
            .collect();
        if signed.len() == 2 {
            paired.extend(signed);
        }
    }
    Ok(paired)
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
         WHERE space_id = ? AND writer_id = ? AND seq = ? AND verified = 1",
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
/// `skip` before the limit applies, and the key the next selection continues after.
///
/// When the wanted set does not fit and `lead` is set, the [`priority_keys`] come first, up to
/// half of each limit.
/// The rest is one rotation through the wanted keys and refetch hashes in key order, starting
/// after `after` and wrapping around once, which stops when either limit is full. A caller that
/// keeps the returned key and passes it back once the request was answered asks every key
/// within a number of requests bounded by the size of the wanted set over half the limit,
/// whatever keys sort low.
async fn wanted_first(
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    limit: usize,
    skip: &BTreeSet<Vec<u8>>,
    after: &[u8],
    lead: bool,
) -> SyncResult<(RecordWants, Vec<u8>)> {
    let mut wants = RecordWants::default();
    let mut taken = BTreeSet::new();
    let mut offer =
        |wants: &mut RecordWants, kind: &str, key: Vec<u8>, cap: usize| -> SyncResult<()> {
            if skip.contains(&key) || taken.contains(&key) {
                return Ok(());
            }
            match kind {
                "writer" if wants.writers.len() < cap => wants.writers.push(log::writer_id(&key)?),
                "entry" | "envelope" if wants.hashes.len() < cap => {
                    wants.hashes.push(log::digest(&key)?);
                }
                "writer" | "entry" | "envelope" => return Ok(()),
                _ => return Err(corrupt("stored wanted kind is invalid")),
            }
            taken.insert(key);
            Ok(())
        };
    let full = |wants: &RecordWants| wants.writers.len() >= limit || wants.hashes.len() >= limit;
    let space = ctx.space.as_bytes();
    let total: i64 = sqlx::query_scalar(
        "SELECT (SELECT count(*) FROM sync_auth_wanted WHERE space_id = ?1)
              + (SELECT count(*) FROM sync_auth_refetch WHERE space_id = ?1)",
    )
    .bind(space.as_slice())
    .fetch_one(&mut *conn)
    .await?;
    if lead && usize::try_from(total).map_or(true, |total| total > limit) {
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
                offer(&mut wants, kind, key, limit / 2)?;
            }
        }
    }
    let mut cursor = after.to_vec();
    let mut wrapped = false;
    'rotation: while !full(&wants) {
        let upper: Option<&[u8]> = wrapped.then_some(after);
        let keys = wanted_page(conn, ctx, &cursor, upper).await?;
        if keys.is_empty() {
            if wrapped || after.is_empty() {
                break;
            }
            wrapped = true;
            cursor.clear();
            continue;
        }
        for (kind, key) in keys {
            if full(&wants) {
                break 'rotation;
            }
            cursor.clone_from(&key);
            offer(&mut wants, &kind, key, limit)?;
        }
    }
    Ok((wants, cursor))
}

/// Kinds of the wanted set, each read through its own primary key range.
const WANTED_KINDS: [&str; 3] = ["entry", "envelope", "writer"];

/// Up to [`WANTED_PAGE`] wanted keys and refetch hashes past `after`, and at most `upper` when
/// it is set, in key order. Each kind and the refetch hashes are read in key order through
/// their primary keys and merged here, so no read sorts the whole set.
async fn wanted_page(
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    after: &[u8],
    upper: Option<&[u8]>,
) -> SyncResult<Vec<(String, Vec<u8>)>> {
    let space = ctx.space.as_bytes();
    let mut keys: Vec<(String, Vec<u8>)> = Vec::new();
    for kind in WANTED_KINDS {
        let page: Vec<Vec<u8>> = sqlx::query_scalar(
            "SELECT key FROM sync_auth_wanted
             WHERE space_id = ?1 AND kind = ?2 AND key > ?3 AND (?4 IS NULL OR key <= ?4)
             ORDER BY key LIMIT ?5",
        )
        .bind(space.as_slice())
        .bind(kind)
        .bind(after)
        .bind(upper)
        .bind(WANTED_PAGE)
        .fetch_all(&mut *conn)
        .await?;
        keys.extend(page.into_iter().map(|key| (kind.to_string(), key)));
    }
    let refetch: Vec<Vec<u8>> = sqlx::query_scalar(
        "SELECT envelope_hash FROM sync_auth_refetch
         WHERE space_id = ?1 AND envelope_hash > ?2 AND (?3 IS NULL OR envelope_hash <= ?3)
         ORDER BY envelope_hash LIMIT ?4",
    )
    .bind(space.as_slice())
    .bind(after)
    .bind(upper)
    .bind(WANTED_PAGE)
    .fetch_all(&mut *conn)
    .await?;
    keys.extend(refetch.into_iter().map(|key| ("envelope".to_string(), key)));
    keys.sort_by(|a, b| (&a.1, &a.0).cmp(&(&b.1, &b.0)));
    keys.truncate(usize::try_from(WANTED_PAGE).unwrap_or(usize::MAX));
    Ok(keys)
}

/// Wanted keys that complete the chains of the stored records the kit key at the local selected
/// head signs that bound as class 1, and of the stored yields their parent on the selected chain
/// admits and an admin at the selected head signs, newest first: the parent entry, writer, and
/// previous envelope of each, then the same of each stored ancestor, within
/// [`PRIORITY_SCAN_ROWS`] reads. Records that bound nothing and yields of keys that are not
/// admins at the head never lead. Best effort: it orders requests and decides nothing.
async fn priority_keys(
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
) -> SyncResult<Vec<(&'static str, Vec<u8>)>> {
    let mut seeds = classes(conn, ctx, &BTreeSet::new(), true).await?.priority;
    seeds.sort_by(|a, b| b.cmp(a));
    let mut chase: Vec<Digest32> = seeds.into_iter().rev().map(|(_, entry)| entry).collect();
    let mut keys = Vec::new();
    let mut seen = BTreeSet::new();
    let mut reads = 0;
    // Seeds are stored, so each read finds its envelope; a missing ancestor is a wanted entry.
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
            keys.push(("writer", envelope.writer.as_bytes().to_vec()));
            keys.push(("envelope", envelope.previous_hash.to_vec()));
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

/// Whether the store holds a verified envelope with `hash`.
async fn holds_verified(
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    hash: Digest32,
) -> SyncResult<bool> {
    let held: Option<i64> = sqlx::query_scalar(
        "SELECT 1 FROM sync_auth_envelopes
         WHERE space_id = ? AND envelope_hash = ? AND verified = 1 LIMIT 1",
    )
    .bind(ctx.space.as_bytes().as_slice())
    .bind(hash.as_slice())
    .fetch_optional(&mut *conn)
    .await?;
    Ok(held.is_some())
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
