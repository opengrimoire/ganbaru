//! Simulated installations of one vault: an in-memory database, the vault engine, and the
//! local writer, with helpers that mirror what the app does between engine calls.

use std::sync::OnceLock;

use ganbaru_contacts::PersonKeyPair;
use ganbaru_sync_contracts::{
    SignedCertificate, VersionVector, WriterCertificate, WriterId, WriterKeyPair, sign_certificate,
};
use sqlx::sqlite::SqliteOwnedBuf;
use sqlx::{Connection, SqliteConnection};

use super::migrated_pool;
use crate::{
    ApplyReport, Engine, LocalWriter, ResealNeeded, ResealReport, SealReport, SequenceReservation,
    SpaceContext, StoreOutcome, SyncResult,
};

/// Vault id every replica shares.
pub(super) const VAULT_ID: &str = "vault-sim";
/// Simulated time every replica starts from, in Unix milliseconds.
pub(super) const START_MS: u64 = 1_800_000_000_000;

const PERSON_SEED: [u8; 32] = [7; 32];

/// The vault owner's person key.
pub(super) fn person() -> PersonKeyPair {
    PersonKeyPair::from_seed(&PERSON_SEED).expect("test person seed should be valid")
}

/// Database bytes of a migrated vault, built once per test binary.
static TEMPLATE: OnceLock<Vec<u8>> = OnceLock::new();

