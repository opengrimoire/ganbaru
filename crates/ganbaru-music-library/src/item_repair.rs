use super::local_refresh::RelinkArtworkCache;
use super::*;
use sqlx::{Row, SqlitePool};
use std::path::{Path, PathBuf};

pub async fn preview(
    pool: &SqlitePool,
    item_id: &str,
    file_path: &str,
) -> MusicLibraryResult<MusicItemRepairPreview> {
    if item_id.trim().is_empty() {
        return Err(MusicLibraryError::validation("itemId", "is required"));
    }
    let path = PathBuf::from(file_path);
    if !path.is_absolute() || !path.is_file() || !crate::media::is_supported_media_path(&path) {
        return Err(MusicLibraryError::validation(
            "filePath",
            "must be an existing supported media file",
        ));
    }
    let folder = path
        .parent()
        .ok_or_else(|| MusicLibraryError::validation("filePath", "must have a parent folder"))?;
    let relative_path = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| MusicLibraryError::validation("filePath", "must be valid UTF-8"))?;
    let item =
        sqlx::query("SELECT original_title, duration_ms FROM music_library_items WHERE id = ?")
            .bind(item_id)
            .fetch_optional(pool)
            .await
            .map_err(|error| MusicLibraryError::database("load item for location repair", error))?
            .ok_or_else(|| MusicLibraryError::not_found("music library item", item_id))?;
    let mut artwork_cache = RelinkArtworkCache::new();
    let evidence = local_refresh::inspect_for_relink(folder, relative_path, &mut artwork_cache)
        .map_err(|message| MusicLibraryError::runtime("inspect replacement media", message))?;
    let strong_fingerprint = local_refresh::strong_fingerprint_for_repair(&path)
        .map_err(|message| MusicLibraryError::runtime("fingerprint replacement media", message))?;
    let locations = sqlx::query(
        "SELECT file_size_bytes, lightweight_fingerprint, strong_fingerprint
         FROM music_local_locations WHERE item_id = ?",
    )
    .bind(item_id)
    .fetch_all(pool)
    .await
    .map_err(|error| MusicLibraryError::database("load existing item locations", error))?;
    let exact = locations.iter().any(|location| {
        location
            .get::<Option<String>, _>("strong_fingerprint")
            .as_deref()
            == Some(strong_fingerprint.as_str())
    });
    let likely_fingerprint = locations.iter().any(|location| {
        location.get::<Option<i64>, _>("file_size_bytes") == Some(evidence.file_size_bytes)
            && location
                .get::<Option<String>, _>("lightweight_fingerprint")
                .as_deref()
                == Some(evidence.lightweight_fingerprint.as_str())
    });
    let original_title: String = item.get("original_title");
    let original_duration: Option<i64> = item.get("duration_ms");
    let likely_metadata = !original_title.trim().is_empty()
        && normalized_text(&original_title) == normalized_text(&evidence.title)
        && duration_near(original_duration, evidence.duration_ms);
    let (match_strength, reasons) = if exact {
        (
            MusicRepairMatchStrength::Exact,
            vec!["The full file fingerprint matches a known location.".to_string()],
        )
    } else if likely_fingerprint || likely_metadata {
        let mut reasons = Vec::new();
        if likely_fingerprint {
            reasons.push("The sampled fingerprint and file size match.".to_string());
        }
        if likely_metadata {
            reasons.push("The title and duration closely match the library item.".to_string());
        }
        (MusicRepairMatchStrength::Likely, reasons)
    } else {
        (
            MusicRepairMatchStrength::Weak,
            vec!["No strong identity evidence matches this library item.".to_string()],
        )
    };
    Ok(MusicItemRepairPreview {
        item_id: item_id.to_string(),
        folder_path: folder.to_string_lossy().into_owned(),
        relative_path: relative_path.to_string(),
        title: evidence.title,
        artist: evidence.artist,
        album: evidence.album,
        duration_ms: evidence.duration_ms,
        file_size_bytes: evidence.file_size_bytes,
        lightweight_fingerprint: evidence.lightweight_fingerprint,
        strong_fingerprint,
        match_strength,
        reasons,
    })
}

