use crate::identity::stable_id;
use sqlx::{Row, SqlitePool, Transaction};
use std::collections::{HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

use super::local_refresh::{RelinkArtworkCache, RelinkMediaEvidence, inspect_for_relink};
use super::*;

const MAX_RELINK_DECISIONS: usize = 10_000;
/// Files and folders a relink scan visits before it stops with an error. Far above any music library, it keeps a mistaken pick such as a whole disk from running unbounded.
const MAX_RELINK_SCAN_ENTRIES: usize = 2_000_000;

pub async fn create_plan(
    pool: &SqlitePool,
    request: MusicRelinkPlanRequest,
) -> MusicLibraryResult<MusicRelinkPlanSummary> {
    create_plan_with_scan_limit(pool, request, MAX_RELINK_SCAN_ENTRIES).await
}

pub(crate) async fn create_plan_with_scan_limit(
    pool: &SqlitePool,
    request: MusicRelinkPlanRequest,
    scan_limit: usize,
) -> MusicLibraryResult<MusicRelinkPlanSummary> {
    validate_plan_request(&request)?;
    let replacement = Path::new(&request.replacement_folder_path);
    if !fs::metadata(replacement).is_ok_and(|metadata| metadata.is_dir()) {
        return Err(MusicLibraryError::validation(
            "replacementFolderPath",
            "must reference an accessible directory",
        ));
    }
    let root_exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM music_local_roots WHERE id = ?)")
            .bind(&request.root_id)
            .fetch_one(pool)
            .await
            .map_err(|error| MusicLibraryError::database("load relink root", error))?;
    if !root_exists {
        return Err(MusicLibraryError::not_found(
            "logical music root",
            &request.root_id,
        ));
    }
    sqlx::query(
        "INSERT INTO music_relink_plans (id, root_id, state, created_at_ms, updated_at_ms)
         VALUES (?, ?, 'planning', ?, ?)",
    )
    .bind(&request.plan_id)
    .bind(&request.root_id)
    .bind(request.created_at_ms)
    .bind(request.created_at_ms)
    .execute(pool)
    .await
    .map_err(|error| map_plan_conflict("create relink plan", error))?;

    let result = scan_plan(pool, &request, replacement, scan_limit).await;
    if let Err(error) = &result {
        let _ = sqlx::query("DELETE FROM music_relink_plans WHERE id = ?")
            .bind(&request.plan_id)
            .execute(pool)
            .await;
        return Err(error.clone());
    }
    result
}

pub(crate) async fn scan_plan(
    pool: &SqlitePool,
    request: &MusicRelinkPlanRequest,
    replacement: &Path,
    scan_limit: usize,
) -> MusicLibraryResult<MusicRelinkPlanSummary> {
    let mut directories = VecDeque::from([PathBuf::new()]);
    let mut artwork_cache = RelinkArtworkCache::new();
    let mut visited_entries = 0_usize;
    while let Some(relative_directory) = directories.pop_front() {
        if !plan_is_planning(pool, &request.plan_id).await? {
            return plan_summary(pool, &request.plan_id).await;
        }
        let directory = replacement.join(&relative_directory);
        let mut entries = fs::read_dir(&directory)
            .map_err(|error| MusicLibraryError::runtime("scan relink folder", error))?
            .take(scan_limit.saturating_sub(visited_entries).saturating_add(1))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| MusicLibraryError::runtime("read relink folder", error))?;
        visited_entries = visited_entries.saturating_add(entries.len());
        if visited_entries > scan_limit {
            return Err(MusicLibraryError::validation(
                "replacementFolderPath",
                format!(
                    "contains more than {scan_limit} files and folders; choose the folder that holds this library"
                ),
            ));
        }
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let file_type = entry
                .file_type()
                .map_err(|error| MusicLibraryError::runtime("inspect relink candidate", error))?;
            if file_type.is_symlink() {
                continue;
            }
            let relative_path = normalize_relative(replacement, &entry.path())?;
            if file_type.is_dir() {
                directories.push_back(PathBuf::from(relative_path));
                continue;
            }
            if !file_type.is_file() || !crate::media::is_supported_media_path(&entry.path()) {
                continue;
            }
            if !plan_is_planning(pool, &request.plan_id).await? {
                return plan_summary(pool, &request.plan_id).await;
            }
            let evidence = inspect_for_relink(replacement, &relative_path, &mut artwork_cache)
                .map_err(|message| MusicLibraryError::runtime("inspect relink media", message))?;
            insert_candidate(pool, request, &evidence).await?;
        }
    }
    insert_missing_entries(pool, request).await?;
    finalize_plan(pool, request).await
}

