//! Secret requests: a running teammate asks for a value it needs, a person provides it on the
//! device that executes the run, and the host writes the value to a file only this user can
//! read, under the app config directory and outside every vault and working folder.
//!
//! The value never reaches SQLite, sync, events, diagnostics, or the provider transcript.
//! Request rows hold only the label, purpose, and state, and they are device-local like the
//! runs they belong to. Every provider event of the run's thread, and every host tool argument,
//! is redacted against the values provided to that thread before Chat stores it.
//!
//! The files are removed once their run is no longer active, or after
//! [`SECRET_FILE_MAX_AGE_HOURS`] in any case. One lock serializes providing a value and the
//! sweep that removes files, so a sweep never races a write into the same directory.

use crate::chat::command_support::chat_pool;
use crate::chat::internal_mcp::secure_random_hex;
use chrono::{DateTime, SecondsFormat, Utc};
use ganbaru_atomic_files::{FileAccess, remove_file_within, write_file_atomically};
use ganbaru_chat::agent_runs::{SETTLED_RUN_STATES, unsettled_run_state_sql};
use ganbaru_chat::credentials::SecretValue;
use ganbaru_chat::ingestion::{RegisteredSecret, SecretRedactor, longest_first, redact_json};
use ganbaru_chat_contracts::models::{
    ChatError, ChatErrorCode, ChatReplyThreadId, ChatResult, ChatThreadId,
};
use serde::Serialize;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};
use tauri::Manager;

/// Minutes a request stays open for a person to answer.
pub(crate) const SECRET_REQUEST_TTL_MINUTES: i64 = 30;
/// Largest value a person may provide.
pub(crate) const MAX_SECRET_VALUE_BYTES: usize = 16 * 1024;
/// Smallest value a person may provide. Redaction replaces every exact occurrence of the
/// value, so a very short value would also replace ordinary text.
pub(crate) const MIN_SECRET_VALUE_BYTES: usize = 6;
/// Requests one run may have open at once.
pub(crate) const MAX_OPEN_SECRET_REQUESTS_PER_RUN: i64 = 3;
/// Hours a provided file and its registered value live, even while the run keeps working.
pub(crate) const SECRET_FILE_MAX_AGE_HOURS: u64 = 24;
/// Longest request label, in characters.
pub(crate) const MAX_SECRET_LABEL_CHARS: usize = 120;
/// Longest request purpose, in characters.
pub(crate) const MAX_SECRET_PURPOSE_CHARS: usize = 500;
/// Folder under the app config directory that holds one private folder per run.
const SECRET_DIRECTORY: &str = "chat-secrets";
/// Random bytes in a request id.
const REQUEST_ID_BYTES: usize = 16;

const SECRET_FILE_MAX_AGE: Duration = Duration::from_secs(SECRET_FILE_MAX_AGE_HOURS * 60 * 60);

/// The values provided on this device, by provider thread, and the lock that serializes
/// writing and removing secret files. Values stay registered for
/// [`SECRET_FILE_MAX_AGE_HOURS`] after they are provided, not only while their run is active,
/// because a continued or resumed session of the same thread still holds them in context.
#[derive(Clone, Default)]
pub struct ChatSecretRegistry {
    inner: Arc<RegistryInner>,
}

#[derive(Default)]
struct RegistryInner {
    values: std::sync::Mutex<HashMap<String, Vec<RegisteredValue>>>,
    files: tokio::sync::Mutex<()>,
}

struct RegisteredValue {
    request_id: String,
    secret: Arc<RegisteredSecret>,
    registered_at: Instant,
}

impl SecretRedactor for ChatSecretRegistry {
    fn registered(&self, thread_id: &ChatThreadId) -> Vec<Arc<RegisteredSecret>> {
        let mut values = self.lock_values();
        prune_values(&mut values);
        values
            .get(thread_id.as_str())
            .map(|registered| {
                registered
                    .iter()
                    .map(|value| Arc::clone(&value.secret))
                    .collect()
            })
            .unwrap_or_default()
    }
}

impl ChatSecretRegistry {
    /// Registers the value of `request_id` for `thread_id`, as if it was provided `age` ago.
    /// A request registered already keeps its first entry.
    fn register(&self, thread_id: &str, request_id: &str, secret: RegisteredSecret, age: Duration) {
        let mut values = self.lock_values();
        prune_values(&mut values);
        let registered = values.entry(thread_id.to_owned()).or_default();
        if registered
            .iter()
            .any(|value| value.request_id == request_id)
        {
            return;
        }
        registered.push(RegisteredValue {
            request_id: request_id.to_owned(),
            secret: Arc::new(secret),
            registered_at: Instant::now().checked_sub(age).unwrap_or_else(Instant::now),
        });
    }

