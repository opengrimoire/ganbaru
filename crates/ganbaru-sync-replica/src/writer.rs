//! The local writer: its signing key, its device-local record, and the high-water check that
//! detects a restored or copied database.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ganbaru_sync::local::LocalWriterHead;
use ganbaru_sync::{Engine, LocalWriter, SequenceReservation, SpaceContext, WriterState};
use ganbaru_sync_contracts::{
    DeviceKeyPair, DevicePublicKey, Digest32, SignedCertificate, WriterCertificate, WriterId,
    WriterKeyPair, sign_certificate,
};
use serde::{Deserialize, Serialize};
use sqlx::SqliteConnection;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

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
    /// The last authorization entry this installation reserved as the anchor, kept across
    /// successor writers of the same charter, so a database restored behind it can be detected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sequenced: Option<SequencedEntry>,
}

/// An authorization revision the anchor reserved before sealing its entry.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SequencedEntry {
    /// Hash of the charter the log starts from, base64url without padding.
    pub charter: String,
    /// Epoch of the entry; a handover the anchor seals opens the next one.
    #[serde(default)]
    pub epoch: u32,
    /// Revision of the entry.
    pub revision: u64,
    /// Entry hash, base64url without padding.
    pub entry_hash: String,
    /// The writer that reserved the entry was retired because the database lost part of its
    /// chain, so a selected head one revision behind is a restore, not a rolled back seal.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub restored: bool,
}

impl SequencedEntry {
    /// The decoded entry hash, or `None` when the record holds a malformed one.
    pub fn hash(&self) -> Option<Digest32> {
        decode_digest(&self.entry_hash)
    }

    /// The decoded charter hash, or `None` when the record holds a malformed one.
    pub fn charter(&self) -> Option<Digest32> {
        decode_digest(&self.charter)
    }
}

fn decode_digest(text: &str) -> Option<Digest32> {
    URL_SAFE_NO_PAD
        .decode(text)
        .ok()
        .and_then(|bytes| bytes.try_into().ok())
}

/// How the anchor's recorded high-water compares with the selected log.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SequencedCheck {
    /// The log holds the recorded entry, or nothing is recorded.
    Current,
    /// The record is ahead by exactly the entry it reserved last: that seal rolled back.
    RolledBack,
    /// The record is ahead by more than one entry, or names an epoch the selected log never
    /// reached, so the database is older than the log this installation sequenced: it was
    /// restored from a backup or copied.
    Restored {
        /// The revision the record reached.
        recorded: u64,
    },
}

/// The selected head the recorded high-water is compared with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelectedHead {
    /// Hash of the charter the selected chain starts from.
    pub charter: Digest32,
    /// Epoch of the selected head.
    pub epoch: u32,
    /// Revision of the selected head.
    pub revision: u64,
}

/// Compares the recorded high-water with the selected head. A reservation is written before its
/// seal commits, so being ahead by one is a rolled back seal, not a restore, unless the writer
/// that reserved it was retired because the database lost part of its chain. A record of another
/// charter belongs to a log a Reset cleared and is current. Every chain this installation sealed
/// an epoch of stays stored, and selection prefers the highest epoch, so a selected head of a
/// lower epoch than the record means the database lost that epoch.
pub fn sequenced_check(
    sequenced: Option<&SequencedEntry>,
    selected: SelectedHead,
) -> SequencedCheck {
    let Some(entry) = sequenced.filter(|entry| entry.charter() == Some(selected.charter)) else {
        return SequencedCheck::Current;
    };
    let next = selected.revision.saturating_add(1);
    if entry.revision == next && !entry.restored {
        SequencedCheck::RolledBack
    } else if entry.revision >= next || entry.epoch > selected.epoch {
        SequencedCheck::Restored {
            recorded: entry.revision,
        }
    } else {
        SequencedCheck::Current
    }
}

/// A secret the sync key store holds for one vault.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncKey<'a> {
    /// A writer signing key, by hex writer id.
    Writer(&'a str),
    /// The installation's device signing key, which certifies its writers.
    Device,
    /// The anchor's admin signing key, which signs its authorization proposals.
    Admin,
    /// The recovery seed, kept until the user saves the recovery kit.
    RecoverySeed,
}

