//! Simulated installations of one vault: an in-memory database, an engine for the vault and
//! fixture domains, and the local writer, with helpers that mirror what the app does between
//! engine calls.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Mutex, OnceLock};

use ganbaru_contacts::PersonKeyPair;
use ganbaru_sync_contracts::{
    AdminKeyPair, ChangeAction, Charter, Content, DeviceKeyPair, DeviceKind, DeviceRole, Enroll,
    Entry, Envelope, GroupId, Proposal, ProposalRejection, Remove, SignedCertificate,
    SignedCharter, SignedProposal, SpaceId, TableId, Value, VersionVector, WriterCertificate,
    WriterId, WriterKeyPair, sign_certificate, sign_charter, sign_proposal,
};
use sqlx::sqlite::SqliteOwnedBuf;
use sqlx::{AssertSqlSafe, Connection, SqliteConnection};

use super::fixture::{NODE_MAX_DEPTH, after_local_edit, fixture_pool, test_engine};
use crate::manifest::Manifest;
use crate::{
    ApplyReport, CompactionReport, Engine, LocalWriter, SealReport, SequenceReservation,
    SpaceContext, StoreOutcome, SyncResult,
};
use crate::{hydrate, log, publication};

/// Vault id every replica shares.
pub(super) const VAULT_ID: &str = "vault-sim";
/// Simulated time every replica starts from, in Unix milliseconds.
pub(super) const START_MS: u64 = 1_800_000_000_000;

const PERSON_SEED: [u8; 32] = [7; 32];

/// The vault owner's person key.
pub(super) fn person() -> PersonKeyPair {
    PersonKeyPair::from_seed(&PERSON_SEED).expect("test person seed should be valid")
}

/// Database bytes of a migrated vault with the fixture domain, built once per test binary.
static TEMPLATE: OnceLock<Vec<u8>> = OnceLock::new();

async fn template() -> &'static [u8] {
    if let Some(bytes) = TEMPLATE.get() {
        return bytes;
    }
    let pool = fixture_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    let bytes = conn.serialize(None).await.unwrap().to_vec();
    drop(conn);
    pool.close().await;
    TEMPLATE.get_or_init(|| bytes)
}

/// An in-memory connection holding a copy of `bytes`, with foreign keys on like vault pools.
pub(super) async fn connection_from(bytes: &[u8]) -> SqliteConnection {
    let mut conn = SqliteConnection::connect("sqlite::memory:").await.unwrap();
    conn.deserialize(None, SqliteOwnedBuf::try_from(bytes).unwrap(), false)
        .await
        .unwrap();
    sqlx::raw_sql("PRAGMA foreign_keys=ON")
        .execute(&mut conn)
        .await
        .unwrap();
    conn
}

/// What a replica retained and stored before a seal, to check what the seal wrote.
pub(super) struct SealSnapshot {
    /// Retained value hashes of each captured row, by table, row key, and group.
    retained: BTreeMap<(i64, String, i64), BTreeSet<Vec<u8>>>,
    /// Forced group mask of each captured row, with every group set for a derived-key row
    /// inserted again, which seals whole unless every group is unchanged.
    forced: BTreeMap<(i64, String), i64>,
    /// Stored operations by writer id and sequence.
    ops: BTreeSet<(Vec<u8>, i64)>,
}

/// A sequence reservation kept in memory; it survives rolled-back transactions like a durable
/// writer record does.
#[derive(Debug, Default, Clone)]
pub(super) struct Reservation {
    pub(super) reserved: u64,
}

impl SequenceReservation for Reservation {
    fn ensure_reserved(&mut self, seq: u64) -> std::io::Result<()> {
        self.reserved = self.reserved.max(seq);
        Ok(())
    }
}

/// Device id of the anchor every test space is chartered with.
pub(super) const ANCHOR_DEVICE_ID: &str = "device-anchor";
/// Devices `device_id(0)` through `device_id(ENROLLED_DEVICES - 1)` are enrolled in every test
/// space before any replica starts.
pub(super) const ENROLLED_DEVICES: usize = 16;
/// Purposes a test key seed is derived for.
const DEVICE_PURPOSE: u8 = 1;
const ADMIN_PURPOSE: u8 = 2;
const RECOVERY_PURPOSE: u8 = 3;
const AUTHORITY_PURPOSE: u8 = 4;
/// Index the anchor device's keys derive from, outside the enrolled range.
const ANCHOR_KEY_INDEX: u64 = u64::MAX;

/// A deterministic seed for a key of `person`'s spaces, distinct per purpose and index.
fn key_seed(person: &PersonKeyPair, purpose: u8, index: u64) -> [u8; 32] {
    let mut seed = *person.public_key().as_bytes();
    seed[0] ^= purpose;
    for (byte, index_byte) in seed[1..9].iter_mut().zip(index.to_le_bytes()) {
        *byte ^= index_byte;
    }
    seed
}

/// The device key of replica device `index` in `person`'s spaces.
pub(super) fn device_key(person: &PersonKeyPair, index: usize) -> DeviceKeyPair {
    DeviceKeyPair::from_seed(&key_seed(person, DEVICE_PURPOSE, index as u64))
        .expect("test device seed should be valid")
}

fn anchor_device_key(person: &PersonKeyPair) -> DeviceKeyPair {
    DeviceKeyPair::from_seed(&key_seed(person, DEVICE_PURPOSE, ANCHOR_KEY_INDEX))
        .expect("test device seed should be valid")
}