    /// `arguments` with every registered value of `thread_id` replaced by its marker, so a
    /// host tool never writes a provided value into a replicated row or a workspace file.
    pub(crate) fn redact_arguments(
        &self,
        thread_id: &ChatThreadId,
        arguments: &Map<String, Value>,
    ) -> Option<Map<String, Value>> {
        let secrets = longest_first(self.registered(thread_id));
        if secrets.is_empty() {
            return None;
        }
        let mut value = Value::Object(arguments.clone());
        if !redact_json(&mut value, &secrets) {
            return None;
        }
        match value {
            Value::Object(redacted) => Some(redacted),
            _ => None,
        }
    }

    fn lock_values(&self) -> std::sync::MutexGuard<'_, HashMap<String, Vec<RegisteredValue>>> {
        // A panic cannot leave a half-registered value: entries are pushed or dropped whole.
        self.inner
            .values
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

fn prune_values(values: &mut HashMap<String, Vec<RegisteredValue>>) {
    values.retain(|_, registered| {
        registered.retain(|value| value.registered_at.elapsed() < SECRET_FILE_MAX_AGE);
        !registered.is_empty()
    });
}

/// A request as the person answering it sees it. It never carries the value.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatSecretRequestRead {
    pub id: String,
    pub assignment_id: String,
    pub label: String,
    pub purpose: String,
    pub created_at: String,
    pub expires_at: String,
}

/// Why providing or declining a value was refused. The code is the error message.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SecretRefusal {
    /// The request was already answered or expired.
    NotPending,
    /// The request expired, or its run ended, before the answer.
    Expired,
    /// The value is shorter than [`MIN_SECRET_VALUE_BYTES`].
    TooShort,
    /// The value is longer than [`MAX_SECRET_VALUE_BYTES`].
    TooLarge,
    /// The value occurs in the request's label or purpose, which the teammate wrote.
    MatchesRequest,
}

impl SecretRefusal {
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::NotPending => "secret_request_not_pending",
            Self::Expired => "secret_request_expired",
            Self::TooShort => "secret_value_too_short",
            Self::TooLarge => "secret_value_too_large",
            Self::MatchesRequest => "secret_value_matches_request",
        }
    }

    fn into_error(self) -> ChatError {
        match self {
            Self::NotPending | Self::Expired => {
                ChatError::new(ChatErrorCode::Conflict, self.code(), false)
            }
            Self::TooShort | Self::TooLarge | Self::MatchesRequest => {
                ChatError::validation("value", self.code())
            }
        }
    }
}

/// The folder that holds every run's secret folder.
pub(crate) fn secret_root(app: &tauri::AppHandle) -> ChatResult<PathBuf> {
    app.path()
        .app_config_dir()
        .map(|directory| directory.join(SECRET_DIRECTORY))
        .map_err(|_| {
            ChatError::new(
                ChatErrorCode::Internal,
                "Chat could not find the app config folder",
                false,
            )
        })
}

/// The private folder of `run_id`. Run ids may contain characters that are not valid in a
/// file name on every platform, so the folder is named by the run id's SHA-256 digest.
fn run_directory(root: &Path, run_id: &str) -> PathBuf {
    root.join(run_directory_name(run_id))
}

fn run_directory_name(run_id: &str) -> String {
    format!("{:x}", Sha256::digest(run_id.as_bytes()))
}

/// The file a provided value for `request_id` of `run_id` is written to.
pub(crate) fn secret_file_path(root: &Path, run_id: &str, request_id: &str) -> PathBuf {
    run_directory(root, run_id).join(request_id)
}

/// Whether this host can keep a provided file private to the current user. Unix modes prove
/// it; other platforms inherit the app config folder's access list, which this host cannot
/// verify, so they never offer secret requests.
pub(crate) const fn secret_files_supported() -> bool {
    cfg!(unix)
}