/// Storage of sync secrets outside the vault.
pub trait SyncKeyStore: Send + Sync {
    fn read(&self, vault_id: &str, key: SyncKey<'_>) -> Result<Option<Vec<u8>>, String>;
    fn write(&self, vault_id: &str, key: SyncKey<'_>, secret: &[u8]) -> Result<(), String>;
    fn remove(&self, vault_id: &str, key: SyncKey<'_>) -> Result<(), String>;
}

fn key_reference(vault_id: &str, key: SyncKey<'_>) -> String {
    match key {
        SyncKey::Writer(writer_id) => format!("writer-key:{vault_id}:{writer_id}"),
        SyncKey::Device => format!("device-key:{vault_id}"),
        SyncKey::Admin => format!("admin-key:{vault_id}"),
        SyncKey::RecoverySeed => format!("recovery-seed:{vault_id}"),
    }
}

/// Desktop key store in the operating-system keyring.
#[cfg(desktop)]
pub struct KeyringStore;

#[cfg(desktop)]
impl KeyringStore {
    const SERVICE: &'static str = "com.ganbaru.sync";

    fn entry(vault_id: &str, key: SyncKey<'_>) -> Result<keyring::Entry, String> {
        keyring::Entry::new(Self::SERVICE, &key_reference(vault_id, key))
            .map_err(|error| format!("open sync key store: {error}"))
    }
}

#[cfg(desktop)]
impl SyncKeyStore for KeyringStore {
    fn read(&self, vault_id: &str, key: SyncKey<'_>) -> Result<Option<Vec<u8>>, String> {
        match Self::entry(vault_id, key)?.get_password() {
            Ok(encoded) => URL_SAFE_NO_PAD
                .decode(encoded)
                .map(Some)
                .map_err(|error| format!("decode stored sync key: {error}")),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(format!("read sync key: {error}")),
        }
    }

    fn write(&self, vault_id: &str, key: SyncKey<'_>, secret: &[u8]) -> Result<(), String> {
        Self::entry(vault_id, key)?
            .set_password(&URL_SAFE_NO_PAD.encode(secret))
            .map_err(|error| format!("store sync key: {error}"))
    }

    fn remove(&self, vault_id: &str, key: SyncKey<'_>) -> Result<(), String> {
        match Self::entry(vault_id, key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(format!("remove sync key: {error}")),
        }
    }
}

/// Key store of one private file per secret, used on Android and in tests.
pub struct FileKeyStore {
    pub directory: PathBuf,
}

impl FileKeyStore {
    fn path(&self, vault_id: &str, key: SyncKey<'_>) -> PathBuf {
        self.directory.join(format!(
            "{}.key",
            key_reference(vault_id, key).replace(':', "-")
        ))
    }
}

impl SyncKeyStore for FileKeyStore {
    fn read(&self, vault_id: &str, key: SyncKey<'_>) -> Result<Option<Vec<u8>>, String> {
        match std::fs::read(self.path(vault_id, key)) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(format!("read sync key: {error}")),
        }
    }

    fn write(&self, vault_id: &str, key: SyncKey<'_>, secret: &[u8]) -> Result<(), String> {
        ganbaru_handoff::pairing::write_private_file_atomically(&self.path(vault_id, key), secret)
    }

    fn remove(&self, vault_id: &str, key: SyncKey<'_>) -> Result<(), String> {
        match std::fs::remove_file(self.path(vault_id, key)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!("remove sync key: {error}")),
        }
    }
}

/// Key store that keeps secrets in memory only, for tests and hosts without a keyring.
#[derive(Debug, Default)]
pub struct MemoryKeyStore {
    secrets: Mutex<HashMap<String, Vec<u8>>>,
}

impl MemoryKeyStore {
    fn secrets(&self) -> Result<std::sync::MutexGuard<'_, HashMap<String, Vec<u8>>>, String> {
        self.secrets
            .lock()
            .map_err(|_| "sync memory key store is poisoned".to_string())
    }
}

impl SyncKeyStore for MemoryKeyStore {
    fn read(&self, vault_id: &str, key: SyncKey<'_>) -> Result<Option<Vec<u8>>, String> {
        Ok(self.secrets()?.get(&key_reference(vault_id, key)).cloned())
    }

    fn write(&self, vault_id: &str, key: SyncKey<'_>, secret: &[u8]) -> Result<(), String> {
        self.secrets()?
            .insert(key_reference(vault_id, key), secret.to_vec());
        Ok(())
    }

    fn remove(&self, vault_id: &str, key: SyncKey<'_>) -> Result<(), String> {
        self.secrets()?.remove(&key_reference(vault_id, key));
        Ok(())
    }
}