/// The admin key the anchor signs proposals with in `person`'s spaces.
pub(super) fn anchor_admin(person: &PersonKeyPair) -> AdminKeyPair {
    AdminKeyPair::from_seed(&key_seed(person, ADMIN_PURPOSE, ANCHOR_KEY_INDEX))
        .expect("test admin seed should be valid")
}

/// The recovery admin key of `person`'s spaces.
pub(super) fn recovery_admin(person: &PersonKeyPair) -> AdminKeyPair {
    AdminKeyPair::from_seed(&key_seed(person, RECOVERY_PURPOSE, 0))
        .expect("test admin seed should be valid")
}

/// The charter of `space`, owned by `person`, with the anchor device as first anchor.
pub(super) fn test_charter(person: &PersonKeyPair, space: SpaceId) -> SignedCharter {
    sign_charter(
        &Charter {
            space,
            anchor_device_id: ANCHOR_DEVICE_ID.to_owned(),
            anchor_device_key: anchor_device_key(person).public_key(),
            anchor_tls_fingerprint: [1; 32],
            anchor_admin_key: anchor_admin(person).public_key(),
            recovery_admin_key: recovery_admin(person).public_key(),
            discovery_key: [2; 32],
            created_at_ms: START_MS as i64,
        },
        person,
    )
    .expect("test charter should sign")
}

/// A proposal of `entry` for `space` signed by `admin`.
pub(super) fn proposal(
    space: SpaceId,
    admin: &AdminKeyPair,
    created_at_ms: u64,
    entry: Entry,
) -> SignedProposal {
    sign_proposal(
        &Proposal {
            space,
            author: admin.public_key(),
            created_at_ms: created_at_ms as i64,
            entry,
        },
        admin,
    )
    .expect("test proposal should sign")
}

/// An enrollment of replica device `index` as a desktop with `role`.
pub(super) fn enroll_entry(person: &PersonKeyPair, index: usize, role: DeviceRole) -> Entry {
    Entry::Enroll(Enroll {
        device_id: device_id(index),
        device_key: device_key(person, index).public_key(),
        tls_fingerprint: [3; 32],
        label: format!("Device {index}"),
        device_kind: DeviceKind::Desktop,
        role,
    })
}

/// A local writer: key, certificate, and reservation.
pub(super) struct Identity {
    seed: [u8; 32],
    pub(super) key: WriterKeyPair,
    pub(super) certificate: SignedCertificate,
    pub(super) reservation: Reservation,
}

impl Identity {
    pub(super) fn new(
        seed: [u8; 32],
        device: &DeviceKeyPair,
        device_id: &str,
        predecessor: Option<WriterId>,
    ) -> Self {
        let key = WriterKeyPair::from_seed(&seed).expect("test writer seed should be valid");
        let certificate = sign_certificate(
            &WriterCertificate {
                writer_key: key.public_key(),
                device_id: device_id.to_owned(),
                device_key: device.public_key(),
                created_at_ms: START_MS as i64,
                predecessor,
            },
            device,
        )
        .expect("test certificate should sign");
        Self {
            seed,
            key,
            certificate,
            reservation: Reservation::default(),
        }
    }

    pub(super) fn id(&self) -> WriterId {
        self.certificate.certificate.writer_id()
    }

    pub(super) fn local(&mut self) -> LocalWriter<'_> {
        LocalWriter {
            key: &self.key,
            certificate: &self.certificate,
            reservation: &mut self.reservation,
        }
    }

    /// A copy with the same key and reservation, as a cloned installation would have.
    fn duplicate(&self) -> Self {
        Self {
            seed: self.seed,
            key: WriterKeyPair::from_seed(&self.seed).expect("test writer seed should be valid"),
            certificate: self.certificate.clone(),
            reservation: self.reservation.clone(),
        }
    }
}

/// Database bytes of each test space once its charter is installed and the anchor enrolled
/// every test device, by vault id and person public key.
static SPACES: Mutex<BTreeMap<(String, String), Vec<u8>>> = Mutex::new(BTreeMap::new());

fn spaces() -> std::sync::MutexGuard<'static, BTreeMap<(String, String), Vec<u8>>> {
    SPACES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// What every space template of `person` holds: the authority writer's genesis and one
/// enrollment for each pre-enrolled device.
pub(super) fn template_vector(person: &PersonKeyPair) -> VersionVector {
    let ops = u64::try_from(1 + ENROLLED_DEVICES).unwrap();
    [(authority(person).id(), ops)].into_iter().collect()
}

/// The first writer of the anchor device in `person`'s spaces.
fn authority(person: &PersonKeyPair) -> Identity {
    Identity::new(
        key_seed(person, AUTHORITY_PURPOSE, 0),
        &anchor_device_key(person),
        ANCHOR_DEVICE_ID,
        None,
    )
}