/// Opens a request for `run_id` and returns its id and expiry. Expired requests of the run
/// close first, so only open ones count against [`MAX_OPEN_SECRET_REQUESTS_PER_RUN`].
pub(crate) async fn open_request(
    pool: &SqlitePool,
    run_id: &str,
    assignment_id: &str,
    label: &str,
    purpose: &str,
    now: DateTime<Utc>,
) -> ChatResult<Option<String>> {
    let now_text = timestamp(now);
    let expires_at = timestamp(now + chrono::Duration::minutes(SECRET_REQUEST_TTL_MINUTES));
    let mut transaction = pool
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(persistence_error)?;
    expire_due_requests(&mut transaction, &now_text).await?;
    let active_sql = format!(
        "SELECT count(*) FROM chat_agent_runs
         WHERE id = ? AND assignment_id = ? AND {}",
        unsettled_run_state_sql("state")
    );
    let active: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(active_sql))
        .bind(run_id)
        .bind(assignment_id)
        .fetch_one(&mut *transaction)
        .await
        .map_err(persistence_error)?;
    if active != 1 {
        return Err(ChatError::new(
            ChatErrorCode::Permission,
            "The run is not active",
            false,
        ));
    }
    let open: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM chat_secret_requests WHERE run_id = ? AND state = 'pending'",
    )
    .bind(run_id)
    .fetch_one(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    if open >= MAX_OPEN_SECRET_REQUESTS_PER_RUN {
        return Ok(None);
    }
    let request_id = secure_random_hex(REQUEST_ID_BYTES)?;
    sqlx::query(
        "INSERT INTO chat_secret_requests
            (id, run_id, assignment_id, label, purpose, state, created_at, expires_at)
         VALUES (?, ?, ?, ?, ?, 'pending', ?, ?)",
    )
    .bind(&request_id)
    .bind(run_id)
    .bind(assignment_id)
    .bind(label)
    .bind(purpose)
    .bind(&now_text)
    .bind(&expires_at)
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    transaction.commit().await.map_err(persistence_error)?;
    Ok(Some(request_id))
}

/// The state of `request_id` when it belongs to `run_id`, closing it first when it expired.
pub(crate) async fn request_state(
    pool: &SqlitePool,
    run_id: &str,
    request_id: &str,
    now: DateTime<Utc>,
) -> ChatResult<Option<String>> {
    let mut transaction = pool
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(persistence_error)?;
    expire_due_requests(&mut transaction, &timestamp(now)).await?;
    let state: Option<String> =
        sqlx::query_scalar("SELECT state FROM chat_secret_requests WHERE id = ? AND run_id = ?")
            .bind(request_id)
            .bind(run_id)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(persistence_error)?;
    transaction.commit().await.map_err(persistence_error)?;
    Ok(state)
}

/// Registers `value` for redaction, writes it to the request's private file, and marks the
/// request provided.
///
/// The value is registered before the file exists, so the teammate cannot read it and echo
/// it before its events are redacted. A refused or failed write leaves the value registered;
/// redacting a value that was never delivered costs nothing.
pub(crate) async fn provide_secret(
    registry: &ChatSecretRegistry,
    pool: &SqlitePool,
    root: &Path,
    request_id: &str,
    value: SecretValue,
    now: DateTime<Utc>,
) -> ChatResult<()> {
    if !secret_files_supported() {
        return Err(unsupported_error());
    }
    let length = value.expose().len();
    if length < MIN_SECRET_VALUE_BYTES {
        return Err(SecretRefusal::TooShort.into_error());
    }
    if length > MAX_SECRET_VALUE_BYTES {
        return Err(SecretRefusal::TooLarge.into_error());
    }
    let _files = registry.inner.files.lock().await;
    let now_text = timestamp(now);
    let mut transaction = pool
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(persistence_error)?;
    expire_due_requests(&mut transaction, &now_text).await?;
    let request = answerable_request(&mut transaction, request_id).await?;
    if request.state == "expired" {
        transaction.commit().await.map_err(persistence_error)?;
        return Err(SecretRefusal::Expired.into_error());
    }
    if request.state != "pending" {
        return Err(SecretRefusal::NotPending.into_error());
    }
    if request.label.contains(value.expose()) || request.purpose.contains(value.expose()) {
        return Err(SecretRefusal::MatchesRequest.into_error());
    }
    registry.register(
        &request.provider_thread_id,
        request_id,
        RegisteredSecret::new(&request.label, redaction_copy(&value)?),
        Duration::ZERO,
    );
    let path = secret_file_path(root, &request.run_id, request_id);
    write_private_file(root, &path, value.expose().as_bytes())?;
    let updated = sqlx::query(
        "UPDATE chat_secret_requests SET state = 'provided', resolved_at = ?
         WHERE id = ? AND state = 'pending'",
    )
    .bind(&now_text)
    .bind(request_id)
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error);
    let committed = match updated {
        Ok(_) => transaction.commit().await.map_err(persistence_error),
        Err(error) => Err(error),
    };
    if let Err(error) = committed {
        let _ = remove_file_within(root, &path);
        return Err(error);
    }
    Ok(())
}

