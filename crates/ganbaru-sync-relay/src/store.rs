//! Relay persistence: spaces, epochs, sealed records with per-writer floors, sealed asset
//! chunks, setup tokens, and retention by quota and age.
//!
//! Every write that a refusal can stop runs in one transaction, so a refused request leaves the
//! store as it was.

use ganbaru_handoff::protocol::relay::{
    MAX_RELAY_ADMIN_KEYS, MAX_RELAY_EPOCH_ENTRY_BYTES, MAX_RELAY_PAGE_RECORDS, RELAY_PAGE_BYTES,
    RelayAssetOutcome, RelayPushOutcome, RelayRefusalCode, RelaySetupToken, constant_time_equal,
};
use ganbaru_sync_contracts::bounds::MAX_WRITERS_PER_SPACE;
use ganbaru_sync_contracts::keys::PUBLIC_KEY_BYTES;
use ganbaru_sync_contracts::relay::RELAY_ASSET_ID_BYTES;
use ganbaru_sync_contracts::{
    AdminPublicKey, RelayAccessPublicKey, RelayAssetHeader, RelayRecordHeader, SpaceId, WriterId,
};
use sha2::{Digest, Sha256};
use sqlx::sqlite::{
    SqliteConnectOptions, SqliteConnection, SqliteJournalMode, SqlitePool, SqlitePoolOptions,
    SqliteSynchronous,
};
use sqlx::{Connection, Row};
use std::collections::HashMap;
use std::fmt;
use std::path::Path;
use std::time::Duration;

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// How long a write waits for a lock held by another process, such as `setup`.
const WRITE_CONTENTION_TIMEOUT: Duration = Duration::from_secs(5);
/// Rows retention examines per query.
const RETENTION_BATCH: i64 = 256;

/// Default storage quota of one space.
pub const DEFAULT_QUOTA_BYTES: u64 = 2 * 1024 * 1024 * 1024;
/// Smallest quota a relay accepts, so a space always holds many pages.
pub const MIN_QUOTA_BYTES: u64 = 64 * 1024 * 1024;
/// Default age after which records and unread assets are trimmed.
pub const DEFAULT_MAX_AGE: Duration = Duration::from_secs(90 * 24 * 60 * 60);
/// Shortest maximum age a relay accepts.
pub const MIN_MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60);

/// When stored data is trimmed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RetentionPolicy {
    /// Bytes of records and asset chunks one space may hold.
    pub quota_bytes: u64,
    /// Age after which a record, or an asset not read since, is trimmed.
    pub max_age: Duration,
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self {
            quota_bytes: DEFAULT_QUOTA_BYTES,
            max_age: DEFAULT_MAX_AGE,
        }
    }
}

/// A store operation that failed.
#[derive(Debug)]
pub enum StoreError {
    /// The request is refused for a reason the device can act on.
    Refused(RelayRefusalCode, &'static str),
    /// The database failed.
    Database(sqlx::Error),
}

impl fmt::Display for StoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused(code, reason) => write!(formatter, "refused ({code:?}): {reason}"),
            Self::Database(error) => write!(formatter, "relay database: {error}"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<sqlx::Error> for StoreError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

/// Result of a store operation.
pub type StoreResult<T> = Result<T, StoreError>;

fn refuse<T>(code: RelayRefusalCode, reason: &'static str) -> StoreResult<T> {
    Err(StoreError::Refused(code, reason))
}

fn to_sql(value: u64, reason: &'static str) -> StoreResult<i64> {
    i64::try_from(value).map_err(|_| StoreError::Refused(RelayRefusalCode::BadRequest, reason))
}

fn from_sql(value: i64) -> u64 {
    u64::try_from(value).unwrap_or(0)
}

fn epoch_from_sql(value: i64) -> u32 {
    u32::try_from(value).unwrap_or(0)
}

fn writer_from_sql(bytes: &[u8]) -> StoreResult<WriterId> {
    WriterId::from_slice(bytes)
        .ok_or_else(|| StoreError::Database(sqlx::Error::Protocol("stored writer id".into())))
}

fn token_hash(token: &RelaySetupToken) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

/// What a claim creates.
#[derive(Clone, Debug)]
pub struct ClaimRequest<'a> {
    /// Space to create.
    pub space: SpaceId,
    /// One-time setup token.
    pub token: &'a RelaySetupToken,
    /// First epoch the relay accepts credentials for.
    pub epoch: u32,
    /// Relay access key of that epoch.
    pub access_key: RelayAccessPublicKey,
    /// Initial admin set.
    pub admins: &'a [AdminPublicKey],
    /// Initial admin counter.
    pub counter: u64,
}

/// A change an admin makes, after its signature verified.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdminChange {
    /// Adds a key to the admin set.
    AddKey(AdminPublicKey),
    /// Removes a key from the admin set.
    RemoveKey(AdminPublicKey),
    /// Deletes stored records, floors, and assets.
    Wipe,
    /// Deletes the space.
    RemoveSpace,
}

/// Stored range of one writer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WriterRange {
    /// Writer id.
    pub writer: WriterId,
    /// Lowest stored sequence.
    pub floor: u64,
    /// Sequence the next record takes.
    pub next: u64,
}

