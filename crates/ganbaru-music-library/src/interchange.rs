use super::*;
#[cfg(test)]
use sqlx::SqlitePool;
use sqlx::{Sqlite, Transaction};
use std::collections::{HashMap, HashSet};

const INTERCHANGE_FORMAT: &str = "ganbaru/music-playlists";
const INTERCHANGE_VERSION: i64 = 1;
const MAX_PLAYLISTS: usize = 500;
const MAX_MEMBERSHIPS: usize = 10_000;
type ImportedSnoozeKey = (String, String, Option<String>, i64, Option<i64>, String);

#[cfg(test)]
pub(crate) async fn import(
    pool: &SqlitePool,
    request: MusicInterchangeImportRequest,
) -> MusicLibraryResult<MusicInterchangeImportResult> {
    validate_import(&request)?;
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| MusicLibraryError::database("begin music playlist import", error))?;
    let result =
        import_in_transaction(&mut transaction, &request, &request.imported_at.to_string()).await?;
    super::writes::commit(transaction, "commit music playlist import").await?;
    Ok(result)
}

/// Applies a validated document within its caller's transaction, including search changes.
pub(super) async fn import_in_transaction(
    transaction: &mut Transaction<'_, Sqlite>,
    request: &MusicInterchangeImportRequest,
    operation_id: &str,
) -> MusicLibraryResult<MusicInterchangeImportResult> {
    validate_import(request)?;
    let mut item_ids = HashMap::<String, String>::new();
    let mut playlist_ids = HashMap::<String, String>::new();
    let mut changed_items = HashSet::<String>::new();
    let mut imported_snoozes = HashSet::<ImportedSnoozeKey>::new();
    let mut result = MusicInterchangeImportResult {
        playlist_count: 0,
        item_count: 0,
        membership_count: 0,
        assignment_count: 0,
    };

    import_roots(transaction, request).await?;
    for (playlist_index, playlist) in request.document.playlists.iter().enumerate() {
        let Some(target_playlist_id) =
            import_playlist(transaction, playlist, playlist_index, request, operation_id).await?
        else {
            continue;
        };
        playlist_ids.insert(playlist.id.clone(), target_playlist_id.clone());
        result.playlist_count += 1;
        let mut playlist_item_ids = HashSet::<String>::new();
        for (membership_index, membership) in playlist.memberships.iter().enumerate() {
            let item_id = import_item(
                transaction,
                &membership.item,
                &mut item_ids,
                &mut changed_items,
                request,
            )
            .await?;
            if !playlist_item_ids.insert(item_id.clone()) {
                continue;
            }
            import_membership(
                transaction,
                &target_playlist_id,
                &item_id,
                membership,
                (playlist_index, membership_index, operation_id),
                &mut imported_snoozes,
                request,
            )
            .await?;
            result.membership_count += 1;
        }
        remove_absent_memberships(
            transaction,
            &target_playlist_id,
            &playlist_item_ids,
            &mut changed_items,
        )
        .await?;
    }
    result.item_count = item_ids.len() as i64;
    if request.import_context_assignments {
        result.assignment_count = import_assignments(
            transaction,
            &request.document.context_assignments,
            &playlist_ids,
            request.imported_at,
        )
        .await?;
    }
    for item_id in changed_items {
        super::search::refresh_item(transaction, &item_id).await?;
    }
    Ok(result)
}

/// Deletes the playlist's memberships whose item the imported playlist no longer contains, so a
/// replaced playlist keeps the rows it shares with the import.
async fn remove_absent_memberships(
    transaction: &mut Transaction<'_, Sqlite>,
    playlist_id: &str,
    kept_item_ids: &HashSet<String>,
    changed_items: &mut HashSet<String>,
) -> MusicLibraryResult<()> {
    let stored: Vec<(String, String)> =
        sqlx::query_as("SELECT id, item_id FROM music_playlist_memberships WHERE playlist_id = ?")
            .bind(playlist_id)
            .fetch_all(&mut **transaction)
            .await
            .map_err(|error| MusicLibraryError::database("load replaced memberships", error))?;
    for (membership_id, item_id) in stored {
        if kept_item_ids.contains(&item_id) {
            continue;
        }
        sqlx::query("DELETE FROM music_playlist_memberships WHERE id = ?")
            .bind(&membership_id)
            .execute(&mut **transaction)
            .await
            .map_err(|error| MusicLibraryError::database("remove replaced membership", error))?;
        changed_items.insert(item_id);
    }
    Ok(())
}