async fn space_template(vault_id: &str, person: &PersonKeyPair) -> Vec<u8> {
    let key = (vault_id.to_owned(), person.public_key().to_text());
    if let Some(bytes) = spaces().get(&key) {
        return bytes.clone();
    }
    let mut conn = connection_from(template().await).await;
    let engine = test_engine();
    let space = engine
        .init_space(&mut conn, vault_id, START_MS)
        .await
        .unwrap();
    let ctx = SpaceContext {
        space,
        person_key: person.public_key(),
    };
    engine
        .install_charter(&mut conn, &ctx, &test_charter(person, space))
        .await
        .unwrap();
    let mut authority = authority(person);
    let admin = anchor_admin(person);
    for index in 0..ENROLLED_DEVICES {
        let entry = enroll_entry(person, index, DeviceRole::Member);
        engine
            .sequence_proposal(
                &mut conn,
                &ctx,
                &proposal(space, &admin, START_MS, entry),
                [0; 32],
                &mut authority.local(),
                START_MS,
            )
            .await
            .unwrap()
            .unwrap();
    }
    let bytes = conn.serialize(None).await.unwrap().to_vec();
    spaces().entry(key).or_insert(bytes).clone()
}

fn writer_seed(salt: u64, replica: usize, generation: u32) -> [u8; 32] {
    let mut seed = [0u8; 32];
    seed[..8].copy_from_slice(&salt.to_le_bytes());
    seed[8..16].copy_from_slice(&(replica as u64).to_le_bytes());
    seed[16..20].copy_from_slice(&generation.to_le_bytes());
    seed[31] = 1;
    seed
}

/// Salt of the `clones`th clone of an installation with `salt`, distinct from the salt of
/// every other clone, a clone of a clone included.
fn clone_salt(salt: u64, clones: u32) -> u64 {
    const MIX: u64 = 0x9e37_79b9_7f4a_7c15;
    salt.wrapping_mul(MIX).wrapping_add(u64::from(clones))
}

/// One installation of the vault.
pub(super) struct Replica {
    pub(super) index: usize,
    pub(super) conn: SqliteConnection,
    pub(super) engine: Engine,
    pub(super) ctx: SpaceContext,
    pub(super) identity: Identity,
    pub(super) person: PersonKeyPair,
    /// Device the replica's writers belong to.
    pub(super) device_id: String,
    device_seed: [u8; 32],
    /// Local wall clock passed to engine calls.
    pub(super) now_ms: u64,
    salt: u64,
    generation: u32,
    /// Clones made of this installation, so each draws distinct writers.
    clones: u32,
}

impl Replica {
    /// A fresh installation of the shared vault.
    pub(super) async fn new(index: usize, salt: u64) -> Self {
        Self::with_space(index, salt, VAULT_ID, person()).await
    }

    /// A fresh installation of the shared vault on the anchor device, which sequences
    /// authorization changes.
    pub(super) async fn anchor(index: usize, salt: u64) -> Self {
        let mut replica = Self::new(index, salt).await;
        replica.identity = authority(&replica.person);
        replica.device_id = ANCHOR_DEVICE_ID.to_owned();
        replica.device_seed = key_seed(&replica.person, DEVICE_PURPOSE, ANCHOR_KEY_INDEX);
        replica
    }

    /// A fresh installation of `vault_id` owned by `person`.
    pub(super) async fn with_space(
        index: usize,
        salt: u64,
        vault_id: &str,
        person: PersonKeyPair,
    ) -> Self {
        let mut conn = connection_from(&space_template(vault_id, &person).await).await;
        let engine = test_engine();
        let space = engine
            .init_space(&mut conn, vault_id, START_MS)
            .await
            .unwrap();
        let ctx = SpaceContext {
            space,
            person_key: person.public_key(),
        };
        let identity = Identity::new(
            writer_seed(salt, index, 0),
            &device_key(&person, index),
            &device_id(index),
            None,
        );
        Self {
            index,
            conn,
            engine,
            ctx,
            identity,
            device_id: device_id(index),
            device_seed: key_seed(&person, DEVICE_PURPOSE, index as u64),
            person,
            now_ms: START_MS,
            salt,
            generation: 0,
            clones: 0,
        }
    }

    /// A clone of this installation's database and writer, as a restored backup would be. Its
    /// later writers belong to the device of replica `index`.
    pub(super) async fn clone_installation(&mut self, index: usize) -> Self {
        let mut clone = self.clone_device().await;
        clone.index = index;
        clone.device_id = device_id(index);
        clone.device_seed = key_seed(&clone.person, DEVICE_PURPOSE, index as u64);
        clone
    }

    /// A clone of this installation's database and writer whose later writers belong to the
    /// same device, as a copied vault folder on the same machine would be. Each clone draws
    /// writers distinct from this installation's and from other clones'.
    pub(super) async fn clone_device(&mut self) -> Self {
        let bytes = self.snapshot().await;
        self.clones += 1;
        Self {
            index: self.index,
            conn: connection_from(&bytes).await,
            engine: test_engine(),
            ctx: self.ctx.clone(),
            identity: self.identity.duplicate(),
            device_id: self.device_id.clone(),
            device_seed: self.device_seed,
            person: person(),
            now_ms: self.now_ms,
            salt: clone_salt(self.salt, self.clones),
            generation: self.generation,
            clones: 0,
        }
    }

    pub(super) fn writer(&self) -> WriterId {
        self.identity.id()
    }

    /// Replaces the local writer with a new one, as the app does when its writer stops being
    /// active.
    pub(super) fn rotate(&mut self) {
        let old = self.identity.id();
        self.generation += 1;
        let device =
            DeviceKeyPair::from_seed(&self.device_seed).expect("test device seed should be valid");
        self.identity = Identity::new(
            writer_seed(self.salt, self.index, self.generation),
            &device,
            &self.device_id,
            Some(old),
        );
    }