async fn template() -> &'static [u8] {
    if let Some(bytes) = TEMPLATE.get() {
        return bytes;
    }
    let pool = migrated_pool().await;
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
        person: &PersonKeyPair,
        device_id: &str,
        predecessor: Option<WriterId>,
    ) -> Self {
        let key = WriterKeyPair::from_seed(&seed).expect("test writer seed should be valid");
        let certificate = sign_certificate(
            &WriterCertificate {
                writer_key: key.public_key(),
                person_key: person.public_key(),
                device_id: device_id.to_owned(),
                created_at_ms: START_MS as i64,
                predecessor,
            },
            person,
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

fn writer_seed(salt: u64, replica: usize, generation: u32) -> [u8; 32] {
    let mut seed = [0u8; 32];
    seed[..8].copy_from_slice(&salt.to_le_bytes());
    seed[8..16].copy_from_slice(&(replica as u64).to_le_bytes());
    seed[16..20].copy_from_slice(&generation.to_le_bytes());
    seed[31] = 1;
    seed
}

/// One installation of the vault.
pub(super) struct Replica {
    pub(super) index: usize,
    pub(super) conn: SqliteConnection,
    pub(super) engine: Engine,
    pub(super) ctx: SpaceContext,
    pub(super) identity: Identity,
    pub(super) person: PersonKeyPair,
    /// Local wall clock passed to engine calls.
    pub(super) now_ms: u64,
    salt: u64,
    generation: u32,
}

impl Replica {
    /// A fresh installation of the shared vault.
    pub(super) async fn new(index: usize, salt: u64) -> Self {
        Self::with_space(index, salt, VAULT_ID, person()).await
    }

    /// A fresh installation of `vault_id` owned by `person`.
    pub(super) async fn with_space(
        index: usize,
        salt: u64,
        vault_id: &str,
        person: PersonKeyPair,
    ) -> Self {
        let mut conn = connection_from(template().await).await;
        let engine = Engine::vault();
        let space = engine
            .init_space(&mut conn, vault_id, START_MS)
            .await
            .unwrap();
        let ctx = SpaceContext {
            space,
            anchor: person.public_key(),
        };
        let identity = Identity::new(
            writer_seed(salt, index, 0),
            &person,
            &device_id(index),
            None,
        );
        Self {
            index,
            conn,
            engine,
            ctx,
            identity,
            person,
            now_ms: START_MS,
            salt,
            generation: 0,
        }
    }

    /// A clone of this installation's database and writer, as a restored backup would be.
    pub(super) async fn clone_installation(&mut self, index: usize) -> Self {
        let bytes = self.snapshot().await;
        Self {
            index,
            conn: connection_from(&bytes).await,
            engine: Engine::vault(),
            ctx: self.ctx,
            identity: self.identity.duplicate(),
            person: person(),
            now_ms: self.now_ms,
            salt: self.salt,
            generation: self.generation,
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
        self.identity = Identity::new(
            writer_seed(self.salt, self.index, self.generation),
            &self.person,
            &device_id(self.index),
            Some(old),
        );
    }

    /// Runs SQL that must succeed.
    pub(super) async fn exec(&mut self, sql: &str) {
        sqlx::raw_sql(sql)
            .execute(&mut self.conn)
            .await
            .unwrap_or_else(|error| panic!("{sql}: {error}"));
    }

    /// Runs SQL in one transaction; returns whether it committed.
    pub(super) async fn try_exec(&mut self, sql: &str) -> bool {
        let mut tx = self.conn.begin().await.unwrap();
        match sqlx::raw_sql(sql).execute(&mut *tx).await {
            Ok(_) => {
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
        sqlx::query_scalar(sql)
            .fetch_optional(&mut self.conn)
            .await
            .unwrap_or_else(|error| panic!("{sql}: {error}"))
    }

    pub(super) async fn count(&mut self, sql: &str) -> i64 {
        sqlx::query_scalar(sql)
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

    /// Re-seals the local writer after a revocation, rotating first unless that already
    /// happened in an attempt that rolled back.
    pub(super) async fn reseal(&mut self, needed: ResealNeeded) -> SyncResult<ResealReport> {
        if self.identity.id() == needed.writer {
            self.rotate();
        }
        self.engine
            .reseal(
                &mut self.conn,
                &self.ctx,
                needed.writer,
                needed.keep_through,
                &mut self.identity.local(),
                self.now_ms,
            )
            .await
    }

    /// Applies until nothing ready remains, re-sealing and rotating the local writer the way
    /// the app does. Returns every apply report.
    pub(super) async fn apply_all(&mut self) -> SyncResult<Vec<ApplyReport>> {
        let mut reports = Vec::new();
        for _ in 0..10_000 {
            let report = self.apply_once().await?;
            let reseal = report.reseal_needed;
            let frozen = report.local_writer_frozen;
            let more = report.more;
            reports.push(report);
            if let Some(needed) = reseal {
                self.reseal(needed).await?;
            } else if frozen {
                self.rotate();
            } else if !more {
                return Ok(reports);
            }
        }
        panic!("replica {} never finished applying", self.index);
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

    pub(super) async fn receive(&mut self, ops: &[Vec<u8>]) -> Vec<StoreOutcome> {
        self.engine
            .store_many(&mut self.conn, &self.ctx, ops, self.now_ms)
            .await
            .unwrap()
    }

    /// Stores what `from` has and this replica lacks, then applies.
    pub(super) async fn pull(&mut self, from: &mut Self) -> Vec<StoreOutcome> {
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
    }

    /// Replicated state in a canonical text form: domain tables without local columns and
    /// engine merge state without local timestamps.
    pub(super) async fn dump(&mut self) -> Vec<(&'static str, Vec<String>)> {
        let mut dump = Vec::new();
        for (table, excluded) in DUMPED {
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
            let rows: Vec<String> = sqlx::query_scalar(&format!(
                "SELECT {selected} AS row FROM {table} ORDER BY row"
            ))
            .fetch_all(&mut self.conn)
            .await
            .unwrap();
            dump.push((*table, rows));
        }
        dump
    }

    /// Domain tables only.
    pub(super) async fn domain_dump(&mut self) -> Vec<(&'static str, Vec<String>)> {
        let mut dump = self.dump().await;
        dump.retain(|(table, _)| !table.starts_with("sync_"));
        dump
    }
}

/// Tables compared across replicas and their local columns.
const DUMPED: &[(&str, &[&str])] = &[
    ("quick_note_tags", &[]),
    ("quick_notes", &["revision"]),
    ("quick_note_text_runs", &[]),
    ("sync_writers", &["created_at_ms"]),
    ("sync_ops", &["stored_at_ms"]),
    ("sync_rows", &[]),
    ("sync_register_versions", &[]),
    ("sync_tombstones", &[]),
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