pub(super) fn validate_import(request: &MusicInterchangeImportRequest) -> MusicLibraryResult<()> {
    if request.document.format != INTERCHANGE_FORMAT
        || request.document.version != INTERCHANGE_VERSION
    {
        return Err(MusicLibraryError::validation(
            "document",
            "must be a supported Ganbaru music export",
        ));
    }
    if request.imported_at <= 0 || request.document.exported_at <= 0 {
        return Err(MusicLibraryError::validation(
            "importedAt",
            "must be positive",
        ));
    }
    if request.document.playlists.len() > MAX_PLAYLISTS
        || request
            .document
            .playlists
            .iter()
            .map(|playlist| playlist.memberships.len())
            .sum::<usize>()
            > MAX_MEMBERSHIPS
    {
        return Err(MusicLibraryError::validation(
            "document",
            "exceeds the music import record limit",
        ));
    }
    let mut root_ids = HashSet::new();
    for root in &request.document.roots {
        validate_id(&root.id, "roots.id")?;
        if root.name.trim().is_empty() || !root_ids.insert(&root.id) {
            return Err(MusicLibraryError::validation(
                "roots",
                "must contain unique roots with names",
            ));
        }
    }
    let mut playlist_ids = HashSet::new();
    for playlist in &request.document.playlists {
        validate_id(&playlist.id, "playlists.id")?;
        if playlist.name.trim().is_empty() || !playlist_ids.insert(&playlist.id) {
            return Err(MusicLibraryError::validation(
                "playlists",
                "must contain unique playlists with names",
            ));
        }
        validate_icon(&playlist.icon)?;
        super::validation::validate_playlist_playback_mode(
            playlist.shuffle_enabled,
            playlist.mix_enabled,
        )?;
        for membership in &playlist.memberships {
            validate_item(&membership.item, &root_ids)?;
            if membership.position < 0
                || membership.start_ms.is_some_and(|value| value < 0)
                || membership.end_ms.is_some_and(|value| value < 0)
                || membership
                    .volume
                    .is_some_and(|value| !(0.0..=1.0).contains(&value))
                || membership
                    .rate
                    .is_some_and(|value| !(0.25..=2.0).contains(&value))
            {
                return Err(MusicLibraryError::validation(
                    "memberships",
                    "contains invalid playback settings",
                ));
            }
            let mut previous_end = None;
            for range in &membership.skip_ranges {
                if range.start_ms < 0
                    || range.end_ms <= range.start_ms
                    || previous_end.is_some_and(|end| range.start_ms < end)
                {
                    return Err(MusicLibraryError::validation(
                        "memberships.skipRanges",
                        "must be ordered and non-overlapping",
                    ));
                }
                previous_end = Some(range.end_ms);
            }
        }
    }
    Ok(())
}

fn validate_item(
    item: &MusicInterchangeItem,
    root_ids: &HashSet<&String>,
) -> MusicLibraryResult<()> {
    validate_id(&item.identity_key, "items.identityKey")?;
    if item.duration_ms.is_some_and(|value| value < 0)
        || (item.source_kind == MusicLibrarySourceKind::YouTubeVideo)
            != item
                .youtube_video_id
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty())
    {
        return Err(MusicLibraryError::validation(
            "items",
            "contains an invalid source identity",
        ));
    }
    let mut signals = HashSet::new();
    if item.signals.iter().any(|signal| !signals.insert(signal)) {
        return Err(MusicLibraryError::validation(
            "items.signals",
            "must not contain duplicates",
        ));
    }
    for location in &item.locations {
        if !root_ids.contains(&location.root_id) || !safe_relative_path(&location.relative_path) {
            return Err(MusicLibraryError::validation(
                "items.locations",
                "must reference an exported root with a safe relative path",
            ));
        }
    }
    Ok(())
}