    /// Sequences `entry` as the anchor, signed with the anchor admin key at the local clock.
    pub(super) async fn sequence(
        &mut self,
        entry: Entry,
    ) -> SyncResult<Result<u64, ProposalRejection>> {
        let signed = proposal(
            self.ctx.space,
            &anchor_admin(&self.person),
            self.now_ms,
            entry,
        );
        self.sequence_signed(&signed).await
    }

    /// Sequences a signed proposal as the anchor.
    pub(super) async fn sequence_signed(
        &mut self,
        signed: &SignedProposal,
    ) -> SyncResult<Result<u64, ProposalRejection>> {
        self.engine
            .sequence_proposal(
                &mut self.conn,
                &self.ctx,
                signed,
                [5; 32],
                &mut self.identity.local(),
                self.now_ms,
            )
            .await
    }

    /// Removes `device_id` from the space as the anchor.
    pub(super) async fn remove(&mut self, device_id: &str) -> SyncResult<u64> {
        let entry = Entry::Remove(Remove {
            device_id: device_id.to_owned(),
        });
        Ok(self
            .sequence(entry)
            .await?
            .unwrap_or_else(|rejection| panic!("removal of {device_id} rejected: {rejection:?}")))
    }

    /// Whether the selected authorization chain removed this replica's device.
    pub(super) async fn device_removed(&mut self) -> bool {
        let view = self
            .engine
            .authorization(&mut self.conn, &self.ctx)
            .await
            .unwrap()
            .expect("test spaces have a charter");
        view.devices
            .iter()
            .any(|device| device.device_id == self.device_id && device.removed_revision.is_some())
    }

    /// Runs SQL that must succeed.
    pub(super) async fn exec(&mut self, sql: &str) {
        sqlx::raw_sql(AssertSqlSafe(sql))
            .execute(&mut self.conn)
            .await
            .unwrap_or_else(|error| panic!("{sql}: {error}"));
    }

    /// Runs a local edit in one transaction followed by what fixture domain code does after an
    /// edit; returns whether it committed.
    pub(super) async fn try_exec(&mut self, sql: &str) -> bool {
        let mut tx = self.conn.begin().await.unwrap();
        match sqlx::raw_sql(AssertSqlSafe(sql)).execute(&mut *tx).await {
            Ok(_) => {
                after_local_edit(&mut tx)
                    .await
                    .unwrap_or_else(|error| panic!("after {sql}: {error}"));
                tx.commit().await.unwrap();
                true
            }
            Err(_) => {
                tx.rollback().await.unwrap();
                false
            }
        }
    }

    pub(super) async fn scalar_text(&mut self, sql: &str) -> Option<String> {
        sqlx::query_scalar(AssertSqlSafe(sql))
            .fetch_optional(&mut self.conn)
            .await
            .unwrap_or_else(|error| panic!("{sql}: {error}"))
    }

    pub(super) async fn count(&mut self, sql: &str) -> i64 {
        sqlx::query_scalar(AssertSqlSafe(sql))
            .fetch_one(&mut self.conn)
            .await
            .unwrap_or_else(|error| panic!("{sql}: {error}"))
    }

    pub(super) async fn seal(&mut self) -> SyncResult<SealReport> {
        self.engine
            .seal(
                &mut self.conn,
                &self.ctx,
                &mut self.identity.local(),
                self.now_ms,
            )
            .await
    }

    pub(super) async fn apply_once(&mut self) -> SyncResult<ApplyReport> {
        self.engine
            .apply(
                &mut self.conn,
                &self.ctx,
                Some(&mut self.identity.local()),
                self.now_ms,
            )
            .await
    }

    /// Applies until nothing ready remains, rotating the local writer the way the app does
    /// while its device stays enrolled. Returns every apply report.
    pub(super) async fn apply_all(&mut self) -> SyncResult<Vec<ApplyReport>> {
        let mut reports = Vec::new();
        for _ in 0..10_000 {
            let report = self.apply_once().await?;
            let frozen = report.local_writer_frozen;
            let more = report.more;
            reports.push(report);
            if frozen && !self.device_removed().await {
                if !self.reseal_own_forks().await {
                    self.rotate();
                }
            } else if !more {
                return Ok(reports);
            }
        }
        panic!("replica {} never finished applying", self.index);
    }

    /// Re-seals every forked chain of this device that this installation still holds, each with
    /// a new writer, the way the app does. Returns whether there was one.
    pub(super) async fn reseal_own_forks(&mut self) -> bool {
        let forks = self
            .engine
            .own_forks(&mut self.conn, &self.ctx)
            .await
            .unwrap();
        for (old, keep_through) in &forks {
            self.rotate();
            self.engine
                .reseal_fork(
                    &mut self.conn,
                    &self.ctx,
                    *old,
                    *keep_through,
                    &mut self.identity.local(),
                    self.now_ms,
                )
                .await
                .unwrap();
        }
        !forks.is_empty()
    }

    /// Seals, rotating once when the local writer is no longer active.
    pub(super) async fn seal_rotating(&mut self) -> SyncResult<SealReport> {
        match self.seal().await {
            Err(crate::SyncError::WriterInactive(_)) => {
                self.rotate();
                self.seal().await
            }
            other => other,
        }
    }

    pub(super) async fn stored_vector(&mut self) -> VersionVector {
        self.engine
            .stored_vector(&mut self.conn, &self.ctx)
            .await
            .unwrap()
    }

    pub(super) async fn applied_vector(&mut self) -> VersionVector {
        self.engine
            .applied_vector(&mut self.conn, &self.ctx)
            .await
            .unwrap()
    }