pub async fn apply(
    pool: &SqlitePool,
    request: MusicItemRepairApply,
) -> MusicLibraryResult<MusicWriteReceipt> {
    validate_item_repair_apply(&request)?;
    let full_path = Path::new(&request.folder_path).join(&request.relative_path);
    let inspected = preview(pool, &request.item_id, &full_path.to_string_lossy()).await?;
    if inspected.strong_fingerprint != request.expected_strong_fingerprint {
        return Err(MusicLibraryError::conflict(
            "the selected file changed after the repair preview",
        ));
    }
    if inspected.match_strength == MusicRepairMatchStrength::Weak && !request.accept_weak_mismatch {
        return Err(MusicLibraryError::validation(
            "acceptWeakMismatch",
            "must be confirmed when identity evidence is weak",
        ));
    }
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| MusicLibraryError::database("begin item location repair", error))?;
    sqlx::query(
        "INSERT INTO music_local_roots (id, name, created_at_ms, updated_at_ms)
         VALUES (?, ?, ?, ?)",
    )
    .bind(&request.root_id)
    .bind(request.root_name.trim())
    .bind(request.applied_at)
    .bind(request.applied_at)
    .execute(&mut *transaction)
    .await
    .map_err(|error| super::writes::map_database_error("create repair location root", error))?;
    sqlx::query(
        "INSERT INTO music_local_locations
            (id, item_id, root_id, relative_path, file_size_bytes, modified_at_ms,
             lightweight_fingerprint, strong_fingerprint, availability, first_seen_at_ms, updated_at_ms)
         VALUES (?, ?, ?, ?, ?, NULL, ?, ?, 'available', ?, ?)",
    )
    .bind(&request.location_id)
    .bind(&request.item_id)
    .bind(&request.root_id)
    .bind(&request.relative_path)
    .bind(inspected.file_size_bytes)
    .bind(&inspected.lightweight_fingerprint)
    .bind(&inspected.strong_fingerprint)
    .bind(request.applied_at)
    .bind(request.applied_at)
    .execute(&mut *transaction)
    .await
    .map_err(|error| super::writes::map_database_error("save repaired item location", error))?;
    super::writes::commit(transaction, "commit item location repair").await?;
    Ok(MusicWriteReceipt {
        id: request.location_id,
        version: 1,
    })
}

fn normalized_text(value: &str) -> String {
    value.trim().to_lowercase()
}

fn duration_near(left: Option<i64>, right: Option<i64>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => left.abs_diff(right) <= 2_000,
        _ => false,
    }
}

pub async fn undo(pool: &SqlitePool, location_id: &str, root_id: &str) -> MusicLibraryResult<()> {
    if location_id.trim().is_empty() || root_id.trim().is_empty() {
        return Err(MusicLibraryError::validation(
            "locationId",
            "repair location and root ids are required",
        ));
    }
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| MusicLibraryError::database("begin item repair undo", error))?;
    let deleted = sqlx::query("DELETE FROM music_local_locations WHERE id = ? AND root_id = ?")
        .bind(location_id)
        .bind(root_id)
        .execute(&mut *transaction)
        .await
        .map_err(|error| MusicLibraryError::database("remove repaired item location", error))?
        .rows_affected();
    if deleted == 0 {
        return Err(MusicLibraryError::not_found(
            "repaired music location",
            location_id,
        ));
    }
    sqlx::query(
        "DELETE FROM music_local_roots
         WHERE id = ? AND NOT EXISTS (
             SELECT 1 FROM music_local_locations WHERE root_id = ?
         ) AND NOT EXISTS (
             SELECT 1 FROM music_source_collections WHERE local_root_id = ?
         )",
    )
    .bind(root_id)
    .bind(root_id)
    .bind(root_id)
    .execute(&mut *transaction)
    .await
    .map_err(|error| MusicLibraryError::database("remove empty repair root", error))?;
    super::writes::commit(transaction, "commit item repair undo").await
}