pub(super) fn safe_relative_path(value: &str) -> bool {
    !value.trim().is_empty()
        && !value.starts_with(['/', '\\'])
        && !value.contains([':', '\0'])
        && !value.chars().any(char::is_control)
        && value
            .split(['/', '\\'])
            .all(|component| !component.is_empty() && component != "." && component != "..")
}

async fn import_roots(
    transaction: &mut Transaction<'_, Sqlite>,
    request: &MusicInterchangeImportRequest,
) -> MusicLibraryResult<()> {
    for root in &request.document.roots {
        sqlx::query(
            "INSERT INTO music_local_roots (id, name, created_at_ms, updated_at_ms, version)
             VALUES (?, ?, ?, ?, 1) ON CONFLICT(id) DO NOTHING",
        )
        .bind(&root.id)
        .bind(root.name.trim())
        .bind(request.imported_at)
        .bind(request.imported_at)
        .execute(&mut **transaction)
        .await
        .map_err(|error| MusicLibraryError::database("import logical music root", error))?;
    }
    Ok(())
}

async fn import_playlist(
    transaction: &mut Transaction<'_, Sqlite>,
    playlist: &MusicInterchangePlaylist,
    playlist_index: usize,
    request: &MusicInterchangeImportRequest,
    operation_id: &str,
) -> MusicLibraryResult<Option<String>> {
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM music_playlists WHERE id = ?)")
            .bind(&playlist.id)
            .fetch_one(&mut **transaction)
            .await
            .map_err(|error| MusicLibraryError::database("match imported playlist", error))?;
    if exists && request.playlist_conflict == MusicImportPlaylistConflict::KeepExisting {
        return Ok(None);
    }
    let target_id =
        if exists && request.playlist_conflict == MusicImportPlaylistConflict::ImportCopy {
            format!("music-import:{operation_id}:playlist:{playlist_index}")
        } else {
            playlist.id.clone()
        };
    if exists && request.playlist_conflict == MusicImportPlaylistConflict::ReplaceExisting {
        let protected_identity = super::defaults::built_in_music_playlist(&target_id);
        let protected_name = protected_identity
            .map(|playlist| playlist.name)
            .unwrap_or(playlist.name.trim());
        let protected_icon = protected_identity
            .map(|playlist| playlist.icon)
            .unwrap_or(playlist.icon.trim());
        sqlx::query(
            "UPDATE music_playlists SET name = ?, icon = ?, shuffle_enabled = ?, mix_enabled = ?, repeat_mode = ?, updated_at_ms = ?, version = version + 1
             WHERE id = ? AND (name, icon, shuffle_enabled, mix_enabled, repeat_mode) IS NOT (?, ?, ?, ?, ?)",
        )
        .bind(protected_name)
        .bind(protected_icon)
        .bind(i64::from(playlist.shuffle_enabled))
        .bind(i64::from(playlist.mix_enabled))
        .bind(playlist.repeat_mode.as_ref())
        .bind(request.imported_at)
        .bind(&target_id)
        .bind(protected_name)
        .bind(protected_icon)
        .bind(i64::from(playlist.shuffle_enabled))
        .bind(i64::from(playlist.mix_enabled))
        .bind(playlist.repeat_mode.as_ref())
        .execute(&mut **transaction)
        .await
        .map_err(|error| MusicLibraryError::database("replace imported playlist", error))?;
    } else {
        sqlx::query(
            "INSERT INTO music_playlists (id, name, icon, shuffle_enabled, mix_enabled, repeat_mode, sort_order, created_at_ms, updated_at_ms, version)
             VALUES (?, ?, ?, ?, ?, ?, (SELECT COALESCE(MAX(sort_order), -1) + 1 FROM music_playlists), ?, ?, 1)",
        )
        .bind(&target_id)
        .bind(playlist.name.trim())
        .bind(playlist.icon.trim())
        .bind(i64::from(playlist.shuffle_enabled))
        .bind(i64::from(playlist.mix_enabled))
        .bind(playlist.repeat_mode.as_ref())
        .bind(request.imported_at)
        .bind(request.imported_at)
        .execute(&mut **transaction)
        .await
        .map_err(|error| MusicLibraryError::database("import playlist", error))?;
    }
    super::writes::replace_intended_uses(transaction, &target_id, &playlist.intended_uses).await?;
    Ok(Some(target_id))
}