    /// Every stored operation `known` does not include, across pages.
    pub(super) async fn ops_since(&mut self, known: &VersionVector) -> Vec<Vec<u8>> {
        let mut known = known.clone();
        let mut ops = Vec::new();
        loop {
            let page = self
                .engine
                .ops_after(&mut self.conn, &self.ctx, &known, 64 * 1024)
                .await
                .unwrap();
            for bytes in &page.ops {
                let envelope = ganbaru_sync_contracts::Envelope::decode(bytes).unwrap();
                known.advance(envelope.writer, envelope.seq);
            }
            ops.extend(page.ops);
            if !page.more {
                return ops;
            }
        }
    }

    /// Every stored operation of this replica's own writer, leaving out the authorization log
    /// every space template holds.
    pub(super) async fn own_ops(&mut self) -> Vec<Vec<u8>> {
        let writer = self.writer();
        self.ops_since(&VersionVector::new())
            .await
            .into_iter()
            .filter(|bytes| {
                ganbaru_sync_contracts::Envelope::decode(bytes)
                    .unwrap()
                    .writer
                    == writer
            })
            .collect()
    }

    pub(super) async fn receive(&mut self, ops: &[Vec<u8>]) -> Vec<StoreOutcome> {
        self.engine
            .store_many(&mut self.conn, &self.ctx, ops, self.now_ms)
            .await
            .unwrap()
    }

    /// Every fork evidence this replica stores.
    pub(super) async fn evidence(&mut self) -> Vec<crate::ForkEvidence> {
        let forked = self
            .engine
            .forked_writers(&mut self.conn, &self.ctx)
            .await
            .unwrap();
        let mut evidence = Vec::with_capacity(forked.len());
        for (writer, _) in forked {
            evidence.push(
                self.engine
                    .fork_evidence(&mut self.conn, &self.ctx, writer)
                    .await
                    .unwrap()
                    .expect("a forked writer has evidence"),
            );
        }
        evidence
    }

    /// Stores fork evidence `from` holds and this replica lacks.
    pub(super) async fn take_evidence(&mut self, from: &mut Self) -> Vec<crate::EvidenceOutcome> {
        let mut outcomes = Vec::new();
        for evidence in from.evidence().await {
            outcomes.push(
                self.engine
                    .store_fork_evidence(
                        &mut self.conn,
                        &self.ctx,
                        &evidence.first,
                        &evidence.second,
                        self.now_ms,
                    )
                    .await
                    .unwrap(),
            );
        }
        outcomes
    }

    /// Stores the fork evidence and then the operations `from` has and this replica lacks, then
    /// applies.
    pub(super) async fn pull(&mut self, from: &mut Self) -> Vec<StoreOutcome> {
        self.take_evidence(from).await;
        let known = self.stored_vector().await;
        let ops = from.ops_since(&known).await;
        let outcomes = self.receive(&ops).await;
        self.apply_all().await.unwrap();
        outcomes
    }

    /// The whole database, for comparing before and after a failed call.
    pub(super) async fn snapshot(&mut self) -> Vec<u8> {
        self.conn.serialize(None).await.unwrap().to_vec()
    }

