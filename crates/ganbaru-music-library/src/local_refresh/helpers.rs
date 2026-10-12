use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use super::super::{MusicLibraryError, MusicLibraryResult, MusicLocalRefreshRequest};
use super::traversal::LocalMediaEvidence;

pub(super) fn validate_request(request: &MusicLocalRefreshRequest) -> MusicLibraryResult<()> {
    for (field, value) in [
        ("jobId", request.job_id.as_str()),
        ("rootId", request.root_id.as_str()),
        ("collectionId", request.collection_id.as_str()),
        ("folderPath", request.folder_path.as_str()),
    ] {
        if value.trim().is_empty() {
            return Err(MusicLibraryError::validation(field, "is required"));
        }
    }
    if request.requested_at_ms <= 0 {
        return Err(MusicLibraryError::validation(
            "requestedAt",
            "must be a positive Unix epoch millisecond value",
        ));
    }
    if !Path::new(&request.folder_path).is_absolute() {
        return Err(MusicLibraryError::validation(
            "folderPath",
            "must be an absolute directory path",
        ));
    }
    if request.available_roots.len() > 256 {
        return Err(MusicLibraryError::validation(
            "availableRoots",
            "cannot contain more than 256 device-local bindings",
        ));
    }
    let mut root_ids = HashSet::new();
    for (index, binding) in request.available_roots.iter().enumerate() {
        if binding.root_id.trim().is_empty() || !Path::new(&binding.folder_path).is_absolute() {
            return Err(MusicLibraryError::validation(
                format!("availableRoots[{index}]"),
                "requires a root id and absolute folder path",
            ));
        }
        if !root_ids.insert(binding.root_id.as_str()) {
            return Err(MusicLibraryError::validation(
                "availableRoots",
                "cannot contain the same logical root twice",
            ));
        }
    }
    if !request.available_roots.iter().any(|binding| {
        binding.root_id == request.root_id
            && Path::new(&binding.folder_path) == Path::new(&request.folder_path)
    }) {
        return Err(MusicLibraryError::validation(
            "availableRoots",
            "must include the root being refreshed with the selected folder path",
        ));
    }
    Ok(())
}

pub(super) fn new_item_id(
    request: &MusicLocalRefreshRequest,
    media: &LocalMediaEvidence,
) -> String {
    stable_id("item", &[&request.root_id, &media.relative_path])
}

pub(super) fn stable_id(kind: &str, values: &[&str]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(kind.as_bytes());
    for value in values {
        hasher.update([0]);
        hasher.update(value.as_bytes());
    }
    format!("music-{kind}-{:x}", hasher.finalize())
}

pub(super) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or(1)
}

pub(super) fn map_conflict(context: &str, error: sqlx::Error) -> MusicLibraryError {
    if error
        .as_database_error()
        .is_some_and(|database| database.is_unique_violation())
    {
        MusicLibraryError::conflict(format!("{context}: the canonical identity is already used"))
    } else {
        MusicLibraryError::database(context, error)
    }
}