async fn import_item(
    transaction: &mut Transaction<'_, Sqlite>,
    item: &MusicInterchangeItem,
    item_ids: &mut HashMap<String, String>,
    changed_items: &mut HashSet<String>,
    request: &MusicInterchangeImportRequest,
) -> MusicLibraryResult<String> {
    if let Some(item_id) = item_ids.get(&item.identity_key) {
        return Ok(item_id.clone());
    }
    let existing: Option<String> =
        sqlx::query_scalar("SELECT id FROM music_library_items WHERE identity_key = ?")
            .bind(&item.identity_key)
            .fetch_optional(&mut **transaction)
            .await
            .map_err(|error| MusicLibraryError::database("match imported music item", error))?;
    let is_new = existing.is_none();
    let item_id = existing.unwrap_or_else(|| match &item.youtube_video_id {
        Some(video_id) if item.source_kind == MusicLibrarySourceKind::YouTubeVideo => {
            super::identity::youtube_item_id(video_id)
        }
        _ => super::identity::import_item_id(&item.identity_key),
    });
    if is_new {
        sqlx::query(
            "INSERT INTO music_library_items
                (id, identity_key, source_kind, media_kind, youtube_video_id, original_title,
                 original_artist, original_album, duration_ms, availability, review_state,
                 discovered_at_ms, updated_at_ms, version)
             VALUES (?, ?, ?, 'unknown', ?, ?, ?, ?, ?, ?, 'unreviewed', ?, ?, 1)",
        )
        .bind(&item_id)
        .bind(&item.identity_key)
        .bind(item.source_kind.as_ref())
        .bind(&item.youtube_video_id)
        .bind(bounded_item_description(&item.title))
        .bind(bounded_item_description(&item.artist))
        .bind(bounded_item_description(&item.album))
        .bind(item.duration_ms)
        .bind(if item.source_kind == MusicLibrarySourceKind::LocalFile {
            "missing"
        } else {
            "unknown"
        })
        .bind(request.imported_at)
        .bind(request.imported_at)
        .execute(&mut **transaction)
        .await
        .map_err(|error| MusicLibraryError::database("import music item", error))?;
    } else if request.replace_item_descriptions {
        sqlx::query(
            "UPDATE music_library_items SET original_title = ?, original_artist = ?, original_album = ?, duration_ms = ?, updated_at_ms = ?, version = version + 1
             WHERE id = ? AND (original_title, original_artist, original_album, duration_ms) IS NOT (?, ?, ?, ?)",
        )
        .bind(bounded_item_description(&item.title))
        .bind(bounded_item_description(&item.artist))
        .bind(bounded_item_description(&item.album))
        .bind(item.duration_ms)
        .bind(request.imported_at)
        .bind(&item_id)
        .bind(bounded_item_description(&item.title))
        .bind(bounded_item_description(&item.artist))
        .bind(bounded_item_description(&item.album))
        .bind(item.duration_ms)
        .execute(&mut **transaction)
        .await
        .map_err(|error| MusicLibraryError::database("replace imported item description", error))?;
    }
    if is_new || request.replace_item_descriptions {
        super::writes::replace_item_signals(
            transaction,
            &item_id,
            &item.signals,
            request.imported_at,
        )
        .await?;
    }
    for location in &item.locations {
        let relative_path = super::identity::normalize_relative_path(&location.relative_path);
        sqlx::query(
            "INSERT INTO music_local_locations
                (id, item_id, root_id, relative_path, availability, first_seen_at_ms, updated_at_ms)
             VALUES (?, ?, ?, ?, 'missing', ?, ?)
             ON CONFLICT DO NOTHING",
        )
        .bind(super::identity::location_id(
            &location.root_id,
            &relative_path,
        ))
        .bind(&item_id)
        .bind(&location.root_id)
        .bind(&relative_path)
        .bind(request.imported_at)
        .bind(request.imported_at)
        .execute(&mut **transaction)
        .await
        .map_err(|error| MusicLibraryError::database("import relative music location", error))?;
    }
    item_ids.insert(item.identity_key.clone(), item_id.clone());
    changed_items.insert(item_id.clone());
    Ok(item_id)
}