/// Marks `request_id` declined, so the teammate learns no value is coming.
pub(crate) async fn decline_secret(
    pool: &SqlitePool,
    request_id: &str,
    now: DateTime<Utc>,
) -> ChatResult<()> {
    let now_text = timestamp(now);
    let mut transaction = pool
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(persistence_error)?;
    expire_due_requests(&mut transaction, &now_text).await?;
    let request = answerable_request(&mut transaction, request_id).await?;
    if request.state == "expired" {
        transaction.commit().await.map_err(persistence_error)?;
        return Err(SecretRefusal::Expired.into_error());
    }
    if request.state != "pending" {
        return Err(SecretRefusal::NotPending.into_error());
    }
    sqlx::query(
        "UPDATE chat_secret_requests SET state = 'declined', resolved_at = ?
         WHERE id = ? AND state = 'pending'",
    )
    .bind(&now_text)
    .bind(request_id)
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    transaction.commit().await.map_err(persistence_error)
}

/// The open requests of the work in `reply_thread_id`, oldest first.
pub(crate) async fn list_open_requests(
    pool: &SqlitePool,
    reply_thread_id: &str,
    now: DateTime<Utc>,
) -> ChatResult<Vec<ChatSecretRequestRead>> {
    let rows: Vec<(String, String, String, String, String, String)> = sqlx::query_as(
        "SELECT request.id, request.assignment_id, request.label, request.purpose,
                request.created_at, request.expires_at
         FROM chat_secret_requests request
         JOIN chat_work_assignments assignment ON assignment.id = request.assignment_id
         WHERE assignment.reply_thread_id = ? AND request.state = 'pending'
           AND request.expires_at > ?
         ORDER BY request.created_at, request.id",
    )
    .bind(reply_thread_id)
    .bind(timestamp(now))
    .fetch_all(pool)
    .await
    .map_err(persistence_error)?;
    Ok(rows
        .into_iter()
        .map(
            |(id, assignment_id, label, purpose, created_at, expires_at)| ChatSecretRequestRead {
                id,
                assignment_id,
                label,
                purpose,
                created_at,
                expires_at,
            },
        )
        .collect())
}

/// Closes open requests that expired or whose run ended, registers again the values of files
/// that are still on disk, removes the secret folders of runs that are no longer active, and
/// removes files older than [`SECRET_FILE_MAX_AGE_HOURS`]. Returns how many files it removed.
///
/// Registering again keeps redaction working after an app restart, when a resumed session
/// still holds a value in context. A value whose file is already gone cannot be recovered.
/// One entry that cannot be read or removed does not stop the rest of the pass; the first
/// such failure is returned at the end. Safe to repeat: every step skips what is gone.
pub(crate) async fn sweep_secret_files(
    registry: &ChatSecretRegistry,
    pool: &SqlitePool,
    root: &Path,
    now: DateTime<Utc>,
) -> ChatResult<u64> {
    let _files = registry.inner.files.lock().await;
    prune_values(&mut registry.lock_values());
    let mut transaction = pool
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(persistence_error)?;
    expire_due_requests(&mut transaction, &timestamp(now)).await?;
    transaction.commit().await.map_err(persistence_error)?;
    restore_registered_values(registry, pool, root).await?;
    let active_sql = format!(
        "SELECT id FROM chat_agent_runs WHERE {}",
        unsettled_run_state_sql("state")
    );
    let active: HashSet<String> = sqlx::query_scalar::<_, String>(sqlx::AssertSqlSafe(active_sql))
        .fetch_all(pool)
        .await
        .map_err(persistence_error)?
        .iter()
        .map(|run_id| run_directory_name(run_id))
        .collect();
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(file_error(error)),
    };
    let mut sweep = SweepOutcome::default();
    for entry in entries {
        let Some(entry) = sweep.keep(entry) else {
            continue;
        };
        let path = entry.path();
        let Some(metadata) = sweep.keep(std::fs::symlink_metadata(&path)) else {
            continue;
        };
        if !metadata.is_dir() {
            sweep.keep(std::fs::remove_file(&path));
            continue;
        }
        let keep_folder = entry
            .file_name()
            .to_str()
            .is_some_and(|name| active.contains(name));
        remove_files_in(root, &path, keep_folder, &mut sweep);
        if !keep_folder {
            match std::fs::remove_dir(&path) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                result => {
                    sweep.keep(result);
                }
            }
        }
    }
    sweep.finish()
}

/// Files removed by one sweep and its first failure.
#[derive(Default)]
struct SweepOutcome {
    removed: u64,
    failure: Option<ChatError>,
}

impl SweepOutcome {
    /// The value of `result`, or `None` after recording its failure.
    fn keep<T>(&mut self, result: std::io::Result<T>) -> Option<T> {
        match result {
            Ok(value) => Some(value),
            Err(error) => {
                self.failure.get_or_insert_with(|| file_error(error));
                None
            }
        }
    }