/// The installation's device key for a vault, or `None` before the installation paired or
/// founded the vault's space. Key stores may block, so callers run this off the async runtime.
pub fn read_device_key(
    keys: &dyn SyncKeyStore,
    vault_id: &str,
) -> Result<Option<DeviceKeyPair>, String> {
    keys.read(vault_id, SyncKey::Device)?
        .map(|pkcs8| {
            DeviceKeyPair::from_pkcs8(&pkcs8)
                .map_err(|error| format!("decode sync device key: {error}"))
        })
        .transpose()
}

/// The installation's device key for a vault, created once and never replaced, because the
/// space's log enrolls its public half. Key stores may block, so callers run this off the async
/// runtime.
pub fn ensure_device_key(keys: &dyn SyncKeyStore, vault_id: &str) -> Result<DeviceKeyPair, String> {
    if let Some(device) = read_device_key(keys, vault_id)? {
        return Ok(device);
    }
    let (pkcs8, device) =
        DeviceKeyPair::generate().map_err(|error| format!("create sync device key: {error}"))?;
    keys.write(vault_id, SyncKey::Device, &pkcs8)?;
    Ok(device)
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

    /// Directory of the vault's other device-local sync files.
    pub fn vault_directory(&self) -> PathBuf {
        self.directory.join(&self.vault_id)
    }

    /// The proposal outbox of this installation.
    pub fn outbox_path(&self) -> PathBuf {
        self.vault_directory().join("outbox.json")
    }

    /// The devices this installation quarantined.
    pub fn quarantine_path(&self) -> PathBuf {
        self.vault_directory().join("quarantine.json")
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
    if !names_writer(&record.writer_id) || !record.retired_keys.iter().all(|id| names_writer(id)) {
        return Err("sync writer record names an invalid writer id".to_string());
    }
    Ok(Some(record))
}

/// Whether `id` is the canonical lowercase hex form of a writer id. Record ids name key store
/// entries and Android key files, so only the form this installation writes is accepted.
fn names_writer(id: &str) -> bool {
    WriterId::from_hex(id).is_some_and(|writer| writer.to_hex() == id)
}

pub(crate) fn write_record(path: &Path, record: &WriterRecord) -> Result<(), String> {
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

    /// Clears the recorded high-water when the space starts from another charter, because it
    /// then describes a log a Reset cleared. A space without a charter keeps it: the database
    /// was restored from before the space was founded, and founding refuses to start a new log
    /// over it until Reset sync state removes the record.
    pub fn settle_sequenced(&mut self, charter: Option<Digest32>) -> Result<(), String> {
        let (Some(sequenced), Some(charter)) =
            (self.reservation.record.sequenced.as_ref(), charter)
        else {
            return Ok(());
        };
        if sequenced.charter() == Some(charter) {
            return Ok(());
        }
        let mut next = self.reservation.record.clone();
        next.sequenced = None;
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

    fn reserve_entry(
        &mut self,
        charter: Digest32,
        epoch: u32,
        revision: u64,
        entry_hash: Digest32,
    ) -> std::io::Result<()> {
        let entry = SequencedEntry {
            charter: URL_SAFE_NO_PAD.encode(charter),
            epoch,
            revision,
            entry_hash: URL_SAFE_NO_PAD.encode(entry_hash),
            restored: false,
        };
        // A high-water of another charter belongs to a log a Reset cleared, so it never holds
        // back the current one. An entry of another epoch replaces the record even at a lower
        // revision: the selected epoch never falls below one this installation sealed unless
        // the database was restored, and then the service seals only the self-handover, which
        // continues from the older head.
        let current = self
            .record
            .sequenced
            .as_ref()
            .filter(|current| current.charter == entry.charter && current.epoch == epoch);
        if current.is_some_and(|current| current.revision > revision || *current == entry) {
            return Ok(());
        }
        let mut next = self.record.clone();
        next.sequenced = Some(entry);
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
    keys: &dyn SyncKeyStore,
) -> Result<Option<StoredWriter>, String> {
    let Some(record) = read_record(&files.record_path())? else {
        return Ok(None);
    };
    let key = keys.read(&files.vault_id, SyncKey::Writer(&record.writer_id))?;
    Ok(Some(StoredWriter { record, key }))
}

/// What a successor writer inherits.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SuccessorPlan {
    predecessor: Option<WriterId>,
    paused: bool,
    retired_keys: Vec<String>,
    sequenced: Option<SequencedEntry>,
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
            sequenced: record.sequenced.clone(),
        }
    }

    /// The plan that retires `record` because the database lost part of its chain or of the
    /// log it sequenced: the successor keeps the high-water marked as restored, so the anchor
    /// treats a head one revision behind it as a restore too.
    fn restored(record: &WriterRecord) -> Self {
        let mut plan = Self::retiring(record);
        if let Some(sequenced) = plan.sequenced.as_mut() {
            sequenced.restored = true;
        }
        plan
    }

    /// The last authorization entry the retired record reserved, which the successor keeps.
    pub fn sequenced(&self) -> Option<&SequencedEntry> {
        self.sequenced.as_ref()
    }

    /// The plan that replaces a writer the database no longer accepts.
    pub fn after(writer: &ActiveWriter) -> Self {
        Self::retiring(writer.record())
    }

    /// The plan that replaces the writer of an anchor whose database was restored behind the
    /// log it sequenced.
    pub fn after_restore(writer: &ActiveWriter) -> Self {
        Self::restored(writer.record())
    }
}