async fn import_membership(
    transaction: &mut Transaction<'_, Sqlite>,
    playlist_id: &str,
    item_id: &str,
    membership: &MusicInterchangeMembership,
    position: (usize, usize, &str),
    imported_snoozes: &mut HashSet<ImportedSnoozeKey>,
    request: &MusicInterchangeImportRequest,
) -> MusicLibraryResult<()> {
    let (playlist_index, membership_index, operation_id) = position;
    let existing: Option<String> = sqlx::query_scalar(
        "SELECT id FROM music_playlist_memberships WHERE playlist_id = ? AND item_id = ?",
    )
    .bind(playlist_id)
    .bind(item_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(|error| MusicLibraryError::database("match imported membership", error))?;
    let membership_id = match existing {
        Some(membership_id) => {
            update_imported_membership(transaction, &membership_id, membership, request).await?;
            membership_id
        }
        None => {
            let membership_id = super::identity::membership_id(playlist_id, item_id);
            sqlx::query(
                "INSERT INTO music_playlist_memberships
                    (id, playlist_id, item_id, position, weight, enabled, start_ms, end_ms,
                     volume, rate, created_at_ms, updated_at_ms, version)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1)",
            )
            .bind(&membership_id)
            .bind(playlist_id)
            .bind(item_id)
            .bind(membership.position)
            .bind(membership.weight.as_ref())
            .bind(i64::from(membership.enabled))
            .bind(membership.start_ms)
            .bind(membership.end_ms)
            .bind(membership.volume)
            .bind(membership.rate)
            .bind(request.imported_at)
            .bind(request.imported_at)
            .execute(&mut **transaction)
            .await
            .map_err(|error| MusicLibraryError::database("import playlist membership", error))?;
            membership_id
        }
    };
    let skip_ranges = membership
        .skip_ranges
        .iter()
        .map(|range| MusicMembershipSkipRangeWrite {
            start_ms: range.start_ms,
            end_ms: range.end_ms,
        })
        .collect::<Vec<_>>();
    super::writes::replace_skip_ranges(transaction, &membership_id, &skip_ranges).await?;
    for (snooze_index, snooze) in membership.snoozes.iter().enumerate() {
        let mapped_playlist_id = if snooze.scope == MusicSnoozeScope::Playlist {
            Some(playlist_id)
        } else {
            None
        };
        let snooze_key = (
            item_id.to_string(),
            snooze.scope.as_ref().to_string(),
            mapped_playlist_id.map(str::to_string),
            snooze.starts_at_ms,
            snooze.ends_at_ms,
            snooze.reason.clone(),
        );
        if !imported_snoozes.insert(snooze_key) {
            continue;
        }
        sqlx::query(
            "INSERT INTO music_snoozes (id, item_id, scope, playlist_id, starts_at_ms, ends_at_ms, reason, created_at_ms) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(format!(
            "music-import:{operation_id}:snooze:{playlist_index}:{membership_index}:{snooze_index}"
        ))
        .bind(item_id)
        .bind(snooze.scope.as_ref())
        .bind(mapped_playlist_id)
        .bind(snooze.starts_at_ms)
        .bind(snooze.ends_at_ms)
        .bind(&snooze.reason)
        .bind(request.imported_at)
        .execute(&mut **transaction)
        .await
        .map_err(|error| MusicLibraryError::database("import music snooze", error))?;
    }
    Ok(())
}

