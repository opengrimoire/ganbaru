//! In-memory review snapshot registry, identity, and immutable selection reads.

use super::super::models::{
    ChatError, ChatErrorCode, ChatResult, ChatThreadId, ProjectWorkingFolderId,
};
use super::super::workspace::WorkingFolderAuthorizationOperation;
use super::ReviewWorkspaceAuthorizer;
use super::authorization::authorize_snapshot;
use super::contracts::*;
use super::patch_store::{PatchIndex, ReviewObjectStore, read_patch_page};
use super::selection::{select_provider_patch_lines, select_text_range};
use super::{corrupt_data_error, registry_error, review_output};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const MAX_SNAPSHOTS: usize = 8;
const MAX_COMPLETED_OPERATIONS: usize = 128;
const SNAPSHOT_LIFETIME: Duration = Duration::from_secs(30 * 60);
pub const DEFAULT_PATCH_PAGE_BYTES: usize = 1024 * 1024;

#[derive(Default)]
pub struct ChatReviewRegistry {
    snapshots: Mutex<SnapshotStore>,
}

#[derive(Default)]
struct SnapshotStore {
    values: HashMap<String, Arc<ReviewSnapshot>>,
    order: VecDeque<String>,
    completed: HashMap<String, CompletedReviewOperation>,
    completed_order: VecDeque<String>,
}

#[derive(Clone)]
pub struct CompletedReviewOperation {
    pub database_identity: String,
    pub thread_id: Option<ChatThreadId>,
    pub working_folder_id: ProjectWorkingFolderId,
    pub environment_id: String,
    pub request_fingerprint: String,
    pub result: ChatReviewActionResultRead,
}

pub struct ReviewSnapshot {
    pub database_identity: String,
    pub snapshot_id: String,
    pub review_revision: String,
    pub thread_id: Option<ChatThreadId>,
    pub working_folder_id: ProjectWorkingFolderId,
    pub environment_id: String,
    pub root: PathBuf,
    pub source: ReviewDiffSource,
    pub source_label: String,
    pub before_oid: Option<String>,
    pub after_oid: Option<String>,
    pub context_lines: u32,
    pub ignore_whitespace: bool,
    pub files: Vec<ReviewFileInternal>,
    pub provider_patches: HashMap<String, String>,
    pub patch_cache: tokio::sync::Mutex<HashMap<String, Arc<PatchIndex>>>,
    pub object_store: Option<Arc<ReviewObjectStore>>,
    pub created_at: Instant,
}

#[derive(Clone)]
pub struct ReviewFileInternal {
    pub read: ReviewFileRead,
}

pub struct ResolvedReviewSelection {
    pub relative_path: String,
    pub previous_relative_path: Option<String>,
    pub content_revision: String,
    pub selected_text: String,
    pub source: ReviewDiffSource,
}

pub struct ResolveReviewSelectionRequest<'a> {
    pub thread_id: &'a ChatThreadId,
    pub snapshot_id: &'a str,
    pub review_revision: &'a str,
    pub file_id: &'a str,
    pub side: &'a str,
    pub start_line: u64,
    pub start_column: u64,
    pub end_line: u64,
    pub end_column: u64,
}

pub async fn snapshot_read(
    snapshot: &Arc<ReviewSnapshot>,
    preferred_path: Option<&str>,
) -> ChatResult<ChatReviewOpenRead> {
    let preferred = preferred_path
        .and_then(|path| {
            snapshot
                .files
                .iter()
                .find(|file| file.read.relative_path == path)
        })
        .or_else(|| snapshot.files.first());
    let preferred_patch = match preferred {
        Some(file) => {
            Some(read_patch_page(snapshot, &file.read.file_id, 0, DEFAULT_PATCH_PAGE_BYTES).await?)
        }
        None => None,
    };
    Ok(ChatReviewOpenRead {
        snapshot_id: snapshot.snapshot_id.clone(),
        review_revision: snapshot.review_revision.clone(),
        source: snapshot.source.clone(),
        source_label: snapshot.source_label.clone(),
        files: snapshot
            .files
            .iter()
            .map(|file| file.read.clone())
            .collect(),
        totals: ReviewTotalsRead {
            files: u64::try_from(snapshot.files.len()).unwrap_or(u64::MAX),
            additions: snapshot
                .files
                .iter()
                .filter_map(|file| file.read.additions)
                .sum(),
            deletions: snapshot
                .files
                .iter()
                .filter_map(|file| file.read.deletions)
                .sum(),
        },
        preferred_patch,
        freshness: ReviewFreshness::Current,
    })
}