/// Outcome of the writer check.
pub enum WriterCheck {
    Ready(Box<ActiveWriter>),
    /// The database does not hold the recorded chain, or there is no record.
    Successor(SuccessorPlan),
}

/// Keeps the recorded writer when the database still holds its chain. A copied or restored
/// database, a lost key, a writer certified by another device key, or a writer the space no
/// longer accepts needs a successor.
pub async fn check_writer(
    engine: &Engine,
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    files: &SyncFiles,
    device_key: &DevicePublicKey,
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
        && certificate.certificate.device_key == *device_key
        && certificate.certificate.writer_id().to_hex() == record.writer_id;
    if !certificate_matches {
        return Ok(WriterCheck::Successor(SuccessorPlan::retiring(&record)));
    }
    let head = engine
        .local_writer_head(conn, ctx.space, &key.public_key())
        .await
        .map_err(|error| format!("check sync writer: {error}"))?;
    if !record_matches(&record, head) {
        return Ok(WriterCheck::Successor(SuccessorPlan::restored(&record)));
    }
    // A database behind the anchor's authorization high-water lost entries members may hold
    // at this writer's later sequences, so the writer must not seal again even when its own
    // committed head survived, for instance when its commit was never recorded.
    if let Some(view) = engine
        .authorization(conn, ctx)
        .await
        .map_err(|error| format!("read sync authorization: {error}"))?
        && matches!(
            sequenced_check(
                record.sequenced.as_ref(),
                SelectedHead {
                    charter: view.charter,
                    epoch: view.epoch,
                    revision: view.revision,
                },
            ),
            SequencedCheck::Restored { .. }
        )
    {
        return Ok(WriterCheck::Successor(SuccessorPlan::restored(&record)));
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

/// Creates a writer certified by the device key and makes its key and record durable, then
/// removes the keys it retires. Key stores may block, so callers run this off the async runtime.
pub fn create_writer(
    files: &SyncFiles,
    keys: &dyn SyncKeyStore,
    plan: SuccessorPlan,
    device: &DeviceKeyPair,
    device_id: &str,
    now_ms: u64,
) -> Result<ActiveWriter, String> {
    let (pkcs8, key) =
        WriterKeyPair::generate().map_err(|error| format!("create writer key: {error}"))?;
    let created_at_ms = i64::try_from(now_ms).map_err(|_| "clock is out of range".to_string())?;
    let certificate = sign_certificate(
        &WriterCertificate {
            writer_key: key.public_key(),
            device_id: device_id.to_string(),
            device_key: device.public_key(),
            created_at_ms,
            predecessor: plan.predecessor,
        },
        device,
    )
    .map_err(|error| format!("certify writer: {error}"))?;
    let writer_id = certificate.certificate.writer_id().to_hex();
    keys.write(&files.vault_id, SyncKey::Writer(&writer_id), &pkcs8)?;
    let record = WriterRecord {
        schema_version: RECORD_SCHEMA_VERSION,
        writer_id,
        certificate: URL_SAFE_NO_PAD.encode(certificate.encode()),
        reserved_through: 0,
        last_committed_seq: 0,
        paused: plan.paused,
        retired_keys: plan.retired_keys,
        sequenced: plan.sequenced,
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
    keys: &dyn SyncKeyStore,
) -> Result<(), String> {
    if writer.record().retired_keys.is_empty() {
        return Ok(());
    }
    for retired in &writer.record().retired_keys {
        if *retired != writer.record().writer_id {
            keys.remove(&files.vault_id, SyncKey::Writer(retired))?;
        }
    }
    let mut next = writer.record().clone();
    next.retired_keys.clear();
    writer.reservation.replace(next)
}