/// State of a space.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpaceState {
    /// Current epoch.
    pub current_epoch: u32,
    /// Stored writer ranges, ordered by writer id.
    pub writers: Vec<WriterRange>,
    /// Bytes of stored records and chunks.
    pub stored_bytes: u64,
    /// Admin set.
    pub admins: Vec<AdminPublicKey>,
    /// Last accepted admin counter.
    pub admin_counter: u64,
}

/// One page of pulled records.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PullPage {
    /// Encoded records, per writer in sequence order.
    pub records: Vec<Vec<u8>>,
    /// Writers whose next wanted record was trimmed, with their floor.
    pub behind_floor: Vec<(WriterId, u64)>,
    /// Whether more records were left out of the page.
    pub more: bool,
}

/// One page of registered key epoch entries.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EpochPage {
    /// Current epoch.
    pub current_epoch: u32,
    /// Epochs with their entries, ascending.
    pub entries: Vec<(u32, Vec<u8>)>,
    /// Whether more entries were left out of the page.
    pub more: bool,
}

/// What authenticating with an epoch credential found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EpochCredential {
    /// Access key registered for the epoch.
    pub access_key: RelayAccessPublicKey,
    /// Current epoch of the space.
    pub current_epoch: u32,
}

/// The relay's SQLite store.
#[derive(Clone, Debug)]
pub struct RelayStore {
    pool: SqlitePool,
    retention: RetentionPolicy,
}