pub fn stale_snapshot_read(snapshot: &ReviewSnapshot) -> ChatReviewOpenRead {
    ChatReviewOpenRead {
        snapshot_id: snapshot.snapshot_id.clone(),
        review_revision: snapshot.review_revision.clone(),
        source: snapshot.source.clone(),
        source_label: snapshot.source_label.clone(),
        files: snapshot
            .files
            .iter()
            .map(|file| file.read.clone())
            .collect(),
        totals: ReviewTotalsRead {
            files: u64::try_from(snapshot.files.len()).unwrap_or(u64::MAX),
            additions: snapshot
                .files
                .iter()
                .filter_map(|file| file.read.additions)
                .sum(),
            deletions: snapshot
                .files
                .iter()
                .filter_map(|file| file.read.deletions)
                .sum(),
        },
        preferred_patch: None,
        freshness: ReviewFreshness::Outdated,
    }
}

impl ChatReviewRegistry {
    pub async fn resolve_selection(
        &self,
        authorizer: &dyn ReviewWorkspaceAuthorizer,
        pool: &SqlitePool,
        database_identity: &str,
        request: ResolveReviewSelectionRequest<'_>,
    ) -> ChatResult<ResolvedReviewSelection> {
        if !matches!(request.side, "old" | "new") {
            return Err(ChatError::validation(
                "selectionSide",
                "Snapshot review comments require an old or new side",
            ));
        }
        let thread_scope = Some(request.thread_id.clone());
        let snapshot =
            self.snapshot(request.snapshot_id, Some(&thread_scope), database_identity)?;
        super::require_snapshot_revision(&snapshot, request.review_revision)?;
        authorize_snapshot(
            authorizer,
            pool,
            &snapshot,
            WorkingFolderAuthorizationOperation::FileRead,
        )
        .await?;
        let file = snapshot
            .files
            .iter()
            .find(|file| file.read.file_id == request.file_id)
            .ok_or_else(|| {
                ChatError::new(ChatErrorCode::NotFound, "Review file was not found", true)
            })?;
        if !file.read.capabilities.comment {
            return Err(ChatError::unsupported(
                file.read
                    .capability_reasons
                    .comment
                    .as_deref()
                    .unwrap_or("This file does not support line comments"),
            ));
        }
        let selected_text = if let Some(patch) = snapshot.provider_patches.get(request.file_id) {
            select_provider_patch_lines(
                patch,
                request.side,
                request.start_line,
                request.start_column,
                request.end_line,
                request.end_column,
            )?
        } else {
            let (oid, path) = if request.side == "old" {
                (
                    snapshot.before_oid.as_deref(),
                    file.read
                        .previous_relative_path
                        .as_deref()
                        .unwrap_or(&file.read.relative_path),
                )
            } else {
                (
                    snapshot.after_oid.as_deref(),
                    file.read.relative_path.as_str(),
                )
            };
            let oid = oid.ok_or_else(|| {
                ChatError::new(
                    ChatErrorCode::NotFound,
                    "The selected review side is unavailable",
                    true,
                )
            })?;
            let object = format!("{oid}:{path}");
            let contents = String::from_utf8(
                review_output(
                    &snapshot.root,
                    &["cat-file", "blob", &object],
                    None,
                    snapshot.object_store.as_deref(),
                )
                .await?,
            )
            .map_err(|_| ChatError::unsupported("The selected file is not UTF-8 text"))?;
            select_text_range(
                &contents,
                request.start_line,
                request.start_column,
                request.end_line,
                request.end_column,
            )?
        };
        Ok(ResolvedReviewSelection {
            relative_path: file.read.relative_path.clone(),
            previous_relative_path: file.read.previous_relative_path.clone(),
            content_revision: snapshot.review_revision.clone(),
            selected_text,
            source: snapshot.source.clone(),
        })
    }

    pub fn insert(&self, snapshot: Arc<ReviewSnapshot>) -> ChatResult<()> {
        let mut store = self.snapshots.lock().map_err(|_| registry_error())?;
        purge_snapshots(&mut store);
        store.order.push_back(snapshot.snapshot_id.clone());
        store.values.insert(snapshot.snapshot_id.clone(), snapshot);
        while store.order.len() > MAX_SNAPSHOTS {
            if let Some(id) = store.order.pop_front() {
                store.values.remove(&id);
            }
        }
        Ok(())
    }

