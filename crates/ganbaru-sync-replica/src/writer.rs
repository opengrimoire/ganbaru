//! The local writer: its signing key, its device-local record, and the high-water check that
//! detects a restored or copied database.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ganbaru_contacts::PersonKeyPair;
use ganbaru_sync::local::LocalWriterHead;
use ganbaru_sync::{Engine, LocalWriter, SequenceReservation, SpaceContext, WriterState};
use ganbaru_sync_contracts::{
    SignedCertificate, WriterCertificate, WriterId, WriterKeyPair, sign_certificate,
};
use serde::{Deserialize, Serialize};
use sqlx::SqliteConnection;
use std::path::{Path, PathBuf};

const RECORD_SCHEMA_VERSION: u32 = 1;
/// Sequences one reservation write makes durable ahead of sealing.
pub(super) const RESERVATION_BLOCK: u64 = 256;

/// Device-local state of the writer this installation seals with.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WriterRecord {
    pub schema_version: u32,
    /// Hex writer id.
    pub writer_id: String,
    /// Encoded certificate of the writer, base64url without padding.
    pub certificate: String,
    /// Highest sequence made durable before signing.
    pub reserved_through: u64,
    /// Highest sequence known to be committed in the vault database.
    pub last_committed_seq: u64,
    /// Exchanges are paused on this device; sealing continues.
    pub paused: bool,
    /// Retired writers whose keys still wait to be removed.
    pub retired_keys: Vec<String>,
}

/// Storage of writer signing keys outside the vault.
pub trait WriterKeyStore: Send + Sync {
    fn read(&self, vault_id: &str, writer_id: &str) -> Result<Option<Vec<u8>>, String>;
    fn write(&self, vault_id: &str, writer_id: &str, pkcs8: &[u8]) -> Result<(), String>;
    fn remove(&self, vault_id: &str, writer_id: &str) -> Result<(), String>;
}

fn key_reference(vault_id: &str, writer_id: &str) -> String {
    format!("writer-key:{vault_id}:{writer_id}")
}

/// Desktop key store in the operating-system keyring.
#[cfg(desktop)]
pub struct KeyringStore;

#[cfg(desktop)]
impl KeyringStore {
    const SERVICE: &'static str = "com.ganbaru-ai.sync";

    fn entry(vault_id: &str, writer_id: &str) -> Result<keyring::Entry, String> {
        keyring::Entry::new(Self::SERVICE, &key_reference(vault_id, writer_id))
            .map_err(|error| format!("open writer key store: {error}"))
    }
}

#[cfg(desktop)]
impl WriterKeyStore for KeyringStore {
    fn read(&self, vault_id: &str, writer_id: &str) -> Result<Option<Vec<u8>>, String> {
        match Self::entry(vault_id, writer_id)?.get_password() {
            Ok(encoded) => URL_SAFE_NO_PAD
                .decode(encoded)
                .map(Some)
                .map_err(|error| format!("decode stored writer key: {error}")),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(format!("read writer key: {error}")),
        }
    }

    fn write(&self, vault_id: &str, writer_id: &str, pkcs8: &[u8]) -> Result<(), String> {
        Self::entry(vault_id, writer_id)?
            .set_password(&URL_SAFE_NO_PAD.encode(pkcs8))
            .map_err(|error| format!("store writer key: {error}"))
    }

    fn remove(&self, vault_id: &str, writer_id: &str) -> Result<(), String> {
        match Self::entry(vault_id, writer_id)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(format!("remove writer key: {error}")),
        }
    }
}

/// Key store of one private file per writer, used on Android and in tests.
#[cfg(any(mobile, test))]
pub struct FileKeyStore {
    pub directory: PathBuf,
}

#[cfg(any(mobile, test))]
impl FileKeyStore {
    fn path(&self, vault_id: &str, writer_id: &str) -> PathBuf {
        self.directory.join(format!(
            "{}.key",
            key_reference(vault_id, writer_id).replace(':', "-")
        ))
    }
}

#[cfg(any(mobile, test))]
impl WriterKeyStore for FileKeyStore {
    fn read(&self, vault_id: &str, writer_id: &str) -> Result<Option<Vec<u8>>, String> {
        match std::fs::read(self.path(vault_id, writer_id)) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(format!("read writer key: {error}")),
        }
    }

    fn write(&self, vault_id: &str, writer_id: &str, pkcs8: &[u8]) -> Result<(), String> {
        ganbaru_handoff::pairing::write_private_file_atomically(
            &self.path(vault_id, writer_id),
            pkcs8,
        )
    }

    fn remove(&self, vault_id: &str, writer_id: &str) -> Result<(), String> {
        match std::fs::remove_file(self.path(vault_id, writer_id)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!("remove writer key: {error}")),
        }
    }
}

/// Device-local sync files of one vault under the app config directory.
#[derive(Clone, Debug)]
pub struct SyncFiles {
    pub directory: PathBuf,
    pub vault_id: String,
}