impl RelayStore {
    /// Opens or creates the store at `path` and applies its migrations.
    pub async fn open(path: &Path, retention: RetentionPolicy) -> Result<Self, String> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .foreign_keys(true)
            .busy_timeout(WRITE_CONTENTION_TIMEOUT)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Full)
            .pragma("trusted_schema", "OFF")
            .pragma("cell_size_check", "ON");
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .map_err(|error| format!("open relay database: {error}"))?;
        MIGRATOR
            .run(&pool)
            .await
            .map_err(|error| format!("migrate relay database: {error}"))?;
        Ok(Self { pool, retention })
    }

    /// Retention the store applies.
    pub fn retention(&self) -> RetentionPolicy {
        self.retention
    }

    /// Closes the pool.
    pub async fn close(&self) {
        self.pool.close().await;
    }

    /// Stores a setup token that can claim one space until `expires_at_ms`.
    pub async fn add_setup_token(
        &self,
        token: &RelaySetupToken,
        expires_at_ms: i64,
    ) -> StoreResult<()> {
        sqlx::query("INSERT INTO relay_setup_tokens (token_hash, expires_at_ms) VALUES (?, ?)")
            .bind(token_hash(token).as_slice())
            .bind(expires_at_ms)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Creates a space with a setup token, consuming the token.
    pub async fn claim(&self, claim: &ClaimRequest<'_>, now_ms: i64) -> StoreResult<()> {
        if claim.epoch == 0 {
            return refuse(
                RelayRefusalCode::BadRequest,
                "claim epoch must be at least 1",
            );
        }
        if claim.admins.is_empty() || claim.admins.len() > MAX_RELAY_ADMIN_KEYS {
            return refuse(
                RelayRefusalCode::AdminSet,
                "claim admin set has an invalid size",
            );
        }
        for (index, admin) in claim.admins.iter().enumerate() {
            if claim.admins[..index].contains(admin) {
                return refuse(RelayRefusalCode::AdminSet, "claim admin set repeats a key");
            }
        }
        let counter = to_sql(claim.counter, "admin counter is too large")?;
        let mut connection = self.pool.acquire().await?;
        let mut transaction = connection.begin().await?;
        sqlx::query("DELETE FROM relay_setup_tokens WHERE expires_at_ms <= ?")
            .bind(now_ms)
            .execute(&mut *transaction)
            .await?;
        let wanted = token_hash(claim.token);
        let stored: Vec<Vec<u8>> = sqlx::query("SELECT token_hash FROM relay_setup_tokens")
            .fetch_all(&mut *transaction)
            .await?
            .into_iter()
            .map(|row| row.try_get::<Vec<u8>, _>(0))
            .collect::<Result<_, _>>()?;
        let mut matched = None;
        for hash in stored {
            if constant_time_equal(&hash, &wanted) {
                matched = Some(hash);
            }
        }
        let Some(matched) = matched else {
            transaction.commit().await?;
            return refuse(
                RelayRefusalCode::Token,
                "setup token is unknown, expired, or used",
            );
        };
        if space_row(&mut transaction, &claim.space).await?.is_some() {
            transaction.commit().await?;
            return refuse(RelayRefusalCode::SpaceExists, "space is already claimed");
        }
        sqlx::query("DELETE FROM relay_setup_tokens WHERE token_hash = ?")
            .bind(matched.as_slice())
            .execute(&mut *transaction)
            .await?;
        sqlx::query(
            "INSERT INTO relay_spaces (space_id, current_epoch, admin_keys, admin_counter, \
             created_at_ms) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(claim.space.as_bytes().as_slice())
        .bind(i64::from(claim.epoch))
        .bind(join_keys(claim.admins))
        .bind(counter)
        .bind(now_ms)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "INSERT INTO relay_epochs (space_id, epoch, access_key, entry, registered_at_ms) \
             VALUES (?, ?, ?, NULL, ?)",
        )
        .bind(claim.space.as_bytes().as_slice())
        .bind(i64::from(claim.epoch))
        .bind(claim.access_key.as_bytes().as_slice())
        .bind(now_ms)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    /// Finds the access key a credential for `epoch` must verify against.
    pub async fn credential(&self, space: &SpaceId, epoch: u32) -> StoreResult<EpochCredential> {
        let mut connection = self.pool.acquire().await?;
        let Some(row) = space_row(&mut connection, space).await? else {
            return refuse(RelayRefusalCode::UnknownSpace, "space is not on this relay");
        };
        if epoch > row.current_epoch {
            return refuse(
                RelayRefusalCode::UnknownEpoch,
                "epoch is not registered yet",
            );
        }
        let key: Option<Vec<u8>> = sqlx::query_scalar(
            "SELECT access_key FROM relay_epochs WHERE space_id = ? AND epoch = ?",
        )
        .bind(space.as_bytes().as_slice())
        .bind(i64::from(epoch))
        .fetch_optional(&mut *connection)
        .await?;
        let Some(key) = key else {
            return refuse(
                RelayRefusalCode::StaleEpoch,
                "epoch predates the space on this relay",
            );
        };
        let access_key = RelayAccessPublicKey::from_slice(&key)
            .map_err(|_| StoreError::Database(sqlx::Error::Protocol("stored access key".into())))?;
        Ok(EpochCredential {
            access_key,
            current_epoch: row.current_epoch,
        })
    }

    /// Current epoch of a space.
    pub async fn current_epoch(&self, space: &SpaceId) -> StoreResult<u32> {
        let mut connection = self.pool.acquire().await?;
        match space_row(&mut connection, space).await? {
            Some(row) => Ok(row.current_epoch),
            None => refuse(RelayRefusalCode::UnknownSpace, "space is not on this relay"),
        }
    }

    /// Admin set of a space, for checking a signer before its signature.
    pub async fn admins(&self, space: &SpaceId) -> StoreResult<Vec<AdminPublicKey>> {
        let mut connection = self.pool.acquire().await?;
        match space_row(&mut connection, space).await? {
            Some(row) => Ok(row.admins),
            None => refuse(RelayRefusalCode::UnknownSpace, "space is not on this relay"),
        }
    }

    /// Applies a verified admin change if `signer` is an admin and `counter` is above the last
    /// accepted counter.
    pub async fn apply_admin(
        &self,
        space: &SpaceId,
        signer: &AdminPublicKey,
        counter: u64,
        change: AdminChange,
    ) -> StoreResult<()> {
        let counter_sql = to_sql(counter, "admin counter is too large")?;
        let mut connection = self.pool.acquire().await?;
        let mut transaction = connection.begin().await?;
        let Some(row) = space_row(&mut transaction, space).await? else {
            return refuse(RelayRefusalCode::UnknownSpace, "space is not on this relay");
        };
        if !row.admins.contains(signer) {
            return refuse(RelayRefusalCode::AdminSignature, "signer is not an admin");
        }
        if counter <= row.admin_counter {
            return refuse(
                RelayRefusalCode::AdminCounter,
                "admin counter was already used",
            );
        }
        let space_bytes = space.as_bytes().as_slice();
        match change {
            AdminChange::AddKey(key) => {
                let mut admins = row.admins;
                if !admins.contains(&key) {
                    if admins.len() >= MAX_RELAY_ADMIN_KEYS {
                        return refuse(RelayRefusalCode::AdminSet, "admin set is full");
                    }
                    admins.push(key);
                }
                set_admins(&mut transaction, space, &admins, counter_sql).await?;
            }
            AdminChange::RemoveKey(key) => {
                let mut admins = row.admins;
                let Some(position) = admins.iter().position(|admin| *admin == key) else {
                    return refuse(RelayRefusalCode::AdminSet, "key is not an admin");
                };
                if admins.len() == 1 {
                    return refuse(
                        RelayRefusalCode::AdminSet,
                        "the last admin cannot be removed",
                    );
                }
                admins.remove(position);
                set_admins(&mut transaction, space, &admins, counter_sql).await?;
            }
            AdminChange::Wipe => {
                for statement in [
                    "DELETE FROM relay_records WHERE space_id = ?",
                    "DELETE FROM relay_floors WHERE space_id = ?",
                    "DELETE FROM relay_assets WHERE space_id = ?",
                ] {
                    sqlx::query(statement)
                        .bind(space_bytes)
                        .execute(&mut *transaction)
                        .await?;
                }
                sqlx::query(
                    "UPDATE relay_spaces SET stored_bytes = 0, admin_counter = ? \
                     WHERE space_id = ?",
                )
                .bind(counter_sql)
                .bind(space_bytes)
                .execute(&mut *transaction)
                .await?;
            }
            AdminChange::RemoveSpace => {
                sqlx::query("DELETE FROM relay_spaces WHERE space_id = ?")
                    .bind(space_bytes)
                    .execute(&mut *transaction)
                    .await?;
            }
        }
        transaction.commit().await?;
        Ok(())
    }

    /// State of a space.
    pub async fn state(&self, space: &SpaceId) -> StoreResult<SpaceState> {
        let mut connection = self.pool.acquire().await?;
        let Some(row) = space_row(&mut connection, space).await? else {
            return refuse(RelayRefusalCode::UnknownSpace, "space is not on this relay");
        };
        let writers = writer_ranges(&mut connection, space).await?;
        Ok(SpaceState {
            current_epoch: row.current_epoch,
            writers,
            stored_bytes: row.stored_bytes,
            admins: row.admins,
            admin_counter: row.admin_counter,
        })
    }

    /// Stores a page of records sealed under `epoch`, which must be the current epoch. Each
    /// header must already match the space and epoch.
    pub async fn push(
        &self,
        space: &SpaceId,
        epoch: u32,
        records: &[(RelayRecordHeader, Vec<u8>)],
        now_ms: i64,
    ) -> StoreResult<Vec<RelayPushOutcome>> {
        let mut connection = self.pool.acquire().await?;
        let mut transaction = connection.begin().await?;
        let Some(row) = space_row(&mut transaction, space).await? else {
            return refuse(RelayRefusalCode::UnknownSpace, "space is not on this relay");
        };
        if epoch != row.current_epoch {
            return refuse(RelayRefusalCode::StaleEpoch, "a newer epoch is current");
        }
        let space_bytes = space.as_bytes().as_slice();
        let mut writer_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM relay_floors WHERE space_id = ?")
                .bind(space_bytes)
                .fetch_one(&mut *transaction)
                .await?;
        let mut floors: HashMap<WriterId, Option<(u64, u64)>> = HashMap::new();
        let mut outcomes = Vec::with_capacity(records.len());
        let mut added_bytes = 0i64;
        for (header, bytes) in records {
            if header.space != *space || header.epoch != epoch {
                return refuse(
                    RelayRefusalCode::BadRecord,
                    "record names another space or epoch",
                );
            }
            let seq = to_sql(header.seq, "record sequence is too large")?;
            let next_seq = to_sql(header.seq.saturating_add(1), "record sequence is too large")?;
            let writer_bytes = header.writer.as_bytes().as_slice();
            let range = match floors.get(&header.writer) {
                Some(range) => *range,
                None => {
                    let range = sqlx::query(
                        "SELECT floor, next_seq FROM relay_floors \
                         WHERE space_id = ? AND writer_id = ?",
                    )
                    .bind(space_bytes)
                    .bind(writer_bytes)
                    .fetch_optional(&mut *transaction)
                    .await?
                    .map(|row| -> StoreResult<(u64, u64)> {
                        Ok((
                            from_sql(row.try_get("floor")?),
                            from_sql(row.try_get("next_seq")?),
                        ))
                    })
                    .transpose()?;
                    floors.insert(header.writer, range);
                    range
                }
            };
            let outcome = match range {
                None => {
                    if usize::try_from(writer_count).unwrap_or(usize::MAX) >= MAX_WRITERS_PER_SPACE
                    {
                        return refuse(RelayRefusalCode::WriterLimit, "space has too many writers");
                    }
                    writer_count += 1;
                    sqlx::query(
                        "INSERT INTO relay_floors (space_id, writer_id, floor, next_seq) \
                         VALUES (?, ?, ?, ?)",
                    )
                    .bind(space_bytes)
                    .bind(writer_bytes)
                    .bind(seq)
                    .bind(next_seq)
                    .execute(&mut *transaction)
                    .await?;
                    floors.insert(
                        header.writer,
                        Some((header.seq, header.seq.saturating_add(1))),
                    );
                    RelayPushOutcome::Stored
                }
                Some((floor, _)) if header.seq < floor => RelayPushOutcome::Trimmed,
                Some((_, next)) if header.seq < next => {
                    let stored: Vec<u8> = sqlx::query_scalar(
                        "SELECT record FROM relay_records \
                         WHERE space_id = ? AND writer_id = ? AND seq = ?",
                    )
                    .bind(space_bytes)
                    .bind(writer_bytes)
                    .bind(seq)
                    .fetch_one(&mut *transaction)
                    .await?;
                    if stored == *bytes {
                        RelayPushOutcome::Duplicate
                    } else {
                        RelayPushOutcome::SlotTaken
                    }
                }
                Some((floor, next)) if header.seq == next => {
                    sqlx::query(
                        "UPDATE relay_floors SET next_seq = ? WHERE space_id = ? AND writer_id = ?",
                    )
                    .bind(next_seq)
                    .bind(space_bytes)
                    .bind(writer_bytes)
                    .execute(&mut *transaction)
                    .await?;
                    floors.insert(header.writer, Some((floor, next + 1)));
                    RelayPushOutcome::Stored
                }
                Some(_) => RelayPushOutcome::Gap,
            };
            if outcome == RelayPushOutcome::Stored {
                sqlx::query(
                    "INSERT INTO relay_records (space_id, writer_id, seq, epoch, record, \
                     stored_at_ms) VALUES (?, ?, ?, ?, ?, ?)",
                )
                .bind(space_bytes)
                .bind(writer_bytes)
                .bind(seq)
                .bind(i64::from(epoch))
                .bind(bytes.as_slice())
                .bind(now_ms)
                .execute(&mut *transaction)
                .await?;
                added_bytes += i64::try_from(bytes.len()).unwrap_or(i64::MAX);
            }
            outcomes.push(outcome);
        }
        add_stored_bytes(&mut transaction, space, added_bytes).await?;
        transaction.commit().await?;
        drop(connection);
        self.enforce_retention(space, now_ms).await?;
        Ok(outcomes)
    }

    /// Records after `have` that a credential for `epoch` may read: only records sealed under
    /// `epoch` or earlier. A writer's records stop at its first later-epoch record.
    pub async fn pull(
        &self,
        space: &SpaceId,
        epoch: u32,
        have: &HashMap<WriterId, u64>,
    ) -> StoreResult<PullPage> {
        let mut connection = self.pool.acquire().await?;
        if space_row(&mut connection, space).await?.is_none() {
            return refuse(RelayRefusalCode::UnknownSpace, "space is not on this relay");
        }
        let space_bytes = space.as_bytes().as_slice();
        let mut page = PullPage::default();
        let mut page_bytes = 0usize;
        'writers: for range in writer_ranges(&mut connection, space).await? {
            let want = have
                .get(&range.writer)
                .map_or(1, |seq| seq.saturating_add(1));
            if want < range.floor {
                page.behind_floor.push((range.writer, range.floor));
                continue;
            }
            if want >= range.next {
                continue;
            }
            let room = MAX_RELAY_PAGE_RECORDS - page.records.len();
            if room == 0 {
                page.more = true;
                break;
            }
            let rows = sqlx::query(
                "SELECT seq, epoch, length(record) AS size FROM relay_records \
                 WHERE space_id = ? AND writer_id = ? AND seq >= ? ORDER BY seq LIMIT ?",
            )
            .bind(space_bytes)
            .bind(range.writer.as_bytes().as_slice())
            .bind(to_sql(want, "vector sequence is too large")?)
            .bind(i64::try_from(room).unwrap_or(i64::MAX))
            .fetch_all(&mut *connection)
            .await?;
            let mut last = None;
            let mut stopped = false;
            for row in &rows {
                let seq: i64 = row.try_get("seq")?;
                let record_epoch: i64 = row.try_get("epoch")?;
                let size = usize::try_from(row.try_get::<i64, _>("size")?).unwrap_or(usize::MAX);
                if record_epoch > i64::from(epoch) {
                    stopped = true;
                    break;
                }
                if page_bytes.saturating_add(size) > RELAY_PAGE_BYTES
                    && (!page.records.is_empty() || last.is_some())
                {
                    page.more = true;
                    stopped = true;
                    break;
                }
                page_bytes = page_bytes.saturating_add(size);
                last = Some(seq);
            }
            if let Some(last) = last {
                let records: Vec<Vec<u8>> = sqlx::query_scalar(
                    "SELECT record FROM relay_records \
                     WHERE space_id = ? AND writer_id = ? AND seq >= ? AND seq <= ? ORDER BY seq",
                )
                .bind(space_bytes)
                .bind(range.writer.as_bytes().as_slice())
                .bind(to_sql(want, "vector sequence is too large")?)
                .bind(last)
                .fetch_all(&mut *connection)
                .await?;
                page.records.extend(records);
            }
            if page.more {
                break 'writers;
            }
            if !stopped && rows.len() >= room {
                page.more = true;
                break;
            }
        }
        Ok(page)
    }

    /// Key epoch entries registered after `after`.
    pub async fn epochs(&self, space: &SpaceId, after: u32) -> StoreResult<EpochPage> {
        let mut connection = self.pool.acquire().await?;
        let Some(row) = space_row(&mut connection, space).await? else {
            return refuse(RelayRefusalCode::UnknownSpace, "space is not on this relay");
        };
        let rows = sqlx::query(
            "SELECT epoch, length(entry) AS size FROM relay_epochs \
             WHERE space_id = ? AND epoch > ? AND entry IS NOT NULL ORDER BY epoch",
        )
        .bind(space.as_bytes().as_slice())
        .bind(i64::from(after))
        .fetch_all(&mut *connection)
        .await?;
        let mut page = EpochPage {
            current_epoch: row.current_epoch,
            ..EpochPage::default()
        };
        let mut total = 0usize;
        for row in rows {
            let epoch: i64 = row.try_get("epoch")?;
            let size = usize::try_from(row.try_get::<i64, _>("size")?).unwrap_or(usize::MAX);
            if !page.entries.is_empty() && total.saturating_add(size) > RELAY_PAGE_BYTES {
                page.more = true;
                break;
            }
            total = total.saturating_add(size);
            let entry: Vec<u8> = sqlx::query_scalar(
                "SELECT entry FROM relay_epochs WHERE space_id = ? AND epoch = ?",
            )
            .bind(space.as_bytes().as_slice())
            .bind(epoch)
            .fetch_one(&mut *connection)
            .await?;
            page.entries.push((epoch_from_sql(epoch), entry));
        }
        Ok(page)
    }

    /// Registers `new_epoch` with its access key and key epoch entry. The registering
    /// credential must hold the current epoch and `new_epoch` must be the current epoch plus
    /// one; repeating a registration that already happened with the same key succeeds.
    pub async fn register_epoch(
        &self,
        space: &SpaceId,
        session_epoch: u32,
        new_epoch: u32,
        access_key: &RelayAccessPublicKey,
        entry: &[u8],
        now_ms: i64,
    ) -> StoreResult<()> {
        if entry.is_empty() || entry.len() > MAX_RELAY_EPOCH_ENTRY_BYTES {
            return refuse(
                RelayRefusalCode::BadRequest,
                "epoch entry has an invalid size",
            );
        }
        let mut connection = self.pool.acquire().await?;
        let mut transaction = connection.begin().await?;
        let Some(row) = space_row(&mut transaction, space).await? else {
            return refuse(RelayRefusalCode::UnknownSpace, "space is not on this relay");
        };
        let space_bytes = space.as_bytes().as_slice();
        if new_epoch == row.current_epoch && session_epoch.checked_add(1) == Some(new_epoch) {
            let stored: Option<Vec<u8>> = sqlx::query_scalar(
                "SELECT access_key FROM relay_epochs WHERE space_id = ? AND epoch = ?",
            )
            .bind(space_bytes)
            .bind(i64::from(new_epoch))
            .fetch_optional(&mut *transaction)
            .await?;
            if stored.as_deref() == Some(access_key.as_bytes().as_slice()) {
                return Ok(());
            }
        }
        if session_epoch != row.current_epoch {
            return refuse(RelayRefusalCode::StaleEpoch, "a newer epoch is current");
        }
        if row.current_epoch.checked_add(1) != Some(new_epoch) {
            return refuse(
                RelayRefusalCode::UnknownEpoch,
                "only the next epoch can be registered",
            );
        }
        sqlx::query(
            "INSERT INTO relay_epochs (space_id, epoch, access_key, entry, registered_at_ms) \
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(space_bytes)
        .bind(i64::from(new_epoch))
        .bind(access_key.as_bytes().as_slice())
        .bind(entry)
        .bind(now_ms)
        .execute(&mut *transaction)
        .await?;
        sqlx::query("UPDATE relay_spaces SET current_epoch = ? WHERE space_id = ?")
            .bind(i64::from(new_epoch))
            .bind(space_bytes)
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        Ok(())
    }

    /// Stores one asset chunk sealed under `epoch`, which must be current.
    pub async fn put_asset_chunk(
        &self,
        space: &SpaceId,
        epoch: u32,
        header: &RelayAssetHeader,
        chunk: &[u8],
        now_ms: i64,
    ) -> StoreResult<RelayAssetOutcome> {
        if header.space != *space || header.epoch != epoch {
            return refuse(
                RelayRefusalCode::BadRecord,
                "chunk names another space or epoch",
            );
        }
        let mut connection = self.pool.acquire().await?;
        let mut transaction = connection.begin().await?;
        let Some(row) = space_row(&mut transaction, space).await? else {
            return refuse(RelayRefusalCode::UnknownSpace, "space is not on this relay");
        };
        if epoch != row.current_epoch {
            return refuse(RelayRefusalCode::StaleEpoch, "a newer epoch is current");
        }
        let space_bytes = space.as_bytes().as_slice();
        let asset_bytes = header.asset.as_bytes().as_slice();
        let counts: Vec<i64> = sqlx::query_scalar(
            "SELECT DISTINCT chunk_count FROM relay_assets WHERE space_id = ? AND asset_id = ?",
        )
        .bind(space_bytes)
        .bind(asset_bytes)
        .fetch_all(&mut *transaction)
        .await?;
        if counts.iter().any(|count| *count != i64::from(header.count)) {
            return refuse(
                RelayRefusalCode::AssetMismatch,
                "chunk count differs from the asset",
            );
        }
        let stored: Option<Vec<u8>> = sqlx::query_scalar(
            "SELECT chunk FROM relay_assets WHERE space_id = ? AND asset_id = ? AND chunk_index = ?",
        )
        .bind(space_bytes)
        .bind(asset_bytes)
        .bind(i64::from(header.index))
        .fetch_optional(&mut *transaction)
        .await?;
        if let Some(stored) = stored {
            return Ok(if stored == chunk {
                RelayAssetOutcome::Duplicate
            } else {
                RelayAssetOutcome::SlotTaken
            });
        }
        let last_read: Option<i64> = sqlx::query_scalar(
            "SELECT MAX(last_read_at_ms) FROM relay_assets WHERE space_id = ? AND asset_id = ?",
        )
        .bind(space_bytes)
        .bind(asset_bytes)
        .fetch_one(&mut *transaction)
        .await?;
        sqlx::query(
            "INSERT INTO relay_assets (space_id, asset_id, chunk_index, chunk_count, epoch, chunk, \
             stored_at_ms, last_read_at_ms) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(space_bytes)
        .bind(asset_bytes)
        .bind(i64::from(header.index))
        .bind(i64::from(header.count))
        .bind(i64::from(epoch))
        .bind(chunk)
        .bind(now_ms)
        .bind(last_read.unwrap_or(now_ms).max(now_ms))
        .execute(&mut *transaction)
        .await?;
        add_stored_bytes(
            &mut transaction,
            space,
            i64::try_from(chunk.len()).unwrap_or(i64::MAX),
        )
        .await?;
        transaction.commit().await?;
        drop(connection);
        self.enforce_retention(space, now_ms).await?;
        Ok(RelayAssetOutcome::Stored)
    }

    /// One asset chunk a credential for `epoch` may read. Reading marks the asset as read.
    pub async fn asset_chunk(
        &self,
        space: &SpaceId,
        epoch: u32,
        asset: &[u8; RELAY_ASSET_ID_BYTES],
        index: u32,
        now_ms: i64,
    ) -> StoreResult<Vec<u8>> {
        let mut connection = self.pool.acquire().await?;
        let mut transaction = connection.begin().await?;
        if space_row(&mut transaction, space).await?.is_none() {
            return refuse(RelayRefusalCode::UnknownSpace, "space is not on this relay");
        }
        let space_bytes = space.as_bytes().as_slice();
        let row = sqlx::query(
            "SELECT chunk, epoch FROM relay_assets \
             WHERE space_id = ? AND asset_id = ? AND chunk_index = ?",
        )
        .bind(space_bytes)
        .bind(asset.as_slice())
        .bind(i64::from(index))
        .fetch_optional(&mut *transaction)
        .await?;
        let Some(row) = row else {
            return refuse(RelayRefusalCode::NotFound, "chunk is not stored");
        };
        let chunk_epoch: i64 = row.try_get("epoch")?;
        if chunk_epoch > i64::from(epoch) {
            return refuse(RelayRefusalCode::NotFound, "chunk is not stored");
        }
        let chunk: Vec<u8> = row.try_get("chunk")?;
        sqlx::query(
            "UPDATE relay_assets SET last_read_at_ms = ? WHERE space_id = ? AND asset_id = ?",
        )
        .bind(now_ms)
        .bind(space_bytes)
        .bind(asset.as_slice())
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(chunk)
    }

    /// The assets among `assets` stored completely and readable by a credential for `epoch`.
    pub async fn complete_assets(
        &self,
        space: &SpaceId,
        epoch: u32,
        assets: &[[u8; RELAY_ASSET_ID_BYTES]],
    ) -> StoreResult<Vec<[u8; RELAY_ASSET_ID_BYTES]>> {
        let mut connection = self.pool.acquire().await?;
        if space_row(&mut connection, space).await?.is_none() {
            return refuse(RelayRefusalCode::UnknownSpace, "space is not on this relay");
        }
        let mut complete = Vec::new();
        for asset in assets {
            let found: Option<i64> = sqlx::query_scalar(
                "SELECT 1 FROM relay_assets WHERE space_id = ? AND asset_id = ? \
                 GROUP BY asset_id HAVING COUNT(*) = MAX(chunk_count) AND MAX(epoch) <= ?",
            )
            .bind(space.as_bytes().as_slice())
            .bind(asset.as_slice())
            .bind(i64::from(epoch))
            .fetch_optional(&mut *connection)
            .await?;
            if found.is_some() && !complete.contains(asset) {
                complete.push(*asset);
            }
        }
        Ok(complete)
    }

    /// Trims a space by age, then by quota, oldest first: records in arrival order, which is
    /// always the front of their writer, and assets whole in order of last read.
    pub async fn enforce_retention(&self, space: &SpaceId, now_ms: i64) -> StoreResult<()> {
        let max_age_ms = i64::try_from(self.retention.max_age.as_millis()).unwrap_or(i64::MAX);
        let cutoff = now_ms.saturating_sub(max_age_ms);
        let quota = i64::try_from(self.retention.quota_bytes).unwrap_or(i64::MAX);
        let mut connection = self.pool.acquire().await?;
        let mut transaction = connection.begin().await?;
        let space_bytes = space.as_bytes().as_slice();
        loop {
            let Some(row) = space_row(&mut transaction, space).await? else {
                break;
            };
            let stored = i64::try_from(row.stored_bytes).unwrap_or(i64::MAX);
            let record = sqlx::query(
                "SELECT arrival, writer_id, seq, stored_at_ms, length(record) AS size \
                 FROM relay_records WHERE space_id = ? ORDER BY arrival LIMIT 1",
            )
            .bind(space_bytes)
            .fetch_optional(&mut *transaction)
            .await?;
            let asset = sqlx::query(
                "SELECT asset_id, MAX(last_read_at_ms) AS read_at, SUM(length(chunk)) AS size \
                 FROM relay_assets WHERE space_id = ? GROUP BY asset_id \
                 ORDER BY read_at, asset_id LIMIT 1",
            )
            .bind(space_bytes)
            .fetch_optional(&mut *transaction)
            .await?;
            let record_time = record
                .as_ref()
                .map(|row| row.try_get::<i64, _>("stored_at_ms"))
                .transpose()?;
            let asset_time = asset
                .as_ref()
                .map(|row| row.try_get::<i64, _>("read_at"))
                .transpose()?;
            let Some(evict_record) =
                choose_eviction(record_time, asset_time, cutoff, stored > quota)
            else {
                break;
            };
            if evict_record {
                let Some(record) = record else { break };
                evict_front_record(&mut transaction, space, &record).await?;
            } else {
                let Some(asset) = asset else { break };
                let asset_id: Vec<u8> = asset.try_get("asset_id")?;
                let size: i64 = asset.try_get("size")?;
                sqlx::query("DELETE FROM relay_assets WHERE space_id = ? AND asset_id = ?")
                    .bind(space_bytes)
                    .bind(asset_id.as_slice())
                    .execute(&mut *transaction)
                    .await?;
                add_stored_bytes(&mut transaction, space, -size).await?;
            }
        }
        transaction.commit().await?;
        Ok(())
    }

    /// Trims every space and deletes expired setup tokens.
    pub async fn sweep(&self, now_ms: i64) -> StoreResult<()> {
        sqlx::query("DELETE FROM relay_setup_tokens WHERE expires_at_ms <= ?")
            .bind(now_ms)
            .execute(&self.pool)
            .await?;
        let mut after: Vec<u8> = Vec::new();
        loop {
            let spaces: Vec<Vec<u8>> = sqlx::query_scalar(
                "SELECT space_id FROM relay_spaces WHERE space_id > ? ORDER BY space_id LIMIT ?",
            )
            .bind(after.as_slice())
            .bind(RETENTION_BATCH)
            .fetch_all(&self.pool)
            .await?;
            let Some(last) = spaces.last().cloned() else {
                break;
            };
            for space in spaces {
                if let Some(space) = SpaceId::from_slice(&space) {
                    self.enforce_retention(&space, now_ms).await?;
                }
            }
            after = last;
        }
        Ok(())
    }
}

/// Picks what retention trims next: `Some(true)` for the oldest record, `Some(false)` for the
/// least recently read asset, `None` when nothing is expired and the space is within quota.
/// Expired items go first; over quota the older of the two goes.
fn choose_eviction(
    record_time: Option<i64>,
    asset_time: Option<i64>,
    cutoff: i64,
    over_quota: bool,
) -> Option<bool> {
    let record_expired = record_time.is_some_and(|time| time < cutoff);
    let asset_expired = asset_time.is_some_and(|time| time < cutoff);
    if record_expired || asset_expired || over_quota {
        let candidates = (
            record_time.filter(|_| record_expired || !asset_expired),
            asset_time.filter(|_| asset_expired || !record_expired),
        );
        match candidates {
            (Some(record), Some(asset)) => Some(record <= asset),
            (Some(_), None) => Some(true),
            (None, Some(_)) => Some(false),
            (None, None) => None,
        }
    } else {
        None
    }
}

struct SpaceRow {
    current_epoch: u32,
    admins: Vec<AdminPublicKey>,
    admin_counter: u64,
    stored_bytes: u64,
}

async fn space_row(
    connection: &mut SqliteConnection,
    space: &SpaceId,
) -> StoreResult<Option<SpaceRow>> {
    let row = sqlx::query(
        "SELECT current_epoch, admin_keys, admin_counter, stored_bytes FROM relay_spaces \
         WHERE space_id = ?",
    )
    .bind(space.as_bytes().as_slice())
    .fetch_optional(&mut *connection)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let keys: Vec<u8> = row.try_get("admin_keys")?;
    let stored_admin_key_error =
        || StoreError::Database(sqlx::Error::Protocol("stored admin key".into()));
    let (chunks, remainder) = keys.as_chunks::<PUBLIC_KEY_BYTES>();
    if !remainder.is_empty() {
        return Err(stored_admin_key_error());
    }
    let admins = chunks
        .iter()
        .map(|key| AdminPublicKey::from_slice(key))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| stored_admin_key_error())?;
    Ok(Some(SpaceRow {
        current_epoch: epoch_from_sql(row.try_get("current_epoch")?),
        admins,
        admin_counter: from_sql(row.try_get("admin_counter")?),
        stored_bytes: from_sql(row.try_get("stored_bytes")?),
    }))
}

async fn writer_ranges(
    connection: &mut SqliteConnection,
    space: &SpaceId,
) -> StoreResult<Vec<WriterRange>> {
    let rows = sqlx::query(
        "SELECT writer_id, floor, next_seq FROM relay_floors WHERE space_id = ? \
         ORDER BY writer_id",
    )
    .bind(space.as_bytes().as_slice())
    .fetch_all(&mut *connection)
    .await?;
    rows.into_iter()
        .map(|row| {
            let writer: Vec<u8> = row.try_get("writer_id")?;
            Ok(WriterRange {
                writer: writer_from_sql(&writer)?,
                floor: from_sql(row.try_get("floor")?),
                next: from_sql(row.try_get("next_seq")?),
            })
        })
        .collect()
}

fn join_keys(keys: &[AdminPublicKey]) -> Vec<u8> {
    keys.iter().flat_map(|key| *key.as_bytes()).collect()
}

async fn set_admins(
    connection: &mut SqliteConnection,
    space: &SpaceId,
    admins: &[AdminPublicKey],
    counter: i64,
) -> StoreResult<()> {
    sqlx::query("UPDATE relay_spaces SET admin_keys = ?, admin_counter = ? WHERE space_id = ?")
        .bind(join_keys(admins))
        .bind(counter)
        .bind(space.as_bytes().as_slice())
        .execute(&mut *connection)
        .await?;
    Ok(())
}

async fn add_stored_bytes(
    connection: &mut SqliteConnection,
    space: &SpaceId,
    delta: i64,
) -> StoreResult<()> {
    sqlx::query(
        "UPDATE relay_spaces SET stored_bytes = MAX(0, stored_bytes + ?) WHERE space_id = ?",
    )
    .bind(delta)
    .bind(space.as_bytes().as_slice())
    .execute(&mut *connection)
    .await?;
    Ok(())
}

async fn evict_front_record(
    connection: &mut SqliteConnection,
    space: &SpaceId,
    record: &sqlx::sqlite::SqliteRow,
) -> StoreResult<()> {
    let arrival: i64 = record.try_get("arrival")?;
    let writer: Vec<u8> = record.try_get("writer_id")?;
    let seq: i64 = record.try_get("seq")?;
    let size: i64 = record.try_get("size")?;
    sqlx::query("DELETE FROM relay_records WHERE arrival = ?")
        .bind(arrival)
        .execute(&mut *connection)
        .await?;
    sqlx::query(
        "UPDATE relay_floors SET floor = MAX(floor, MIN(next_seq, ? + 1)) \
         WHERE space_id = ? AND writer_id = ?",
    )
    .bind(seq)
    .bind(space.as_bytes().as_slice())
    .bind(writer.as_slice())
    .execute(&mut *connection)
    .await?;
    add_stored_bytes(connection, space, -size).await
}