    /// Checks invariants that hold between engine calls.
    pub(super) async fn check_invariants(&mut self) {
        let violations = self
            .count("SELECT count(*) FROM pragma_foreign_key_check")
            .await;
        assert_eq!(violations, 0, "replica {} breaks foreign keys", self.index);
        let applying = self
            .count("SELECT applying FROM sync_apply_state WHERE singleton = 1")
            .await;
        assert_eq!(applying, 0, "replica {} left applying set", self.index);
        let stale_index = self
            .count(
                "SELECT (SELECT count(*) FROM (
                     SELECT id, label FROM fixture_items
                     EXCEPT SELECT item_id, label FROM fixture_items_index
                 )) + (SELECT count(*) FROM (
                     SELECT item_id, label FROM fixture_items_index
                     EXCEPT SELECT id, label FROM fixture_items
                 ))",
            )
            .await;
        assert_eq!(
            stale_index, 0,
            "replica {} has a stale fixture index",
            self.index
        );
        // Nodes reached from a root, with their depth; a node on a cycle is never reached.
        let reached = "WITH RECURSIVE reached(id, depth) AS (
                 SELECT id, 0 FROM fixture_nodes WHERE parent_id IS NULL
                 UNION ALL
                 SELECT n.id, r.depth + 1 FROM fixture_nodes n JOIN reached r ON n.parent_id = r.id
             )";
        let unreached = self
            .count(&format!(
                "{reached} SELECT (SELECT count(*) FROM fixture_nodes)
                     - (SELECT count(*) FROM reached)"
            ))
            .await;
        assert_eq!(unreached, 0, "replica {} has a cycle of nodes", self.index);
        let too_deep = self
            .count(&format!(
                "{reached} SELECT count(*) FROM reached WHERE depth > {NODE_MAX_DEPTH}"
            ))
            .await;
        assert_eq!(too_deep, 0, "replica {} nests nodes too deep", self.index);
        let closed = self
            .count(
                "WITH RECURSIVE path(start, node) AS (
                     SELECT from_id, to_id FROM fixture_edges
                     UNION
                     SELECT p.start, e.to_id FROM path p JOIN fixture_edges e ON e.from_id = p.node
                 )
                 SELECT count(*) FROM path WHERE start = node",
            )
            .await;
        assert_eq!(closed, 0, "replica {} has a cycle of edges", self.index);
        // The fixture hook output equals a recomputation from the materialized entries.
        let stale_counts = self
            .count(
                "SELECT (SELECT count(*) FROM fixture_lists AS list
                     WHERE entry_count <> (SELECT count(*) FROM fixture_list_entries
                         WHERE list_id = list.id))
                 + (SELECT count(*) FROM (
                     SELECT id, entry_count FROM fixture_lists
                     EXCEPT SELECT list_id, entries FROM fixture_list_counts
                 ))
                 + (SELECT count(*) FROM (
                     SELECT list_id, entries FROM fixture_list_counts
                     EXCEPT SELECT id, entry_count FROM fixture_lists
                 ))",
            )
            .await;
        assert_eq!(
            stale_counts, 0,
            "replica {} has stale list entry counts",
            self.index
        );
        self.check_compacted_state().await;
    }

    /// Asserts that every retained version reads back to a value with its stored hash, elided
    /// or not. Returns the value bytes stored and the value bytes with nothing elided.
    pub(super) async fn check_version_values(&mut self) -> (usize, usize) {
        let (mut stored, mut full) = (0, 0);
        type StoredVersion = (i64, String, i64, Vec<u8>, i64, Vec<u8>, Option<Vec<u8>>);
        let versions: Vec<StoredVersion> = sqlx::query_as(
            "SELECT table_id, row_key, group_id, writer_id, seq, value_hash, value
             FROM sync_register_versions",
        )
        .fetch_all(&mut self.conn)
        .await
        .unwrap();
        for (table, key, group, writer, seq, hash, value) in versions {
            let table = self
                .engine
                .manifest()
                .table(TableId(u16::try_from(table).unwrap()))
                .unwrap();
            let group = GroupId::new(u8::try_from(group).unwrap()).unwrap();
            let hash = log::digest(&hash).unwrap();
            let value = match value {
                Some(value) => {
                    stored += value.len();
                    Value::decode(&value).unwrap()
                }
                None => hydrate::elided_value(
                    &self.engine,
                    &mut self.conn,
                    table,
                    &key,
                    group,
                    log::writer_id(&writer).unwrap(),
                    u64::try_from(seq).unwrap(),
                    &hash,
                )
                .await
                .unwrap_or_else(|error| {
                    panic!(
                        "replica {} cannot hydrate {key} group {group:?}: {error:?}",
                        self.index
                    )
                }),
            };
            assert_eq!(
                value.hash(table.id, group),
                hash,
                "replica {} reads a version of {key} back to another value",
                self.index
            );
            full += value.encode().len();
        }
        (stored, full)
    }

    /// Retained value hashes per captured row and group, with each captured row's forced
    /// groups and the operations stored so far, taken before a seal.
    pub(super) async fn seal_snapshot(&mut self) -> SealSnapshot {
        let versions: Vec<(i64, String, i64, Vec<u8>)> = sqlx::query_as(
            "SELECT v.table_id, v.row_key, v.group_id, v.value_hash
             FROM sync_register_versions v
             JOIN sync_capture c ON c.table_id = v.table_id AND c.row_key = v.row_key",
        )
        .fetch_all(&mut self.conn)
        .await
        .unwrap();
        let mut retained: BTreeMap<(i64, String, i64), BTreeSet<Vec<u8>>> = BTreeMap::new();
        for (table, key, group, hash) in versions {
            retained
                .entry((table, key, group))
                .or_default()
                .insert(hash);
        }
        let captures: Vec<(i64, String, i64, bool)> =
            sqlx::query_as("SELECT table_id, row_key, forced_mask, created FROM sync_capture")
                .fetch_all(&mut self.conn)
                .await
                .unwrap();
        let forced = captures.into_iter().map(|(table, key, mask, created)| {
            let derived_key = self
                .engine
                .manifest()
                .table(TableId(u16::try_from(table).unwrap()))
                .is_some_and(|spec| spec.derived_key.is_some());
            (table, key, if created && derived_key { -1 } else { mask })
        });
        let ops: Vec<(Vec<u8>, i64)> = sqlx::query_as("SELECT writer_id, seq FROM sync_ops")
            .fetch_all(&mut self.conn)
            .await
            .unwrap();
        SealSnapshot {
            retained,
            forced: forced
                .map(|(table, key, mask)| ((table, key), mask))
                .collect(),
            ops: ops.into_iter().collect(),
        }
    }

    /// Asserts that no write sealed since `before` for a captured row carries a group, other
    /// than the existence group or a forced one, equal to its uncontested winner at the time.
    pub(super) async fn check_sealed_writes(&mut self, before: &SealSnapshot) {
        let ops: Vec<(Vec<u8>, i64)> = sqlx::query_as("SELECT writer_id, seq FROM sync_ops")
            .fetch_all(&mut self.conn)
            .await
            .unwrap();
        for (writer, seq) in ops {
            if before.ops.contains(&(writer.clone(), seq)) {
                continue;
            }
            let envelope: Vec<u8> =
                sqlx::query_scalar("SELECT envelope FROM sync_ops WHERE writer_id = ? AND seq = ?")
                    .bind(writer.as_slice())
                    .bind(seq)
                    .fetch_one(&mut self.conn)
                    .await
                    .unwrap();
            let operation = Envelope::decode(&envelope).unwrap().operation().unwrap();
            let Content::Changes(changes) = operation.content else {
                continue;
            };
            for change in changes {
                let table = i64::from(change.table.0);
                let key = change.row.as_str().to_string();
                let Some(forced) = before.forced.get(&(table, key.clone())) else {
                    continue;
                };
                if change.action != ChangeAction::Write {
                    continue;
                }
                let spec = self.engine.manifest().table(change.table).unwrap();
                for value in &change.groups {
                    let group = i64::from(value.group.get());
                    if spec.is_exists_group(value.group) || forced & (1 << group) != 0 {
                        continue;
                    }
                    let winner = before.retained.get(&(table, key.clone(), group));
                    assert!(
                        winner.is_none_or(|hashes| {
                            hashes.len() != 1 || !hashes.contains(value.hash.as_slice())
                        }),
                        "replica {} sealed group {group} of {key} equal to its uncontested winner",
                        self.index
                    );
                }
            }
        }
    }

    /// Replicated state in a canonical text form: published rows of domain tables without local
    /// columns and engine merge state without local timestamps.
    pub(super) async fn dump(&mut self) -> Vec<(&'static str, Vec<String>)> {
        let mut dump = Vec::new();
        for (table, excluded, published) in dumped(self.engine.manifest()) {
            let columns: Vec<String> =
                sqlx::query_scalar("SELECT name FROM pragma_table_info(?) ORDER BY cid")
                    .bind(table)
                    .fetch_all(&mut self.conn)
                    .await
                    .unwrap();
            let selected = columns
                .iter()
                .filter(|column| !excluded.contains(&column.as_str()))
                .map(|column| format!("quote(\"{column}\")"))
                .collect::<Vec<_>>()
                .join(" || '|' || ");
            let published = published.unwrap_or_else(|| "1".into());
            let rows: Vec<String> = sqlx::query_scalar(AssertSqlSafe(format!(
                "SELECT {selected} AS row FROM {table} AS {DUMPED_ALIAS} WHERE {published}
                 ORDER BY row"
            )))
            .fetch_all(&mut self.conn)
            .await
            .unwrap();
            dump.push((table, rows));
        }
        dump
    }

    /// Compacts up to `limit` in batches of `batch_ops`, asserting that domain rows stay the
    /// same and that compacted state stays readable.
    pub(super) async fn compact(
        &mut self,
        limit: &VersionVector,
        batch_ops: usize,
    ) -> SyncResult<CompactionReport> {
        let before = self.domain_dump().await;
        let report = self
            .engine
            .compact_with(
                &mut self.conn,
                &self.ctx,
                limit,
                batch_ops,
                std::time::Duration::MAX,
            )
            .await;
        assert_eq!(
            self.domain_dump().await,
            before,
            "replica {} changed domain rows while compacting",
            self.index
        );
        self.check_compacted_state().await;
        report
    }

    /// Asserts [`Self::check_compacted_state`] and that every retained version reads back.
    pub(super) async fn check_compacted(&mut self) {
        self.check_compacted_state().await;
        self.check_version_values().await;
    }

    /// Asserts that nothing above a writer's compacted sequence lacks a body, that kept kinds
    /// keep theirs, and that no version of a compacted operation is elided. Heads are left
    /// out: a re-seal may cut a chain back to a compacted operation.
    pub(super) async fn check_compacted_state(&mut self) {
        let above = self
            .count(
                "SELECT count(*) FROM sync_ops o JOIN sync_writers w
                     ON w.space_id = o.space_id AND w.writer_id = o.writer_id
                 WHERE length(o.envelope) = 0 AND o.seq > w.compacted_seq",
            )
            .await;
        assert_eq!(
            above, 0,
            "replica {} lost a body above the compacted sequence",
            self.index
        );
        let kept = self
            .count(
                "SELECT count(*) FROM sync_ops o JOIN sync_writers w
                     ON w.space_id = o.space_id AND w.writer_id = o.writer_id
                 WHERE length(o.envelope) = 0 AND o.kind NOT IN (0, 3)",
            )
            .await;
        assert_eq!(
            kept, 0,
            "replica {} dropped a body compaction keeps",
            self.index
        );
        let elided = self
            .count(
                "SELECT count(*) FROM sync_register_versions v JOIN sync_writers w
                     ON w.writer_id = v.writer_id
                 WHERE v.value IS NULL AND v.seq <= w.compacted_seq",
            )
            .await;
        assert_eq!(
            elided, 0,
            "replica {} elides a version of a compacted operation",
            self.index
        );
    }

    /// Domain tables only.
    pub(super) async fn domain_dump(&mut self) -> Vec<(&'static str, Vec<String>)> {
        let mut dump = self.dump().await;
        dump.retain(|(table, _)| !table.starts_with("sync_"));
        dump
    }
}