    pub fn snapshot(
        &self,
        id: &str,
        thread_id: Option<&Option<ChatThreadId>>,
        database_identity: &str,
    ) -> ChatResult<Arc<ReviewSnapshot>> {
        let mut store = self.snapshots.lock().map_err(|_| registry_error())?;
        purge_snapshots(&mut store);
        let snapshot = store.values.get(id).cloned().ok_or_else(|| {
            ChatError::new(
                ChatErrorCode::NotFound,
                "Review snapshot expired. Open Review again.",
                true,
            )
        })?;
        if thread_id.is_some_and(|thread_id| &snapshot.thread_id != thread_id) {
            return Err(ChatError::new(
                ChatErrorCode::Permission,
                "Review snapshot belongs to another Chat thread",
                false,
            ));
        }
        if snapshot.database_identity != database_identity {
            return Err(ChatError::new(
                ChatErrorCode::Permission,
                "Review snapshot belongs to another Ganbaru database",
                false,
            ));
        }
        Ok(snapshot)
    }

    pub fn completed(
        &self,
        operation_id: &str,
        thread_id: &Option<ChatThreadId>,
        working_folder_id: &ProjectWorkingFolderId,
        environment_id: &str,
        request_fingerprint: &str,
        database_identity: &str,
    ) -> ChatResult<Option<ChatReviewActionResultRead>> {
        let store = self.snapshots.lock().map_err(|_| registry_error())?;
        let storage_key = completed_operation_key(database_identity, operation_id);
        match store.completed.get(&storage_key) {
            Some(operation)
                if &operation.thread_id == thread_id
                    && operation.database_identity == database_identity
                    && &operation.working_folder_id == working_folder_id
                    && operation.environment_id == environment_id
                    && operation.request_fingerprint == request_fingerprint =>
            {
                Ok(Some(operation.result.clone()))
            }
            Some(_) => Err(ChatError::new(
                ChatErrorCode::Conflict,
                "Review operation ID was already used for another review action",
                false,
            )),
            None => Ok(None),
        }
    }

    pub fn complete(&self, id: String, operation: CompletedReviewOperation) -> ChatResult<()> {
        let mut store = self.snapshots.lock().map_err(|_| registry_error())?;
        let storage_key = completed_operation_key(&operation.database_identity, &id);
        store.completed_order.push_back(storage_key.clone());
        store.completed.insert(storage_key, operation);
        while store.completed_order.len() > MAX_COMPLETED_OPERATIONS {
            if let Some(id) = store.completed_order.pop_front() {
                store.completed.remove(&id);
            }
        }
        Ok(())
    }
}

fn purge_snapshots(store: &mut SnapshotStore) {
    while let Some(id) = store.order.front() {
        let expired = store
            .values
            .get(id)
            .is_none_or(|snapshot| snapshot.created_at.elapsed() > SNAPSHOT_LIFETIME);
        if !expired {
            break;
        }
        if let Some(id) = store.order.pop_front() {
            store.values.remove(&id);
        }
    }
}

pub fn review_revision(
    thread_id: Option<&ChatThreadId>,
    environment_id: &str,
    source: &ReviewDiffSource,
    before: &str,
    after: &str,
    _ignore_whitespace: bool,
    _context_lines: u32,
) -> ChatResult<String> {
    let mut hasher = Sha256::new();
    hasher.update(
        thread_id
            .map(ChatThreadId::as_str)
            .unwrap_or("workspace-draft"),
    );
    hasher.update([0]);
    hasher.update(environment_id);
    hasher.update([0]);
    hasher.update(serde_json::to_vec(source).map_err(|_| corrupt_data_error())?);
    hasher.update([0]);
    hasher.update(before);
    hasher.update([0]);
    hasher.update(after);
    Ok(format!("{:x}", hasher.finalize()))
}

pub fn snapshot_id(
    database_identity: &str,
    thread_id: Option<&ChatThreadId>,
    environment_id: &str,
    revision: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(database_identity);
    hasher.update([0]);
    hasher.update(
        thread_id
            .map(ChatThreadId::as_str)
            .unwrap_or("workspace-draft"),
    );
    hasher.update([0]);
    hasher.update(environment_id);
    hasher.update([0]);
    hasher.update(revision);
    hasher.update([0]);
    hasher.update(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
            .to_le_bytes(),
    );
    format!("review:{:x}", hasher.finalize())
}

pub fn completed_operation_key(database_identity: &str, operation_id: &str) -> String {
    format!(
        "{:x}",
        Sha256::digest([database_identity.as_bytes(), &[0], operation_id.as_bytes()].concat())
    )
}

pub fn file_id(revision: &str, path: &str, previous: Option<&str>) -> String {
    let mut hasher = Sha256::new();
    hasher.update(revision);
    hasher.update([0]);
    hasher.update(path);
    hasher.update([0]);
    hasher.update(previous.unwrap_or_default());
    format!("review-file:{:x}", hasher.finalize())
}

pub fn action_request_fingerprint(request: &ApplyChatReviewActionRequest) -> ChatResult<String> {
    serde_json::to_vec(request)
        .map(|value| format!("{:x}", Sha256::digest(value)))
        .map_err(|_| corrupt_data_error())
}