    fn finish(self) -> ChatResult<u64> {
        match self.failure {
            Some(error) => Err(error),
            None => Ok(self.removed),
        }
    }
}

/// Removes the files in one run folder: all of them, or only those past the age limit when
/// `keep_recent` is set. A nested folder never comes from this host and is removed whole.
fn remove_files_in(root: &Path, directory: &Path, keep_recent: bool, sweep: &mut SweepOutcome) {
    let Some(entries) = sweep.keep(std::fs::read_dir(directory)) else {
        return;
    };
    for entry in entries {
        let Some(entry) = sweep.keep(entry) else {
            continue;
        };
        let path = entry.path();
        let Some(metadata) = sweep.keep(std::fs::symlink_metadata(&path)) else {
            continue;
        };
        if metadata.is_dir() {
            sweep.keep(std::fs::remove_dir_all(&path));
            continue;
        }
        if keep_recent && file_age(&metadata).is_some_and(|age| age < SECRET_FILE_MAX_AGE) {
            continue;
        }
        if sweep.keep(remove_file_within(root, &path)).is_some() {
            sweep.removed += 1;
        }
    }
}

/// Registers the values of provided requests whose files are still recent regular files, so
/// redaction survives an app restart. Files of rows this vault does not hold are skipped.
async fn restore_registered_values(
    registry: &ChatSecretRegistry,
    pool: &SqlitePool,
    root: &Path,
) -> ChatResult<()> {
    let rows: Vec<(String, String, String, String)> = sqlx::query_as(
        "SELECT request.id, request.run_id, run.provider_thread_id, request.label
         FROM chat_secret_requests request
         JOIN chat_agent_runs run ON run.id = request.run_id
         WHERE request.state = 'provided'",
    )
    .fetch_all(pool)
    .await
    .map_err(persistence_error)?;
    for (request_id, run_id, provider_thread_id, label) in rows {
        let path = secret_file_path(root, &run_id, &request_id);
        let Ok(metadata) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        let Some(age) = file_age(&metadata).filter(|age| *age < SECRET_FILE_MAX_AGE) else {
            continue;
        };
        if !metadata.is_file() || metadata.len() > MAX_SECRET_VALUE_BYTES as u64 {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(value) = SecretValue::new(text) else {
            continue;
        };
        registry.register(
            &provider_thread_id,
            &request_id,
            RegisteredSecret::new(&label, value),
            age,
        );
    }
    Ok(())
}

/// How long ago the file was last written, when the platform reports it.
fn file_age(metadata: &std::fs::Metadata) -> Option<Duration> {
    metadata
        .modified()
        .ok()
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
}

/// Runs the sweep for the active vault, reporting a failure without its detail.
pub(crate) async fn sweep(app: &tauri::AppHandle, pool: &SqlitePool) {
    let Some(registry) = app.try_state::<ChatSecretRegistry>() else {
        return;
    };
    let result = match secret_root(app) {
        Ok(root) => sweep_secret_files(&registry, pool, &root, Utc::now()).await,
        Err(error) => Err(error),
    };
    if let Err(error) = result {
        eprintln!("Chat secret cleanup failed with code {:?}", error.code);
    }
}

/// Starts [`sweep`] without waiting for it.
pub(crate) fn spawn_sweep(app: &tauri::AppHandle, pool: &SqlitePool) {
    let app = app.clone();
    let pool = pool.clone();
    tauri::async_runtime::spawn(async move { sweep(&app, &pool).await });
}

struct AnswerableRequest {
    run_id: String,
    provider_thread_id: String,
    label: String,
    purpose: String,
    state: String,
}

async fn answerable_request(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    request_id: &str,
) -> ChatResult<AnswerableRequest> {
    let row: Option<(String, String, String, String, String)> = sqlx::query_as(
        "SELECT request.run_id, run.provider_thread_id, request.label, request.purpose,
                request.state
         FROM chat_secret_requests request
         JOIN chat_agent_runs run ON run.id = request.run_id
         WHERE request.id = ?",
    )
    .bind(request_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    let (run_id, provider_thread_id, label, purpose, state) = row.ok_or_else(|| {
        ChatError::new(
            ChatErrorCode::NotFound,
            "The secret request was not found",
            false,
        )
    })?;
    Ok(AnswerableRequest {
        run_id,
        provider_thread_id,
        label,
        purpose,
        state,
    })
}

/// Closes pending requests that are past their expiry or whose run settled.
async fn expire_due_requests(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    now: &str,
) -> ChatResult<()> {
    let settled = SETTLED_RUN_STATES
        .iter()
        .map(|state| format!("'{state}'"))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "UPDATE chat_secret_requests SET state = 'expired', resolved_at = ?1
         WHERE state = 'pending'
           AND (expires_at <= ?1
                OR run_id IN (SELECT id FROM chat_agent_runs WHERE state IN ({settled})))"
    );
    sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(now)
        .execute(&mut **transaction)
        .await
        .map_err(persistence_error)?;
    Ok(())
}

/// Writes `bytes` to `path` as a file only the current user can read, inside run folders
/// only the current user can open.
fn write_private_file(root: &Path, path: &Path, bytes: &[u8]) -> ChatResult<()> {
    let directory = path.parent().ok_or_else(file_error_message)?;
    ganbaru_atomic_files::create_private_directories(directory).map_err(file_error)?;
    require_private_directory(root)?;
    require_private_directory(directory)?;
    write_file_atomically(path, bytes, FileAccess::OwnerOnly).map_err(file_error)
}

/// Refuses a folder that is a link, or that other users can enter.
fn require_private_directory(directory: &Path) -> ChatResult<()> {
    let metadata = std::fs::symlink_metadata(directory).map_err(file_error)?;
    if !metadata.is_dir() {
        return Err(file_error_message());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(file_error_message());
        }
    }
    Ok(())
}