/// Updates a replaced playlist's existing membership to the imported values, writing only when
/// a value differs. Break items are not part of the interchange document, so a replace removes
/// them.
async fn update_imported_membership(
    transaction: &mut Transaction<'_, Sqlite>,
    membership_id: &str,
    membership: &MusicInterchangeMembership,
    request: &MusicInterchangeImportRequest,
) -> MusicLibraryResult<()> {
    sqlx::query(
        "UPDATE music_playlist_memberships
         SET position = ?, weight = ?, enabled = ?, start_ms = ?, end_ms = ?, volume = ?,
             rate = ?, updated_at_ms = ?, version = version + 1
         WHERE id = ?
           AND (position, weight, enabled, start_ms, end_ms, volume, rate)
               IS NOT (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(membership.position)
    .bind(membership.weight.as_ref())
    .bind(i64::from(membership.enabled))
    .bind(membership.start_ms)
    .bind(membership.end_ms)
    .bind(membership.volume)
    .bind(membership.rate)
    .bind(request.imported_at)
    .bind(membership_id)
    .bind(membership.position)
    .bind(membership.weight.as_ref())
    .bind(i64::from(membership.enabled))
    .bind(membership.start_ms)
    .bind(membership.end_ms)
    .bind(membership.volume)
    .bind(membership.rate)
    .execute(&mut **transaction)
    .await
    .map_err(|error| MusicLibraryError::database("replace imported membership", error))?;
    sqlx::query("DELETE FROM music_membership_break_items WHERE membership_id = ?")
        .bind(membership_id)
        .execute(&mut **transaction)
        .await
        .map_err(|error| MusicLibraryError::database("replace imported break items", error))?;
    Ok(())
}

async fn import_assignments(
    transaction: &mut Transaction<'_, Sqlite>,
    assignments: &[MusicContextAssignment],
    playlist_ids: &HashMap<String, String>,
    imported_at: i64,
) -> MusicLibraryResult<i64> {
    let mut count = 0;
    for assignment in assignments {
        let Some(imported_playlist_id) = assignment
            .playlist_id
            .as_ref()
            .and_then(|playlist_id| playlist_ids.get(playlist_id))
        else {
            continue;
        };
        sqlx::query(
            "INSERT INTO music_context_assignments
                (owner_kind, owner_id, phase, behavior, playlist_id, soundscape_preset_id,
                 soundscape_behavior, provenance_kind, provenance_id, updated_at_ms, version)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1)
             ON CONFLICT(owner_kind, owner_id, phase) DO UPDATE SET
                behavior = excluded.behavior, playlist_id = excluded.playlist_id,
                soundscape_preset_id = excluded.soundscape_preset_id,
                soundscape_behavior = excluded.soundscape_behavior,
                provenance_kind = excluded.provenance_kind,
                provenance_id = excluded.provenance_id,
                updated_at_ms = excluded.updated_at_ms, version = music_context_assignments.version + 1
             WHERE (music_context_assignments.behavior, music_context_assignments.playlist_id,
                    music_context_assignments.soundscape_preset_id,
                    music_context_assignments.soundscape_behavior,
                    music_context_assignments.provenance_kind,
                    music_context_assignments.provenance_id)
                IS NOT (excluded.behavior, excluded.playlist_id, excluded.soundscape_preset_id,
                        excluded.soundscape_behavior, excluded.provenance_kind,
                        excluded.provenance_id)",
        )
        .bind(assignment.owner_kind.as_ref())
        .bind(&assignment.owner_id)
        .bind(assignment.phase.as_ref())
        .bind(assignment.behavior.as_ref())
        .bind(imported_playlist_id)
        .bind(&assignment.soundscape_preset_id)
        .bind(assignment.soundscape_behavior.as_ref())
        .bind(assignment.provenance_kind.as_ref())
        .bind(&assignment.provenance_id)
        .bind(imported_at)
        .execute(&mut **transaction)
        .await
        .map_err(|error| MusicLibraryError::database("import music context assignment", error))?;
        count += 1;
    }
    Ok(count)
}