/// Index of the replica that seals geneses for other installations.
const MINTER_INDEX: usize = 9;

/// Writers other installations created: the enrollments of their devices the anchor sealed
/// and the writers' geneses.
pub(super) struct Minted {
    pub(super) enrollments: Vec<Vec<u8>>,
    pub(super) geneses: Vec<Vec<u8>>,
}

impl Minted {
    /// Stores and applies the enrollments on `replica`, so that it accepts the geneses.
    pub(super) async fn enroll(&self, replica: &mut Replica) {
        replica.receive(&self.enrollments).await;
        replica.apply_all().await.unwrap();
    }
}

/// The device of minted writer `writer` when writers fill devices outside the enrolled range in
/// turn, up to the writer limit of each.
pub(super) fn spread_devices(writer: usize) -> usize {
    ENROLLED_DEVICES + writer / ganbaru_sync_contracts::bounds::MAX_WRITERS_PER_DEVICE
}

/// Geneses of new writers numbered `writers`, each on the device of replica `device(writer)`,
/// as other installations would seal them, with the enrollments of devices outside the enrolled
/// range. One space takes one mint, since each mint extends the anchor's chain from the same
/// start.
pub(super) async fn mint_writers(
    writers: std::ops::Range<usize>,
    device: impl Fn(usize) -> usize,
) -> Minted {
    let mut minter = Replica::anchor(MINTER_INDEX, 1).await;
    let start = minter.stored_vector().await;
    let devices: BTreeSet<usize> = writers.clone().map(&device).collect();
    for index in devices
        .into_iter()
        .filter(|index| *index >= ENROLLED_DEVICES)
    {
        let entry = enroll_entry(&minter.person, index, DeviceRole::Member);
        minter.sequence(entry).await.unwrap().unwrap();
    }
    let enrollments = minter.ops_since(&start).await;
    let enrolled = minter.stored_vector().await;
    for writer in writers {
        let mut seed = [0u8; 32];
        seed[..8].copy_from_slice(&(writer as u64).to_le_bytes());
        seed[31] = 2;
        let index = device(writer);
        minter.identity = Identity::new(
            seed,
            &device_key(&minter.person, index),
            &device_id(index),
            None,
        );
        minter.seal().await.unwrap();
    }
    Minted {
        enrollments,
        geneses: minter.ops_since(&enrolled).await,
    }
}