/// A second owned copy of `value` for the redaction registry, which outlives the request.
fn redaction_copy(value: &SecretValue) -> ChatResult<SecretValue> {
    SecretValue::new(value.expose()).map_err(|_| SecretRefusal::TooShort.into_error())
}

fn timestamp(time: DateTime<Utc>) -> String {
    time.to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn unsupported_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::CapabilityUnsupported,
        "Secret requests need a platform where Chat can keep a file private to you",
        false,
    )
}

fn file_error(_error: std::io::Error) -> ChatError {
    file_error_message()
}

fn file_error_message() -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat could not prepare the private secret file",
        true,
    )
}

fn persistence_error(_error: sqlx::Error) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat secret request could not be saved",
        true,
    )
}

/// Provides a value for an open secret request. The value goes to a private file the teammate
/// reads; it is never stored in the vault.
#[tauri::command]
pub async fn chat_provide_secret(
    app: tauri::AppHandle,
    db_url: String,
    request_id: String,
    value: String,
) -> ChatResult<()> {
    let value = SecretValue::new(value).map_err(|_| SecretRefusal::TooShort.into_error())?;
    let pool = chat_pool(app.clone(), db_url).await?;
    let registry = app.state::<ChatSecretRegistry>();
    let root = secret_root(&app)?;
    provide_secret(&registry, &pool, &root, &request_id, value, Utc::now()).await
}

/// Declines an open secret request.
#[tauri::command]
pub async fn chat_decline_secret(
    app: tauri::AppHandle,
    db_url: String,
    request_id: String,
) -> ChatResult<()> {
    let pool = chat_pool(app, db_url).await?;
    decline_secret(&pool, &request_id, Utc::now()).await
}