async fn insert_candidate(
    pool: &SqlitePool,
    request: &MusicRelinkPlanRequest,
    evidence: &RelinkMediaEvidence,
) -> MusicLibraryResult<()> {
    let exact = sqlx::query(
        "SELECT id, item_id FROM music_local_locations
         WHERE root_id = ? AND REPLACE(relative_path, char(92), '/') = ?
         ORDER BY id LIMIT 1",
    )
    .bind(&request.root_id)
    .bind(&evidence.relative_path)
    .fetch_optional(pool)
    .await
    .map_err(|error| MusicLibraryError::database("match exact relink path", error))?;
    if let Some(row) = exact {
        return save_entry(
            pool,
            request,
            MusicRelinkMatchKind::Exact,
            Some(row.get("id")),
            Some(row.get("item_id")),
            Some(&evidence.relative_path),
            &[],
            Some(evidence),
        )
        .await;
    }

    let candidates = sqlx::query(
        "SELECT item_id, MIN(id) AS location_id
         FROM music_local_locations
         WHERE root_id = ? AND lightweight_fingerprint = ? AND file_size_bytes = ?
         GROUP BY item_id ORDER BY item_id LIMIT 33",
    )
    .bind(&request.root_id)
    .bind(&evidence.lightweight_fingerprint)
    .bind(evidence.file_size_bytes)
    .fetch_all(pool)
    .await
    .map_err(|error| MusicLibraryError::database("match relink evidence", error))?;
    let item_ids = candidates
        .iter()
        .map(|candidate| candidate.get::<String, _>("item_id"))
        .collect::<Vec<_>>();
    match candidates.as_slice() {
        [] => {
            save_entry(
                pool,
                request,
                MusicRelinkMatchKind::New,
                None,
                None,
                Some(&evidence.relative_path),
                &[],
                Some(evidence),
            )
            .await
        }
        [candidate] => {
            save_entry(
                pool,
                request,
                MusicRelinkMatchKind::Likely,
                Some(candidate.get("location_id")),
                Some(candidate.get("item_id")),
                Some(&evidence.relative_path),
                &item_ids,
                Some(evidence),
            )
            .await
        }
        _ => {
            save_entry(
                pool,
                request,
                MusicRelinkMatchKind::Ambiguous,
                None,
                None,
                Some(&evidence.relative_path),
                &item_ids,
                Some(evidence),
            )
            .await
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn save_entry(
    pool: &SqlitePool,
    request: &MusicRelinkPlanRequest,
    kind: MusicRelinkMatchKind,
    old_location_id: Option<String>,
    suggested_item_id: Option<String>,
    candidate_relative_path: Option<&str>,
    candidate_item_ids: &[String],
    evidence: Option<&RelinkMediaEvidence>,
) -> MusicLibraryResult<()> {
    let entry_id = stable_id(
        "relink-entry",
        &[
            &request.plan_id,
            kind.as_ref(),
            candidate_relative_path.unwrap_or_default(),
            old_location_id.as_deref().unwrap_or_default(),
        ],
    );
    let candidate_json = serde_json::to_string(candidate_item_ids)
        .map_err(|error| MusicLibraryError::runtime("encode relink candidates", error))?;
    sqlx::query(
        "INSERT INTO music_relink_plan_entries
            (id, plan_id, match_kind, old_location_id, suggested_item_id,
             candidate_relative_path, candidate_item_ids, file_size_bytes,
             lightweight_fingerprint, created_at_ms)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(entry_id)
    .bind(&request.plan_id)
    .bind(kind.as_ref())
    .bind(old_location_id)
    .bind(suggested_item_id)
    .bind(candidate_relative_path)
    .bind(candidate_json)
    .bind(evidence.map(|value| value.file_size_bytes))
    .bind(evidence.map(|value| value.lightweight_fingerprint.as_str()))
    .bind(request.created_at_ms)
    .execute(pool)
    .await
    .map_err(|error| map_plan_conflict("save relink candidate", error))?;
    Ok(())
}

async fn insert_missing_entries(
    pool: &SqlitePool,
    request: &MusicRelinkPlanRequest,
) -> MusicLibraryResult<()> {
    let locations = sqlx::query(
        "SELECT location.id, location.item_id
         FROM music_local_locations AS location
         WHERE location.root_id = ?
           AND NOT EXISTS (
              SELECT 1 FROM music_relink_plan_entries AS entry
              WHERE entry.plan_id = ? AND (
                 entry.old_location_id = location.id
                 OR entry.suggested_item_id = location.item_id
                 OR EXISTS (
                    SELECT 1 FROM json_each(entry.candidate_item_ids)
                    WHERE json_each.value = location.item_id
                 )
              )
           )
         ORDER BY location.id",
    )
    .bind(&request.root_id)
    .bind(&request.plan_id)
    .fetch_all(pool)
    .await
    .map_err(|error| MusicLibraryError::database("find missing relink locations", error))?;
    for location in locations {
        save_entry(
            pool,
            request,
            MusicRelinkMatchKind::Missing,
            Some(location.get("id")),
            Some(location.get("item_id")),
            None,
            &[],
            None,
        )
        .await?;
    }
    Ok(())
}

async fn finalize_plan(
    pool: &SqlitePool,
    request: &MusicRelinkPlanRequest,
) -> MusicLibraryResult<MusicRelinkPlanSummary> {
    sqlx::query(
        "UPDATE music_relink_plans SET state = 'ready',
            exact_count = (SELECT COUNT(*) FROM music_relink_plan_entries WHERE plan_id = ? AND match_kind = 'exact'),
            likely_count = (SELECT COUNT(*) FROM music_relink_plan_entries WHERE plan_id = ? AND match_kind = 'likely'),
            ambiguous_count = (SELECT COUNT(*) FROM music_relink_plan_entries WHERE plan_id = ? AND match_kind = 'ambiguous'),
            missing_count = (SELECT COUNT(*) FROM music_relink_plan_entries WHERE plan_id = ? AND match_kind = 'missing'),
            new_count = (SELECT COUNT(*) FROM music_relink_plan_entries WHERE plan_id = ? AND match_kind = 'new'),
            updated_at_ms = ? WHERE id = ? AND state = 'planning'",
    )
    .bind(&request.plan_id)
    .bind(&request.plan_id)
    .bind(&request.plan_id)
    .bind(&request.plan_id)
    .bind(&request.plan_id)
    .bind(request.created_at_ms)
    .bind(&request.plan_id)
    .execute(pool)
    .await
    .map_err(|error| MusicLibraryError::database("complete relink plan", error))?;
    plan_summary(pool, &request.plan_id).await
}

/// Whether the plan is still being built, so a scan stops once the plan is cancelled.
async fn plan_is_planning(pool: &SqlitePool, plan_id: &str) -> MusicLibraryResult<bool> {
    sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM music_relink_plans WHERE id = ? AND state = 'planning')",
    )
    .bind(plan_id)
    .fetch_one(pool)
    .await
    .map_err(|error| MusicLibraryError::database("check relink plan state", error))
}

pub(crate) async fn plan_summary(
    pool: &SqlitePool,
    plan_id: &str,
) -> MusicLibraryResult<MusicRelinkPlanSummary> {
    let row: (String, String, String, i64, i64, i64, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT id, root_id, state, exact_count, likely_count, ambiguous_count,
                    missing_count, new_count, created_at_ms, updated_at_ms
             FROM music_relink_plans WHERE id = ?",
    )
    .bind(plan_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| MusicLibraryError::database("load relink plan", error))?
    .ok_or_else(|| MusicLibraryError::not_found("music relink plan", plan_id))?;
    Ok(MusicRelinkPlanSummary {
        id: row.0,
        root_id: row.1,
        state: MusicRelinkPlanState::try_from(row.2.as_str())
            .map_err(|message| MusicLibraryError::validation("state", message))?,
        exact_count: row.3,
        likely_count: row.4,
        ambiguous_count: row.5,
        missing_count: row.6,
        new_count: row.7,
        created_at_ms: row.8,
        updated_at_ms: row.9,
    })
}

pub async fn plan_entries(
    pool: &SqlitePool,
    plan_id: &str,
    offset: i64,
    limit: i64,
) -> MusicLibraryResult<MusicRelinkPlanWindow> {
    validate_window(offset, limit)?;
    let total_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM music_relink_plan_entries WHERE plan_id = ?")
            .bind(plan_id)
            .fetch_one(pool)
            .await
            .map_err(|error| MusicLibraryError::database("count relink entries", error))?;
    let rows = sqlx::query(
        "SELECT id, match_kind, old_location_id, suggested_item_id,
                candidate_relative_path, candidate_item_ids, file_size_bytes,
                resolved_item_id, resolved_at_ms
         FROM music_relink_plan_entries WHERE plan_id = ?
         ORDER BY CASE match_kind WHEN 'ambiguous' THEN 0 WHEN 'likely' THEN 1
                  WHEN 'missing' THEN 2 WHEN 'new' THEN 3 ELSE 4 END,
                  candidate_relative_path, id LIMIT ? OFFSET ?",
    )
    .bind(plan_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
    .map_err(|error| MusicLibraryError::database("load relink entries", error))?;
    let entries = rows
        .into_iter()
        .map(|row| {
            let candidate_json: String = row.get("candidate_item_ids");
            let candidate_item_ids = serde_json::from_str::<Vec<String>>(&candidate_json)
                .map_err(|error| MusicLibraryError::runtime("decode relink candidates", error))?;
            Ok(MusicRelinkPlanEntry {
                id: row.get("id"),
                match_kind: MusicRelinkMatchKind::try_from(
                    row.get::<String, _>("match_kind").as_str(),
                )
                .map_err(|message| MusicLibraryError::validation("matchKind", message))?,
                old_location_id: row.get("old_location_id"),
                suggested_item_id: row.get("suggested_item_id"),
                candidate_relative_path: row.get("candidate_relative_path"),
                candidate_item_ids,
                file_size_bytes: row.get("file_size_bytes"),
                resolved_item_id: row.get("resolved_item_id"),
                resolved_at_ms: row.get("resolved_at_ms"),
            })
        })
        .collect::<MusicLibraryResult<Vec<_>>>()?;
    Ok(MusicRelinkPlanWindow {
        entries,
        total_count,
        offset,
        limit,
    })
}

pub async fn apply_plan(
    pool: &SqlitePool,
    request: MusicRelinkApplyRequest,
) -> MusicLibraryResult<MusicRelinkPlanSummary> {
    validate_apply_request(&request)?;
    let summary = plan_summary(pool, &request.plan_id).await?;
    if summary.state != MusicRelinkPlanState::Ready {
        return Err(MusicLibraryError::conflict(
            "only a ready relink plan can be applied",
        ));
    }
    let decision_map = request
        .decisions
        .iter()
        .map(|decision| (decision.entry_id.as_str(), decision.item_id.as_str()))
        .collect::<std::collections::HashMap<_, _>>();
    if decision_map.len() != request.decisions.len() {
        return Err(MusicLibraryError::validation(
            "decisions",
            "cannot contain an entry more than once",
        ));
    }
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| MusicLibraryError::database("begin relink apply", error))?;
    sqlx::query(
        "UPDATE music_local_locations SET availability = 'missing'
         WHERE root_id = ? AND availability IS NOT 'missing'",
    )
    .bind(&summary.root_id)
    .execute(&mut *transaction)
    .await
    .map_err(|error| MusicLibraryError::database("mark prior relink locations missing", error))?;
    let entries = load_apply_entries(&mut transaction, &request.plan_id).await?;
    let mut used_locations = HashSet::new();
    for entry in entries {
        let selected_item = match entry.match_kind {
            MusicRelinkMatchKind::Exact => entry.suggested_item_id.as_deref(),
            MusicRelinkMatchKind::Likely | MusicRelinkMatchKind::Ambiguous => {
                decision_map.get(entry.id.as_str()).copied()
            }
            MusicRelinkMatchKind::Missing | MusicRelinkMatchKind::New => None,
        };
        let Some(item_id) = selected_item else {
            continue;
        };
        validate_decision(&entry, item_id)?;
        let location_id = match entry.old_location_id {
            Some(location_id) if entry.suggested_item_id.as_deref() == Some(item_id) => location_id,
            _ => sqlx::query_scalar(
                "SELECT id FROM music_local_locations WHERE root_id = ? AND item_id = ?
                 ORDER BY id LIMIT 1",
            )
            .bind(&summary.root_id)
            .bind(item_id)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(|error| MusicLibraryError::database("find relink decision location", error))?
            .ok_or_else(|| {
                MusicLibraryError::conflict("a selected relink item has no prior location")
            })?,
        };
        if !used_locations.insert(location_id.clone()) {
            return Err(MusicLibraryError::validation(
                "decisions",
                "cannot map one prior location to several replacement files",
            ));
        }
        let relative_path = entry.candidate_relative_path.as_deref().ok_or_else(|| {
            MusicLibraryError::conflict("a selected relink entry has no replacement path")
        })?;
        relink_location(
            &mut transaction,
            &summary.root_id,
            &location_id,
            item_id,
            relative_path,
            entry.file_size_bytes,
            entry.lightweight_fingerprint.as_deref(),
            request.applied_at,
        )
        .await?;
        sqlx::query(
            "UPDATE music_relink_plan_entries
             SET resolved_item_id = ?, resolved_at_ms = ? WHERE id = ?",
        )
        .bind(item_id)
        .bind(request.applied_at)
        .bind(&entry.id)
        .execute(&mut *transaction)
        .await
        .map_err(|error| MusicLibraryError::database("resolve relink entry", error))?;
    }
    crate::local_scan::reconcile_item_availability(&mut transaction, &summary.root_id).await?;
    sqlx::query(
        "UPDATE music_relink_plans SET state = 'applied', updated_at_ms = ?
         WHERE id = ? AND state = 'ready'",
    )
    .bind(request.applied_at)
    .bind(&request.plan_id)
    .execute(&mut *transaction)
    .await
    .map_err(|error| MusicLibraryError::database("mark relink plan applied", error))?;
    sqlx::query(
        "UPDATE music_source_collections SET refresh_state = 'partial',
             last_refresh_error_code = 'refresh-required-after-relink',
             version = version + 1
         WHERE local_root_id = ?",
    )
    .bind(&summary.root_id)
    .execute(&mut *transaction)
    .await
    .map_err(|error| MusicLibraryError::database("mark relink source for refresh", error))?;
    transaction
        .commit()
        .await
        .map_err(|error| MusicLibraryError::database("commit relink mapping", error))?;
    plan_summary(pool, &request.plan_id).await
}

#[derive(Debug)]
struct ApplyEntry {
    id: String,
    match_kind: MusicRelinkMatchKind,
    old_location_id: Option<String>,
    suggested_item_id: Option<String>,
    candidate_relative_path: Option<String>,
    candidate_item_ids: Vec<String>,
    file_size_bytes: Option<i64>,
    lightweight_fingerprint: Option<String>,
}

/// Points a prior location at its replacement file. The path is part of the location key, so a
/// new path removes the prior location and adds one keyed by the new path.
#[allow(clippy::too_many_arguments)]
async fn relink_location(
    transaction: &mut Transaction<'_, sqlx::Sqlite>,
    root_id: &str,
    location_id: &str,
    item_id: &str,
    relative_path: &str,
    file_size_bytes: Option<i64>,
    lightweight_fingerprint: Option<&str>,
    applied_at: i64,
) -> MusicLibraryResult<()> {
    let stored_path: Option<String> =
        sqlx::query_scalar("SELECT relative_path FROM music_local_locations WHERE id = ?")
            .bind(location_id)
            .fetch_optional(&mut **transaction)
            .await
            .map_err(|error| MusicLibraryError::database("load relink location", error))?;
    let Some(stored_path) = stored_path else {
        return Err(MusicLibraryError::conflict(
            "a selected relink location no longer exists",
        ));
    };
    if stored_path == relative_path {
        sqlx::query(
            "UPDATE music_local_locations
             SET lightweight_fingerprint = COALESCE(?, lightweight_fingerprint),
                 updated_at_ms = ?
             WHERE id = ? AND lightweight_fingerprint IS NOT COALESCE(?, lightweight_fingerprint)",
        )
        .bind(lightweight_fingerprint)
        .bind(applied_at)
        .bind(location_id)
        .bind(lightweight_fingerprint)
        .execute(&mut **transaction)
        .await
        .map_err(|error| map_plan_conflict("apply relink mapping", error))?;
        sqlx::query(
            "UPDATE music_local_locations SET file_size_bytes = ?, availability = 'available'
             WHERE id = ?",
        )
        .bind(file_size_bytes)
        .bind(location_id)
        .execute(&mut **transaction)
        .await
        .map_err(|error| MusicLibraryError::database("observe relink location", error))?;
        return Ok(());
    }
    sqlx::query("DELETE FROM music_local_locations WHERE id = ?")
        .bind(location_id)
        .execute(&mut **transaction)
        .await
        .map_err(|error| MusicLibraryError::database("remove prior relink location", error))?;
    let relinked_id = crate::identity::creatable_key(
        &mut **transaction,
        crate::identity::RetirableTable::Locations,
        crate::identity::location_id(root_id, relative_path),
    )
    .await?;
    sqlx::query(
        "INSERT INTO music_local_locations
            (id, item_id, root_id, relative_path, file_size_bytes, lightweight_fingerprint,
             availability, first_seen_at_ms, updated_at_ms)
         VALUES (?, ?, ?, ?, ?, ?, 'available', ?, ?)",
    )
    .bind(&relinked_id)
    .bind(item_id)
    .bind(root_id)
    .bind(relative_path)
    .bind(file_size_bytes)
    .bind(lightweight_fingerprint)
    .bind(applied_at)
    .bind(applied_at)
    .execute(&mut **transaction)
    .await
    .map_err(|error| map_plan_conflict("apply relink mapping", error))?;
    Ok(())
}

async fn load_apply_entries(
    transaction: &mut Transaction<'_, sqlx::Sqlite>,
    plan_id: &str,
) -> MusicLibraryResult<Vec<ApplyEntry>> {
    let rows = sqlx::query(
        "SELECT id, match_kind, old_location_id, suggested_item_id,
                candidate_relative_path, candidate_item_ids, file_size_bytes,
                lightweight_fingerprint
         FROM music_relink_plan_entries WHERE plan_id = ? ORDER BY id",
    )
    .bind(plan_id)
    .fetch_all(&mut **transaction)
    .await
    .map_err(|error| MusicLibraryError::database("load relink apply entries", error))?;
    rows.into_iter()
        .map(|row| {
            let candidates: String = row.get("candidate_item_ids");
            Ok(ApplyEntry {
                id: row.get("id"),
                match_kind: MusicRelinkMatchKind::try_from(
                    row.get::<String, _>("match_kind").as_str(),
                )
                .map_err(|message| MusicLibraryError::validation("matchKind", message))?,
                old_location_id: row.get("old_location_id"),
                suggested_item_id: row.get("suggested_item_id"),
                candidate_relative_path: row.get("candidate_relative_path"),
                candidate_item_ids: serde_json::from_str(&candidates)
                    .map_err(|error| MusicLibraryError::runtime("decode relink decision", error))?,
                file_size_bytes: row.get("file_size_bytes"),
                lightweight_fingerprint: row.get("lightweight_fingerprint"),
            })
        })
        .collect()
}

fn validate_decision(entry: &ApplyEntry, item_id: &str) -> MusicLibraryResult<()> {
    let accepted = entry.suggested_item_id.as_deref() == Some(item_id)
        || entry
            .candidate_item_ids
            .iter()
            .any(|candidate| candidate == item_id);
    if !accepted {
        return Err(MusicLibraryError::validation(
            "decisions",
            "a selected item is not a candidate for its relink entry",
        ));
    }
    Ok(())
}

pub async fn cancel_plan(
    pool: &SqlitePool,
    plan_id: &str,
    cancelled_at: i64,
) -> MusicLibraryResult<MusicRelinkPlanSummary> {
    if cancelled_at <= 0 {
        return Err(MusicLibraryError::validation(
            "cancelledAt",
            "must be positive",
        ));
    }
    let result = sqlx::query(
        "UPDATE music_relink_plans SET state = 'cancelled', updated_at_ms = ?
         WHERE id = ? AND state IN ('planning', 'ready')",
    )
    .bind(cancelled_at)
    .bind(plan_id)
    .execute(pool)
    .await
    .map_err(|error| MusicLibraryError::database("cancel relink plan", error))?;
    if result.rows_affected() == 0 {
        let summary = plan_summary(pool, plan_id).await?;
        if summary.state != MusicRelinkPlanState::Cancelled {
            return Err(MusicLibraryError::conflict(
                "the relink plan can no longer be cancelled",
            ));
        }
    }
    plan_summary(pool, plan_id).await
}

fn validate_plan_request(request: &MusicRelinkPlanRequest) -> MusicLibraryResult<()> {
    if request.plan_id.trim().is_empty() || request.root_id.trim().is_empty() {
        return Err(MusicLibraryError::validation(
            "planId",
            "a plan id and logical root id are required",
        ));
    }
    if !Path::new(&request.replacement_folder_path).is_absolute() {
        return Err(MusicLibraryError::validation(
            "replacementFolderPath",
            "must be an absolute directory path",
        ));
    }
    if request.created_at_ms <= 0 {
        return Err(MusicLibraryError::validation(
            "createdAt",
            "must be positive",
        ));
    }
    Ok(())
}

fn validate_apply_request(request: &MusicRelinkApplyRequest) -> MusicLibraryResult<()> {
    if request.plan_id.trim().is_empty() || request.applied_at <= 0 {
        return Err(MusicLibraryError::validation(
            "planId",
            "a plan id and positive applied timestamp are required",
        ));
    }
    if request.decisions.len() > MAX_RELINK_DECISIONS {
        return Err(MusicLibraryError::validation(
            "decisions",
            format!("cannot contain more than {MAX_RELINK_DECISIONS} entries"),
        ));
    }
    if request
        .decisions
        .iter()
        .any(|decision| decision.entry_id.trim().is_empty() || decision.item_id.trim().is_empty())
    {
        return Err(MusicLibraryError::validation(
            "decisions",
            "every decision requires an entry id and item id",
        ));
    }
    Ok(())
}

fn validate_window(offset: i64, limit: i64) -> MusicLibraryResult<()> {
    if offset < 0 || !(1..=200).contains(&limit) {
        return Err(MusicLibraryError::validation(
            "limit",
            "offset must be non-negative and limit must be between 1 and 200",
        ));
    }
    Ok(())
}

fn normalize_relative(root: &Path, path: &Path) -> MusicLibraryResult<String> {
    let relative = path
        .strip_prefix(root)
        .map_err(|error| MusicLibraryError::runtime("normalize relink path", error))?;
    relative
        .components()
        .map(|component| {
            component
                .as_os_str()
                .to_str()
                .map(str::to_string)
                .ok_or_else(|| {
                    MusicLibraryError::validation(
                        "replacementFolderPath",
                        "contains a path that is not valid UTF-8",
                    )
                })
        })
        .collect::<MusicLibraryResult<Vec<_>>>()
        .map(|components| components.join("/"))
}

fn map_plan_conflict(context: &str, error: sqlx::Error) -> MusicLibraryError {
    if error
        .as_database_error()
        .is_some_and(|database| database.is_unique_violation())
    {
        MusicLibraryError::conflict(format!("{context}: the identifier or path is already used"))
    } else {
        MusicLibraryError::database(context, error)
    }
}