/// Alias of the dumped table, which publication conditions name.
const DUMPED_ALIAS: &str = "dumped";

/// A table compared across replicas, the columns it leaves out, and the condition its rows
/// must meet to be compared.
type Dumped = (&'static str, Vec<&'static str>, Option<String>);

/// Tables compared across replicas: every replicated table and owned child of `manifest`
/// without its `Local` columns, then the engine merge state. Drafts stay on the replica that
/// wrote them, so a table that publishes conditionally compares its published rows only.
fn dumped(manifest: &'static Manifest) -> Vec<Dumped> {
    let mut tables = Vec::new();
    for table in manifest.tables {
        let local = table.local_columns.iter().map(|column| column.name);
        let published = publication::condition_in(manifest, table, DUMPED_ALIAS);
        tables.push((table.name, local.collect(), published));
        for (_, owned) in table.owned_children() {
            tables.push((owned.table, Vec::new(), None));
        }
    }
    tables.extend(
        ENGINE_DUMPED
            .iter()
            .map(|(table, excluded)| (*table, excluded.to_vec(), None)),
    );
    tables
}

/// Engine merge state compared across replicas and its local timestamps. A retained value is
/// compared by its hash, since whether it is elided depends on local captures. Compaction is
/// local too, so operation bodies and compacted sequences are left out.
const ENGINE_DUMPED: &[(&str, &[&str])] = &[
    ("sync_writers", &["created_at_ms", "compacted_seq"]),
    ("sync_ops", &["stored_at_ms", "envelope"]),
    ("sync_rows", &[]),
    ("sync_register_versions", &["value"]),
    ("sync_version_refs", &[]),
    ("sync_tombstones", &[]),
    ("sync_auth_entries", &[]),
    ("sync_devices", &[]),
    ("sync_late_changes", &[]),
];

pub(super) fn device_id(index: usize) -> String {
    format!("device-{index}")
}

/// Exchanges operations between every pair until no replica stores or seals anything new and
/// every replica stores the same operations. Applying can seal pending captures, so a round
/// that stored nothing still continues while stored vectors differ.
pub(super) async fn converge(replicas: &mut [Replica]) {
    for _ in 0..64 {
        let mut stored = false;
        for target in 0..replicas.len() {
            for source in 0..replicas.len() {
                if source == target {
                    continue;
                }
                let (to, from) = pair(replicas, target, source);
                let outcomes = to.pull(from).await;
                stored |= outcomes
                    .iter()
                    .any(|outcome| matches!(outcome, StoreOutcome::Stored { .. }));
            }
        }
        let mut sealed = false;
        for replica in replicas.iter_mut() {
            sealed |= replica.seal_rotating().await.unwrap().sealed_ops > 0;
            replica.check_invariants().await;
        }
        let mut vectors = Vec::with_capacity(replicas.len());
        for replica in replicas.iter_mut() {
            vectors.push(replica.stored_vector().await);
        }
        let same = vectors.windows(2).all(|pair| pair[0] == pair[1]);
        if !stored && !sealed && same {
            return;
        }
    }
    panic!("replicas did not converge");
}

/// Two distinct replicas of a slice, mutably.
pub(super) fn pair(replicas: &mut [Replica], a: usize, b: usize) -> (&mut Replica, &mut Replica) {
    assert_ne!(a, b);
    if a < b {
        let (low, high) = replicas.split_at_mut(b);
        (&mut low[a], &mut high[0])
    } else {
        let (low, high) = replicas.split_at_mut(a);
        (&mut high[0], &mut low[b])
    }
}

/// A SQL string literal.
pub(super) fn quoted(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

/// The order key at a rank.
pub(super) fn rank(position: u32) -> String {
    ganbaru_sync_contracts::OrderKey::rank(position)
        .unwrap()
        .into_string()
}