/// Lists the open secret requests of the work in one reply thread.
#[tauri::command]
pub async fn chat_list_secret_requests(
    app: tauri::AppHandle,
    db_url: String,
    reply_thread_id: ChatReplyThreadId,
) -> ChatResult<Vec<ChatSecretRequestRead>> {
    let pool = chat_pool(app, db_url).await?;
    list_open_requests(&pool, reply_thread_id.as_str(), Utc::now()).await
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::chat::revocation::tests::pool_with_folder_runs;
    use crate::chat::tests::repository::stored_text;
    use std::os::unix::fs::PermissionsExt;

    const VALUE: &str = "tok-9f2c71e4";

    fn test_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir()
            .join(format!(
                "ganbaru-chat-secrets-{name}-{}",
                std::process::id()
            ))
            .join(SECRET_DIRECTORY);
        let _ = std::fs::remove_dir_all(root.parent().unwrap());
        std::fs::create_dir_all(root.parent().unwrap()).unwrap();
        root
    }

    fn value(text: &str) -> SecretValue {
        SecretValue::new(text).unwrap()
    }

    async fn open(pool: &SqlitePool, now: DateTime<Utc>) -> String {
        open_request(
            pool,
            "run:working",
            "assignment:folders",
            "Deploy token",
            "Publish the preview build",
            now,
        )
        .await
        .unwrap()
        .unwrap()
    }

    async fn state(pool: &SqlitePool, request_id: &str) -> String {
        sqlx::query_scalar("SELECT state FROM chat_secret_requests WHERE id = ?")
            .bind(request_id)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    fn mode(path: &Path) -> u32 {
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[tokio::test]
    async fn a_provided_secret_lands_in_a_private_file_and_nowhere_in_the_database() {
        let pool = pool_with_folder_runs().await;
        let registry = ChatSecretRegistry::default();
        let root = test_root("provided");
        let now = Utc::now();
        let request_id = open(&pool, now).await;
        provide_secret(&registry, &pool, &root, &request_id, value(VALUE), now)
            .await
            .unwrap();

        let path = secret_file_path(&root, "run:working", &request_id);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), VALUE);
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(path.parent().unwrap()), 0o700);
        assert_eq!(mode(&root), 0o700);
        assert_eq!(state(&pool, &request_id).await, "provided");
        assert!(!stored_text(&pool).await.contains(VALUE));

        let thread = ChatThreadId::new("thread-1").unwrap();
        let mut arguments = Map::new();
        arguments.insert("content".into(), Value::String(format!("key={VALUE}")));
        let redacted = registry.redact_arguments(&thread, &arguments).unwrap();
        assert_eq!(redacted["content"], "key=[secret: Deploy token]");
        let other = ChatThreadId::new("thread-2").unwrap();
        assert!(registry.redact_arguments(&other, &arguments).is_none());
    }

    #[tokio::test]
    async fn a_secret_request_is_answered_only_once() {
        let pool = pool_with_folder_runs().await;
        let registry = ChatSecretRegistry::default();
        let root = test_root("once");
        let now = Utc::now();
        let request_id = open(&pool, now).await;
        provide_secret(&registry, &pool, &root, &request_id, value(VALUE), now)
            .await
            .unwrap();
        let again = provide_secret(
            &registry,
            &pool,
            &root,
            &request_id,
            value("other-value"),
            now,
        )
        .await
        .unwrap_err();
        assert_eq!(again.code, ChatErrorCode::Conflict);
        assert_eq!(again.message, "secret_request_not_pending");
        let declined = decline_secret(&pool, &request_id, now).await.unwrap_err();
        assert_eq!(declined.message, "secret_request_not_pending");
        let path = secret_file_path(&root, "run:working", &request_id);
        assert_eq!(std::fs::read_to_string(path).unwrap(), VALUE);

        let declined_id = open(&pool, now).await;
        decline_secret(&pool, &declined_id, now).await.unwrap();
        assert_eq!(state(&pool, &declined_id).await, "declined");
        let late = provide_secret(&registry, &pool, &root, &declined_id, value(VALUE), now)
            .await
            .unwrap_err();
        assert_eq!(late.message, "secret_request_not_pending");
    }

    #[tokio::test]
    async fn an_expired_secret_request_refuses_a_value() {
        let pool = pool_with_folder_runs().await;
        let registry = ChatSecretRegistry::default();
        let root = test_root("expired");
        let opened = Utc::now();
        let request_id = open(&pool, opened).await;
        let later = opened + chrono::Duration::minutes(SECRET_REQUEST_TTL_MINUTES);
        assert!(
            list_open_requests(&pool, "reply:folders", later)
                .await
                .unwrap()
                .is_empty()
        );
        let error = provide_secret(&registry, &pool, &root, &request_id, value(VALUE), later)
            .await
            .unwrap_err();
        assert_eq!(error.code, ChatErrorCode::Conflict);
        assert_eq!(error.message, "secret_request_expired");
        assert_eq!(state(&pool, &request_id).await, "expired");
        assert!(!secret_file_path(&root, "run:working", &request_id).exists());
        assert_eq!(
            request_state(&pool, "run:working", &request_id, later)
                .await
                .unwrap()
                .as_deref(),
            Some("expired")
        );
    }

    #[tokio::test]
    async fn a_secret_value_outside_the_length_bounds_or_in_the_request_is_refused() {
        let pool = pool_with_folder_runs().await;
        let registry = ChatSecretRegistry::default();
        let root = test_root("bounds");
        let now = Utc::now();
        let request_id = open(&pool, now).await;
        let short = "a".repeat(MIN_SECRET_VALUE_BYTES - 1);
        let large = "a".repeat(MAX_SECRET_VALUE_BYTES + 1);
        for (text, code) in [
            (short.as_str(), "secret_value_too_short"),
            (large.as_str(), "secret_value_too_large"),
            ("Deploy token", "secret_value_matches_request"),
            ("preview build", "secret_value_matches_request"),
        ] {
            let error = provide_secret(&registry, &pool, &root, &request_id, value(text), now)
                .await
                .unwrap_err();
            assert_eq!(error.code, ChatErrorCode::Validation, "{code}");
            assert_eq!(error.message, code);
            assert_eq!(error.field.as_deref(), Some("value"), "{code}");
        }
        assert_eq!(state(&pool, &request_id).await, "pending");
        let thread = ChatThreadId::new("thread-1").unwrap();
        assert!(registry.registered(&thread).is_empty());
        let exact = "a".repeat(MAX_SECRET_VALUE_BYTES);
        provide_secret(&registry, &pool, &root, &request_id, value(&exact), now)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn a_run_keeps_at_most_three_open_secret_requests() {
        let pool = pool_with_folder_runs().await;
        let now = Utc::now();
        for _ in 0..MAX_OPEN_SECRET_REQUESTS_PER_RUN {
            open(&pool, now).await;
        }
        let refused = open_request(&pool, "run:working", "assignment:folders", "A", "B", now)
            .await
            .unwrap();
        assert_eq!(refused, None);
        let listed = list_open_requests(&pool, "reply:folders", now)
            .await
            .unwrap();
        assert_eq!(listed.len(), 3);
        let later = now + chrono::Duration::minutes(SECRET_REQUEST_TTL_MINUTES);
        assert!(
            open_request(&pool, "run:working", "assignment:folders", "A", "B", later)
                .await
                .unwrap()
                .is_some()
        );
        let inactive = open_request(&pool, "run:working", "assignment:other", "A", "B", now)
            .await
            .unwrap_err();
        assert_eq!(inactive.code, ChatErrorCode::Permission);
    }

    #[tokio::test]
    async fn the_secret_sweep_removes_files_of_ended_runs_and_closes_their_requests() {
        let pool = pool_with_folder_runs().await;
        let registry = ChatSecretRegistry::default();
        let root = test_root("sweep");
        let now = Utc::now();
        let provided = open(&pool, now).await;
        let pending = open(&pool, now).await;
        provide_secret(&registry, &pool, &root, &provided, value(VALUE), now)
            .await
            .unwrap();
        let stray = run_directory(&root, "run:gone");
        std::fs::create_dir_all(&stray).unwrap();
        std::fs::write(stray.join("left"), b"x").unwrap();

        assert_eq!(
            sweep_secret_files(&registry, &pool, &root, now)
                .await
                .unwrap(),
            1
        );
        let path = secret_file_path(&root, "run:working", &provided);
        assert!(path.exists());
        assert!(!stray.exists());

        sqlx::query("UPDATE chat_agent_runs SET state = 'completed' WHERE id = 'run:working'")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(
            sweep_secret_files(&registry, &pool, &root, now)
                .await
                .unwrap(),
            1
        );
        assert!(!path.exists());
        assert!(!path.parent().unwrap().exists());
        assert_eq!(state(&pool, &pending).await, "expired");
        assert_eq!(state(&pool, &provided).await, "provided");
        assert_eq!(
            sweep_secret_files(&registry, &pool, &root, now)
                .await
                .unwrap(),
            0
        );
        let thread = ChatThreadId::new("thread-1").unwrap();
        assert_eq!(registry.registered(&thread).len(), 1);
    }

    #[tokio::test]
    async fn a_restarted_app_registers_provided_secrets_again_from_their_files() {
        let pool = pool_with_folder_runs().await;
        let root = test_root("restart");
        let now = Utc::now();
        let request_id = open(&pool, now).await;
        provide_secret(
            &ChatSecretRegistry::default(),
            &pool,
            &root,
            &request_id,
            value(VALUE),
            now,
        )
        .await
        .unwrap();
        let restarted = ChatSecretRegistry::default();
        let thread = ChatThreadId::new("thread-1").unwrap();
        assert!(restarted.registered(&thread).is_empty());
        sweep_secret_files(&restarted, &pool, &root, now)
            .await
            .unwrap();
        sweep_secret_files(&restarted, &pool, &root, now)
            .await
            .unwrap();
        let registered = restarted.registered(&thread);
        assert_eq!(registered.len(), 1);
        assert_eq!(registered[0].marker(), "[secret: Deploy token]");
    }

    #[tokio::test]
    async fn a_secret_folder_other_users_can_enter_is_refused() {
        let pool = pool_with_folder_runs().await;
        let registry = ChatSecretRegistry::default();
        let root = test_root("shared");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755)).unwrap();
        let now = Utc::now();
        let request_id = open(&pool, now).await;
        let error = provide_secret(&registry, &pool, &root, &request_id, value(VALUE), now)
            .await
            .unwrap_err();
        assert_eq!(error.code, ChatErrorCode::Persistence);
        assert_eq!(state(&pool, &request_id).await, "pending");
        assert!(!secret_file_path(&root, "run:working", &request_id).exists());
    }
}