impl SyncFiles {
    pub fn new(directory: PathBuf, vault_id: &str) -> Result<Self, String> {
        let valid = !vault_id.is_empty()
            && vault_id.len() <= 128
            && vault_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_');
        if !valid {
            return Err("vault id cannot name sync files".to_string());
        }
        Ok(Self {
            directory,
            vault_id: vault_id.to_string(),
        })
    }

    pub fn record_path(&self) -> PathBuf {
        self.directory.join(format!("{}.json", self.vault_id))
    }

    pub fn carry_path(&self) -> PathBuf {
        self.directory.join(format!("{}.carry", self.vault_id))
    }
}

pub fn read_record(path: &Path) -> Result<Option<WriterRecord>, String> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("read sync writer record: {error}")),
    };
    let record: WriterRecord = serde_json::from_slice(&bytes)
        .map_err(|error| format!("parse sync writer record: {error}"))?;
    if record.schema_version != RECORD_SCHEMA_VERSION {
        return Err("sync writer record has an unsupported schema version".to_string());
    }
    Ok(Some(record))
}

fn write_record(path: &Path, record: &WriterRecord) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(record)
        .map_err(|error| format!("encode sync writer record: {error}"))?;
    ganbaru_handoff::pairing::write_private_file_atomically(path, &bytes)
}

/// Whether the database still holds the chain the record describes. A copied or restored
/// database lacks committed sequences or carries a chain past the reservation.
pub fn record_matches(record: &WriterRecord, head: LocalWriterHead) -> bool {
    match head {
        LocalWriterHead::Absent => record.last_committed_seq == 0,
        LocalWriterHead::Genuine {
            stored,
            state: WriterState::Active,
        } => record.last_committed_seq <= stored && stored <= record.reserved_through,
        LocalWriterHead::Genuine { .. } | LocalWriterHead::Mismatch => false,
    }
}

/// The writer this installation seals with, with its durable record.
pub struct ActiveWriter {
    key: WriterKeyPair,
    certificate: SignedCertificate,
    reservation: RecordReservation,
}

impl ActiveWriter {
    pub fn id(&self) -> WriterId {
        self.certificate.certificate.writer_id()
    }

    pub fn record(&self) -> &WriterRecord {
        &self.reservation.record
    }

    /// The engine view of this writer with its durable reservation.
    pub fn local(&mut self) -> LocalWriter<'_> {
        LocalWriter {
            key: &self.key,
            certificate: &self.certificate,
            reservation: &mut self.reservation,
        }
    }

    /// Records the highest committed sequence after a seal commits.
    pub fn committed(&mut self, seq: u64) -> Result<(), String> {
        if seq <= self.reservation.record.last_committed_seq {
            return Ok(());
        }
        let mut next = self.reservation.record.clone();
        next.last_committed_seq = seq;
        next.reserved_through = next.reserved_through.max(seq);
        self.reservation.replace(next)
    }

    /// Records whether exchanges are paused on this device.
    pub fn set_paused(&mut self, paused: bool) -> Result<(), String> {
        if self.reservation.record.paused == paused {
            return Ok(());
        }
        let mut next = self.reservation.record.clone();
        next.paused = paused;
        self.reservation.replace(next)
    }

    /// Every writer this installation sealed with: the active one and the retired ones whose
    /// keys it still lists.
    pub fn own_writers(&self) -> std::collections::BTreeSet<WriterId> {
        let record = self.record();
        std::iter::once(self.id())
            .chain(
                record
                    .retired_keys
                    .iter()
                    .filter_map(|writer| WriterId::from_hex(writer)),
            )
            .collect()
    }
}

/// The durable reservation of an active writer record.
pub struct RecordReservation {
    path: PathBuf,
    record: WriterRecord,
}

impl RecordReservation {
    fn replace(&mut self, next: WriterRecord) -> Result<(), String> {
        write_record(&self.path, &next)?;
        self.record = next;
        Ok(())
    }
}

impl SequenceReservation for RecordReservation {
    fn ensure_reserved(&mut self, seq: u64) -> std::io::Result<()> {
        if seq <= self.record.reserved_through {
            return Ok(());
        }
        let mut next = self.record.clone();
        next.reserved_through = seq
            .checked_add(RESERVATION_BLOCK - 1)
            .ok_or_else(|| std::io::Error::other("writer sequence space is exhausted"))?;
        self.replace(next).map_err(std::io::Error::other)
    }
}

/// The recorded writer with the key copy it names, read before the database check.
pub struct StoredWriter {
    record: WriterRecord,
    key: Option<Vec<u8>>,
}

/// Reads the writer record and its key. Key stores may block, so callers run this off the
/// async runtime.
pub fn read_stored_writer(
    files: &SyncFiles,
    keys: &dyn WriterKeyStore,
) -> Result<Option<StoredWriter>, String> {
    let Some(record) = read_record(&files.record_path())? else {
        return Ok(None);
    };
    let key = keys.read(&files.vault_id, &record.writer_id)?;
    Ok(Some(StoredWriter { record, key }))
}

/// What a successor writer inherits.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SuccessorPlan {
    predecessor: Option<WriterId>,
    paused: bool,
    retired_keys: Vec<String>,
}

impl SuccessorPlan {
    /// The plan that retires `record`.
    fn retiring(record: &WriterRecord) -> Self {
        let mut retired_keys = record.retired_keys.clone();
        if !retired_keys.contains(&record.writer_id) {
            retired_keys.push(record.writer_id.clone());
        }
        Self {
            predecessor: WriterId::from_hex(&record.writer_id),
            paused: record.paused,
            retired_keys,
        }
    }

    /// The plan that replaces a writer the database no longer accepts.
    pub fn after(writer: &ActiveWriter) -> Self {
        Self::retiring(writer.record())
    }
}

/// Outcome of the writer check.
pub enum WriterCheck {
    Ready(Box<ActiveWriter>),
    /// The database does not hold the recorded chain, or there is no record.
    Successor(SuccessorPlan),
}

/// Keeps the recorded writer when the database still holds its chain. A copied or restored
/// database, a lost key, or a writer the space no longer accepts needs a successor.
pub async fn check_writer(
    engine: &Engine,
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    files: &SyncFiles,
    stored: Option<StoredWriter>,
) -> Result<WriterCheck, String> {
    let Some(StoredWriter { record, key }) = stored else {
        return Ok(WriterCheck::Successor(SuccessorPlan::default()));
    };
    let key = key.and_then(|pkcs8| WriterKeyPair::from_pkcs8(&pkcs8).ok());
    let certificate = URL_SAFE_NO_PAD
        .decode(&record.certificate)
        .ok()
        .and_then(|bytes| SignedCertificate::decode(&bytes).ok());
    let (Some(key), Some(certificate)) = (key, certificate) else {
        return Ok(WriterCheck::Successor(SuccessorPlan::retiring(&record)));
    };
    let certificate_matches = certificate.certificate.writer_key == key.public_key()
        && certificate.certificate.person_key == ctx.anchor
        && certificate.certificate.writer_id().to_hex() == record.writer_id;
    if !certificate_matches {
        return Ok(WriterCheck::Successor(SuccessorPlan::retiring(&record)));
    }
    let head = engine
        .local_writer_head(conn, ctx.space, &key.public_key())
        .await
        .map_err(|error| format!("check sync writer: {error}"))?;
    if !record_matches(&record, head) {
        return Ok(WriterCheck::Successor(SuccessorPlan::retiring(&record)));
    }
    Ok(WriterCheck::Ready(Box::new(ActiveWriter {
        key,
        certificate,
        reservation: RecordReservation {
            path: files.record_path(),
            record,
        },
    })))
}

/// Creates a writer certified by the person key and makes its key and record durable, then
/// removes the keys it retires. Key stores may block, so callers run this off the async runtime.
pub fn create_writer(
    files: &SyncFiles,
    keys: &dyn WriterKeyStore,
    plan: SuccessorPlan,
    person: &PersonKeyPair,
    device_id: &str,
    now_ms: u64,
) -> Result<ActiveWriter, String> {
    let (pkcs8, key) =
        WriterKeyPair::generate().map_err(|error| format!("create writer key: {error}"))?;
    let created_at_ms = i64::try_from(now_ms).map_err(|_| "clock is out of range".to_string())?;
    let certificate = sign_certificate(
        &WriterCertificate {
            writer_key: key.public_key(),
            person_key: person.public_key(),
            device_id: device_id.to_string(),
            created_at_ms,
            predecessor: plan.predecessor,
        },
        person,
    )
    .map_err(|error| format!("certify writer: {error}"))?;
    let writer_id = certificate.certificate.writer_id().to_hex();
    keys.write(&files.vault_id, &writer_id, &pkcs8)?;
    let record = WriterRecord {
        schema_version: RECORD_SCHEMA_VERSION,
        writer_id,
        certificate: URL_SAFE_NO_PAD.encode(certificate.encode()),
        reserved_through: 0,
        last_committed_seq: 0,
        paused: plan.paused,
        retired_keys: plan.retired_keys,
    };
    let path = files.record_path();
    write_record(&path, &record)?;
    let mut writer = ActiveWriter {
        key,
        certificate,
        reservation: RecordReservation { path, record },
    };
    remove_retired_keys(&mut writer, files, keys)?;
    Ok(writer)
}

/// Removes keys of retired writers once the record of their successor is durable.
pub fn remove_retired_keys(
    writer: &mut ActiveWriter,
    files: &SyncFiles,
    keys: &dyn WriterKeyStore,
) -> Result<(), String> {
    if writer.record().retired_keys.is_empty() {
        return Ok(());
    }
    for retired in &writer.record().retired_keys {
        if *retired != writer.record().writer_id {
            keys.remove(&files.vault_id, retired)?;
        }
    }
    let mut next = writer.record().clone();
    next.retired_keys.clear();
    writer.reservation.replace(next)
}
