use super::*;
use ganbaru_music::sync_keys;
use sqlx::{Sqlite, SqlitePool, Transaction};
use std::path::Path;

pub(super) fn map_database_error(context: &str, error: sqlx::Error) -> MusicLibraryError {
    if error
        .as_database_error()
        .is_some_and(|database| database.is_unique_violation())
    {
        MusicLibraryError::conflict(format!("{context}: the record already exists"))
    } else {
        MusicLibraryError::database(context, error)
    }
}

pub(super) async fn commit(
    transaction: Transaction<'_, Sqlite>,
    context: &str,
) -> MusicLibraryResult<()> {
    transaction
        .commit()
        .await
        .map_err(|error| MusicLibraryError::database(context, error))
}

/// Makes a playlist's intended uses equal `uses`, inserting and deleting only the differences.
/// Returns whether any row changed.
pub(super) async fn replace_intended_uses(
    transaction: &mut Transaction<'_, Sqlite>,
    playlist_id: &str,
    uses: &[MusicIntendedUse],
) -> MusicLibraryResult<bool> {
    let stored: Vec<String> = sqlx::query_scalar(
        "SELECT intended_use FROM music_playlist_intended_uses WHERE playlist_id = ?",
    )
    .bind(playlist_id)
    .fetch_all(&mut **transaction)
    .await
    .map_err(|error| MusicLibraryError::database("load playlist intended uses", error))?;
    let desired = uses.iter().map(AsRef::as_ref).collect::<Vec<&str>>();
    let mut changed = false;
    for value in stored
        .iter()
        .filter(|value| !desired.contains(&value.as_str()))
    {
        sqlx::query(
            "DELETE FROM music_playlist_intended_uses WHERE playlist_id = ? AND intended_use = ?",
        )
        .bind(playlist_id)
        .bind(value)
        .execute(&mut **transaction)
        .await
        .map_err(|error| MusicLibraryError::database("remove playlist intended use", error))?;
        changed = true;
    }
    for value in desired {
        if stored.iter().any(|stored| stored == value) {
            continue;
        }
        changed |= insert_intended_use(transaction, playlist_id, value).await?;
    }
    Ok(changed)
}

/// Adds one intended use to a playlist unless it is already there. Returns whether a row was
/// inserted.
pub(super) async fn insert_intended_use(
    transaction: &mut Transaction<'_, Sqlite>,
    playlist_id: &str,
    intended_use: &str,
) -> MusicLibraryResult<bool> {
    let sync_key = sync_keys::intended_use_key(playlist_id, intended_use)?;
    let inserted = sqlx::query(
        "INSERT INTO music_playlist_intended_uses (playlist_id, intended_use, sync_key)
         VALUES (?, ?, ?)
         ON CONFLICT DO NOTHING",
    )
    .bind(playlist_id)
    .bind(intended_use)
    .bind(sync_key)
    .execute(&mut **transaction)
    .await
    .map_err(|error| map_database_error("save music playlist intended use", error))?;
    Ok(inserted.rows_affected() > 0)
}

/// Makes an item's signals equal `signals`, inserting and deleting only the differences.
/// Returns whether any row changed.
pub(super) async fn replace_item_signals(
    transaction: &mut Transaction<'_, Sqlite>,
    item_id: &str,
    signals: &[MusicItemSignal],
    created_at_ms: i64,
) -> MusicLibraryResult<bool> {
    let stored: Vec<String> =
        sqlx::query_scalar("SELECT signal FROM music_item_signals WHERE item_id = ?")
            .bind(item_id)
            .fetch_all(&mut **transaction)
            .await
            .map_err(|error| MusicLibraryError::database("load music item signals", error))?;
    let desired = signals.iter().map(AsRef::as_ref).collect::<Vec<&str>>();
    let mut changed = false;
    for value in stored
        .iter()
        .filter(|value| !desired.contains(&value.as_str()))
    {
        sqlx::query("DELETE FROM music_item_signals WHERE item_id = ? AND signal = ?")
            .bind(item_id)
            .bind(value)
            .execute(&mut **transaction)
            .await
            .map_err(|error| MusicLibraryError::database("remove music item signal", error))?;
        changed = true;
    }
    for value in desired {
        if stored.iter().any(|stored| stored == value) {
            continue;
        }
        let inserted = sqlx::query(
            "INSERT INTO music_item_signals (item_id, signal, created_at_ms, sync_key)
             VALUES (?, ?, ?, ?)
             ON CONFLICT DO NOTHING",
        )
        .bind(item_id)
        .bind(value)
        .bind(created_at_ms)
        .bind(sync_keys::signal_key(item_id, value)?)
        .execute(&mut **transaction)
        .await
        .map_err(|error| MusicLibraryError::database("save music item signal", error))?;
        changed |= inserted.rows_affected() > 0;
    }
    Ok(changed)
}

async fn playlist_exists(
    transaction: &mut Transaction<'_, Sqlite>,
    playlist_id: &str,
) -> MusicLibraryResult<bool> {
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM music_playlists WHERE id = ?)")
        .bind(playlist_id)
        .fetch_one(&mut **transaction)
        .await
        .map_err(|error| MusicLibraryError::database("check music playlist", error))
}

pub async fn create_playlist(
    pool: &SqlitePool,
    request: MusicPlaylistCreate,
) -> MusicLibraryResult<MusicWriteReceipt> {
    validate_playlist_create(&request)?;
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| MusicLibraryError::database("begin create music playlist", error))?;
    let tail = super::order::playlist_tail(&mut transaction).await?;
    let order_key = super::order::key_after(tail.as_deref())?;
    sqlx::query(
        "INSERT INTO music_playlists
            (id, name, icon, shuffle_enabled, mix_enabled, repeat_mode, order_key, created_at_ms, updated_at_ms, version)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 1)",
    )
    .bind(&request.id)
    .bind(request.name.trim())
    .bind(request.icon.trim())
    .bind(request.shuffle_enabled)
    .bind(request.mix_enabled)
    .bind(request.repeat_mode.as_ref())
    .bind(order_key.as_str())
    .bind(request.created_at_ms)
    .bind(request.created_at_ms)
    .execute(&mut *transaction)
    .await
    .map_err(|error| map_database_error("create music playlist", error))?;
    for intended_use in request.intended_uses {
        insert_intended_use(&mut transaction, &request.id, intended_use.as_ref()).await?;
    }
    commit(transaction, "commit create music playlist").await?;
    Ok(MusicWriteReceipt {
        id: request.id,
        version: 1,
    })
}

pub async fn update_playlist(
    pool: &SqlitePool,
    request: MusicPlaylistUpdate,
) -> MusicLibraryResult<MusicWriteReceipt> {
    validate_playlist_update(&request)?;
    let protected_identity = super::defaults::built_in_music_playlist(&request.id);
    let protected_name = protected_identity
        .map(|playlist| playlist.name)
        .unwrap_or(request.name.trim());
    let protected_icon = protected_identity
        .map(|playlist| playlist.icon)
        .unwrap_or(request.icon.trim());
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| MusicLibraryError::database("begin update music playlist", error))?;
    let version: Option<i64> =
        sqlx::query_scalar("SELECT version FROM music_playlists WHERE id = ?")
            .bind(&request.id)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(|error| MusicLibraryError::database("load music playlist version", error))?;
    match version {
        None => return Err(MusicLibraryError::not_found("music playlist", &request.id)),
        Some(version) if version != request.expected_version => {
            return Err(MusicLibraryError::stale("music playlist", &request.id));
        }
        Some(_) => {}
    }
    let row_changed = sqlx::query(
        "UPDATE music_playlists
         SET name = ?, icon = ?, shuffle_enabled = ?, mix_enabled = ?, repeat_mode = ?,
             updated_at_ms = ?, version = version + 1
         WHERE id = ?
           AND (name, icon, shuffle_enabled, mix_enabled, repeat_mode) IS NOT (?, ?, ?, ?, ?)",
    )
    .bind(protected_name)
    .bind(protected_icon)
    .bind(request.shuffle_enabled)
    .bind(request.mix_enabled)
    .bind(request.repeat_mode.as_ref())
    .bind(request.updated_at_ms)
    .bind(&request.id)
    .bind(protected_name)
    .bind(protected_icon)
    .bind(request.shuffle_enabled)
    .bind(request.mix_enabled)
    .bind(request.repeat_mode.as_ref())
    .execute(&mut *transaction)
    .await
    .map_err(|error| map_database_error("update music playlist", error))?
    .rows_affected()
        > 0;
    let uses_changed =
        replace_intended_uses(&mut transaction, &request.id, &request.intended_uses).await?;
    if uses_changed && !row_changed {
        sqlx::query(
            "UPDATE music_playlists SET updated_at_ms = ?, version = version + 1 WHERE id = ?",
        )
        .bind(request.updated_at_ms)
        .bind(&request.id)
        .execute(&mut *transaction)
        .await
        .map_err(|error| MusicLibraryError::database("version music playlist", error))?;
    }
    let changed = row_changed || uses_changed;
    if changed {
        super::search::refresh_playlist(&mut transaction, &request.id).await?;
    }
    commit(transaction, "commit update music playlist").await?;
    Ok(MusicWriteReceipt {
        id: request.id,
        version: request.expected_version + i64::from(changed),
    })
}

pub async fn duplicate_playlist(
    pool: &SqlitePool,
    request: MusicPlaylistDuplicate,
) -> MusicLibraryResult<MusicWriteReceipt> {
    validate_playlist_duplicate(&request)?;
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| MusicLibraryError::database("begin duplicate music playlist", error))?;
    let source: Option<(String, i64, i64, String)> = sqlx::query_as(
        "SELECT icon, shuffle_enabled, mix_enabled, repeat_mode FROM music_playlists WHERE id = ?",
    )
    .bind(&request.source_playlist_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|error| MusicLibraryError::database("load source music playlist", error))?;
    let Some((icon, shuffle_enabled, mix_enabled, repeat_mode)) = source else {
        return Err(MusicLibraryError::not_found(
            "music playlist",
            &request.source_playlist_id,
        ));
    };
    let tail = super::order::playlist_tail(&mut transaction).await?;
    let order_key = super::order::key_after(tail.as_deref())?;
    sqlx::query(
        "INSERT INTO music_playlists
            (id, name, icon, shuffle_enabled, mix_enabled, repeat_mode, order_key, created_at_ms, updated_at_ms, version)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 1)",
    )
    .bind(&request.new_playlist_id)
    .bind(request.name.trim())
    .bind(icon)
    .bind(shuffle_enabled)
    .bind(mix_enabled)
    .bind(repeat_mode)
    .bind(order_key.as_str())
    .bind(request.created_at_ms)
    .bind(request.created_at_ms)
    .execute(&mut *transaction)
    .await
    .map_err(|error| map_database_error("duplicate music playlist", error))?;
    let source_uses: Vec<String> = sqlx::query_scalar(
        "SELECT intended_use FROM music_playlist_intended_uses WHERE playlist_id = ?",
    )
    .bind(&request.source_playlist_id)
    .fetch_all(&mut *transaction)
    .await
    .map_err(|error| MusicLibraryError::database("duplicate playlist intended uses", error))?;
    for intended_use in source_uses {
        insert_intended_use(&mut transaction, &request.new_playlist_id, &intended_use).await?;
    }
    duplicate_memberships(
        &mut transaction,
        &request.source_playlist_id,
        &request.new_playlist_id,
        request.created_at_ms,
    )
    .await?;
    super::search::refresh_playlist(&mut transaction, &request.new_playlist_id).await?;
    commit(transaction, "commit duplicate music playlist").await?;
    Ok(MusicWriteReceipt {
        id: request.new_playlist_id,
        version: 1,
    })
}

/// Copies every membership of `source_playlist_id`, with its skip ranges and break item, into
/// `new_playlist_id` under row keys derived from the new playlist and fresh rank order keys in
/// the source order.
async fn duplicate_memberships(
    transaction: &mut Transaction<'_, Sqlite>,
    source_playlist_id: &str,
    new_playlist_id: &str,
    created_at_ms: i64,
) -> MusicLibraryResult<()> {
    let sources: Vec<(String, String)> = sqlx::query_as(
        "SELECT id, item_id FROM music_playlist_memberships WHERE playlist_id = ?
         ORDER BY order_key, id",
    )
    .bind(source_playlist_id)
    .fetch_all(&mut **transaction)
    .await
    .map_err(|error| MusicLibraryError::database("load duplicated memberships", error))?;
    let order_keys = super::order::rank_keys(sources.len())?;
    for ((source_id, item_id), order_key) in sources.into_iter().zip(order_keys) {
        let copy_id = super::identity::membership_id(new_playlist_id, &item_id)?;
        sqlx::query(
            "INSERT INTO music_playlist_memberships
                (id, playlist_id, item_id, order_key, weight, enabled, start_ms, end_ms,
                 volume_milli, rate_milli, break_item_id, break_start_ms, break_end_ms,
                 break_volume_milli, break_rate_milli, created_at_ms, updated_at_ms, version)
             SELECT ?, ?, item_id, ?, weight, enabled, start_ms, end_ms,
                    volume_milli, rate_milli, break_item_id, break_start_ms, break_end_ms,
                    break_volume_milli, break_rate_milli, ?, ?, 1
             FROM music_playlist_memberships WHERE id = ?",
        )
        .bind(&copy_id)
        .bind(new_playlist_id)
        .bind(order_key.as_str())
        .bind(created_at_ms)
        .bind(created_at_ms)
        .bind(&source_id)
        .execute(&mut **transaction)
        .await
        .map_err(|error| map_database_error("duplicate playlist membership", error))?;
        sqlx::query(
            "INSERT INTO music_membership_skip_ranges (membership_id, ordinal, start_ms, end_ms)
             SELECT ?, ordinal, start_ms, end_ms
             FROM music_membership_skip_ranges WHERE membership_id = ?",
        )
        .bind(&copy_id)
        .bind(&source_id)
        .execute(&mut **transaction)
        .await
        .map_err(|error| MusicLibraryError::database("duplicate membership skip ranges", error))?;
    }
    Ok(())
}

async fn delete_impact_in_transaction(
    transaction: &mut Transaction<'_, Sqlite>,
    playlist_id: &str,
) -> MusicLibraryResult<MusicPlaylistDeleteImpact> {
    super::defaults::reject_built_in_playlist_delete(playlist_id)?;
    if !playlist_exists(transaction, playlist_id).await? {
        return Err(MusicLibraryError::not_found("music playlist", playlist_id));
    }
    let counts: (i64, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT
            (SELECT COUNT(*) FROM music_playlist_memberships WHERE playlist_id = ?),
            (SELECT COUNT(*) FROM projects p WHERE focus_playlist_id = ?
             AND NOT EXISTS (SELECT 1 FROM music_context_assignments a
                 WHERE a.owner_kind = 'project-default' AND a.owner_id = p.id
                   AND a.phase = 'focus' AND a.playlist_id = ?)),
            (SELECT COUNT(*) FROM projects p WHERE break_playlist_id = ?
             AND NOT EXISTS (SELECT 1 FROM music_context_assignments a
                 WHERE a.owner_kind = 'project-default' AND a.owner_id = p.id
                   AND a.phase IN ('short-break', 'long-break') AND a.playlist_id = ?)),
            (SELECT COUNT(*) FROM calendar_events e WHERE playlist_id = ?
             AND NOT EXISTS (SELECT 1 FROM music_context_assignments a
                 WHERE a.owner_kind = 'event-override' AND a.owner_id = e.id
                   AND a.phase = 'focus' AND a.playlist_id = ?)),
            (SELECT COUNT(*) FROM music_context_assignments WHERE playlist_id = ?)",
    )
    .bind(playlist_id)
    .bind(playlist_id)
    .bind(playlist_id)
    .bind(playlist_id)
    .bind(playlist_id)
    .bind(playlist_id)
    .bind(playlist_id)
    .bind(playlist_id)
    .fetch_one(&mut **transaction)
    .await
    .map_err(|error| MusicLibraryError::database("load playlist delete impact", error))?;
    let assignment_rows: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT 'project-focus', p.id, p.name FROM projects p WHERE focus_playlist_id = ?
           AND NOT EXISTS (SELECT 1 FROM music_context_assignments a
             WHERE a.owner_kind = 'project-default' AND a.owner_id = p.id
               AND a.phase = 'focus' AND a.playlist_id = ?)
         UNION ALL SELECT 'project-break', p.id, p.name FROM projects p WHERE break_playlist_id = ?
           AND NOT EXISTS (SELECT 1 FROM music_context_assignments a
             WHERE a.owner_kind = 'project-default' AND a.owner_id = p.id
               AND a.phase IN ('short-break', 'long-break') AND a.playlist_id = ?)
         UNION ALL SELECT 'calendar-event', e.id, e.title FROM calendar_events e WHERE playlist_id = ?
           AND NOT EXISTS (SELECT 1 FROM music_context_assignments a
             WHERE a.owner_kind = 'event-override' AND a.owner_id = e.id
               AND a.phase = 'focus' AND a.playlist_id = ?)
         UNION ALL SELECT 'context-assignment',
           a.owner_kind || ':' || a.owner_id || ':' || a.phase,
           COALESCE(p.name, e.title, a.owner_id) || ' · ' || a.phase
           FROM music_context_assignments a
           LEFT JOIN projects p ON a.owner_kind = 'project-default' AND p.id = a.owner_id
           LEFT JOIN calendar_events e ON a.owner_kind IN ('event-snapshot', 'event-override') AND e.id = a.owner_id
           WHERE a.playlist_id = ?
         ORDER BY 1, 3, 2",
    )
    .bind(playlist_id)
    .bind(playlist_id)
    .bind(playlist_id)
    .bind(playlist_id)
    .bind(playlist_id)
    .bind(playlist_id)
    .bind(playlist_id)
    .fetch_all(&mut **transaction)
    .await
    .map_err(|error| MusicLibraryError::database("load playlist assignment references", error))?;
    let assignments = assignment_rows
        .into_iter()
        .map(|(kind, id, label)| {
            Ok(MusicPlaylistAssignmentReference {
                kind: MusicPlaylistAssignmentKind::try_from(kind.as_str()).map_err(|message| {
                    MusicLibraryError::runtime("decode playlist assignment kind", message)
                })?,
                id,
                label,
            })
        })
        .collect::<MusicLibraryResult<Vec<_>>>()?;
    Ok(MusicPlaylistDeleteImpact {
        membership_count: counts.0,
        project_focus_assignment_count: counts.1,
        project_break_assignment_count: counts.2,
        calendar_assignment_count: counts.3,
        context_assignment_count: counts.4,
        assignments,
    })
}

pub async fn playlist_delete_impact(
    pool: &SqlitePool,
    playlist_id: &str,
) -> MusicLibraryResult<MusicPlaylistDeleteImpact> {
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| MusicLibraryError::database("begin playlist impact read", error))?;
    let impact = delete_impact_in_transaction(&mut transaction, playlist_id).await?;
    commit(transaction, "commit playlist impact read").await?;
    Ok(impact)
}

pub async fn delete_playlist(
    pool: &SqlitePool,
    request: MusicPlaylistDelete,
) -> MusicLibraryResult<MusicPlaylistDeleteImpact> {
    validate_playlist_delete(&request)?;
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| MusicLibraryError::database("begin delete music playlist", error))?;
    let actual_impact =
        delete_impact_in_transaction(&mut transaction, &request.playlist_id).await?;
    if actual_impact != request.expected_impact {
        return Err(MusicLibraryError::conflict(
            "playlist assignments changed after the delete confirmation was shown",
        ));
    }
    if let Some(replacement_id) = &request.replacement_playlist_id {
        if !playlist_exists(&mut transaction, replacement_id).await? {
            return Err(MusicLibraryError::not_found(
                "replacement music playlist",
                replacement_id,
            ));
        }
    }
    let affected_item_ids: Vec<String> = sqlx::query_scalar(
        "SELECT item_id FROM music_playlist_memberships WHERE playlist_id = ? ORDER BY item_id",
    )
    .bind(&request.playlist_id)
    .fetch_all(&mut *transaction)
    .await
    .map_err(|error| MusicLibraryError::database("load deleted playlist search items", error))?;
    // Project and Calendar event rows belong to their own domains and read a deleted
    // playlist as none, so the replacement rewrites Music assignments only.
    sqlx::query(
        "UPDATE music_context_assignments
         SET playlist_id = ?, version = version + 1
         WHERE playlist_id = ?",
    )
    .bind(&request.replacement_playlist_id)
    .bind(&request.playlist_id)
    .execute(&mut *transaction)
    .await
    .map_err(|error| {
        MusicLibraryError::database("repair contextual playlist assignments", error)
    })?;
    let deleted = sqlx::query("DELETE FROM music_playlists WHERE id = ? AND version = ?")
        .bind(&request.playlist_id)
        .bind(request.expected_version)
        .execute(&mut *transaction)
        .await
        .map_err(|error| MusicLibraryError::database("delete music playlist", error))?;
    if deleted.rows_affected() == 0 {
        return Err(MusicLibraryError::stale(
            "music playlist",
            &request.playlist_id,
        ));
    }
    for item_id in affected_item_ids {
        super::search::refresh_item(&mut transaction, &item_id).await?;
    }
    commit(transaction, "commit delete music playlist").await?;
    Ok(actual_impact)
}

pub async fn set_review_state(
    pool: &SqlitePool,
    request: MusicReviewWrite,
) -> MusicLibraryResult<MusicWriteReceipt> {
    validate_review_write(&request)?;
    let result = sqlx::query(
        "UPDATE music_library_items
         SET review_state = ?, review_changed_at_ms = ?, review_deferred_until_ms = ?, updated_at_ms = ?, version = version + 1
         WHERE id = ? AND version = ?",
    )
    .bind(request.review_state.as_ref())
    .bind(request.updated_at_ms)
    .bind(request.deferred_until)
    .bind(request.updated_at_ms)
    .bind(&request.item_id)
    .bind(request.expected_version)
    .execute(pool)
    .await
    .map_err(|error| MusicLibraryError::database("update music review state", error))?;
    if result.rows_affected() == 0 {
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM music_library_items WHERE id = ?)")
                .bind(&request.item_id)
                .fetch_one(pool)
                .await
                .map_err(|error| MusicLibraryError::database("check music library item", error))?;
        return Err(if exists {
            MusicLibraryError::stale("music library item", &request.item_id)
        } else {
            MusicLibraryError::not_found("music library item", &request.item_id)
        });
    }
    Ok(MusicWriteReceipt {
        id: request.item_id,
        version: request.expected_version + 1,
    })
}

/// Sets the metadata overrides of an item. An artwork override must be a managed reference
/// whose verified file is already stored in `vault_root`; its exact size is stored with it.
pub async fn set_metadata_overrides(
    pool: &SqlitePool,
    vault_root: &Path,
    request: MusicMetadataOverrideWrite,
) -> MusicLibraryResult<MusicWriteReceipt> {
    validate_metadata_override_write(&request)?;
    let artwork = match request.artwork_override.clone() {
        // Reading and describing up to 10 MiB of artwork blocks, so it leaves the async worker.
        Some(reference) => {
            let vault_root = vault_root.to_path_buf();
            let artwork = tokio::task::spawn_blocking(move || {
                crate::managed_artwork::stored_artwork(&vault_root, &reference)
            })
            .await
            .map_err(|error| MusicLibraryError::runtime("read stored music artwork", error))??;
            Some(artwork)
        }
        None => None,
    };
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| MusicLibraryError::database("begin metadata override update", error))?;
    let result = sqlx::query(
        "UPDATE music_library_items
         SET title_override = ?, artist_override = ?, album_override = ?, artwork_override = ?,
             artwork_override_byte_size = ?, updated_at_ms = ?, version = version + 1
         WHERE id = ? AND version = ?",
    )
    .bind(request.title_override.as_deref().map(str::trim))
    .bind(request.artist_override.as_deref().map(str::trim))
    .bind(request.album_override.as_deref().map(str::trim))
    .bind(artwork.as_ref().map(|artwork| artwork.reference.as_str()))
    .bind(
        artwork
            .as_ref()
            .map(|artwork| i64::from(artwork.asset.byte_size())),
    )
    .bind(request.updated_at_ms)
    .bind(&request.item_id)
    .bind(request.expected_version)
    .execute(&mut *transaction)
    .await
    .map_err(|error| MusicLibraryError::database("update music metadata overrides", error))?;
    if result.rows_affected() == 0 {
        return Err(MusicLibraryError::stale(
            "music library item",
            &request.item_id,
        ));
    }
    super::search::refresh_item(&mut transaction, &request.item_id).await?;
    commit(transaction, "commit metadata override update").await?;
    Ok(MusicWriteReceipt {
        id: request.item_id,
        version: request.expected_version + 1,
    })
}

pub async fn set_item_signals(
    pool: &SqlitePool,
    request: MusicItemSignalsWrite,
) -> MusicLibraryResult<Vec<MusicWriteReceipt>> {
    validate_item_signals_write(&request)?;
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| MusicLibraryError::database("begin music signal update", error))?;
    let mut receipts = Vec::with_capacity(request.item_ids.len());
    for item_id in &request.item_ids {
        let version: Option<i64> =
            sqlx::query_scalar("SELECT version FROM music_library_items WHERE id = ?")
                .bind(item_id)
                .fetch_optional(&mut *transaction)
                .await
                .map_err(|error| {
                    MusicLibraryError::database("load music item for signal update", error)
                })?;
        let Some(version) = version else {
            return Err(MusicLibraryError::not_found("music library item", item_id));
        };
        if !replace_item_signals(
            &mut transaction,
            item_id,
            &request.signals,
            request.updated_at_ms,
        )
        .await?
        {
            receipts.push(MusicWriteReceipt {
                id: item_id.clone(),
                version,
            });
            continue;
        }
        sqlx::query(
            "UPDATE music_library_items SET updated_at_ms = ?, version = version + 1 WHERE id = ?",
        )
        .bind(request.updated_at_ms)
        .bind(item_id)
        .execute(&mut *transaction)
        .await
        .map_err(|error| MusicLibraryError::database("version music signal update", error))?;
        super::search::refresh_item(&mut transaction, item_id).await?;
        receipts.push(MusicWriteReceipt {
            id: item_id.clone(),
            version: version + 1,
        });
    }
    commit(transaction, "commit music signal update").await?;
    Ok(receipts)
}

/// Saves one membership matched by playlist and item. A new membership gets the derived row key
/// and is appended to the playlist; an existing one keeps its row key and place. An unchanged
/// save writes nothing and returns the current version.
async fn upsert_membership_in_transaction(
    transaction: &mut Transaction<'_, Sqlite>,
    membership: &MusicMembershipWrite,
) -> MusicLibraryResult<MusicWriteReceipt> {
    let existing: Option<(String, i64)> = sqlx::query_as(
        "SELECT id, version FROM music_playlist_memberships WHERE playlist_id = ? AND item_id = ?",
    )
    .bind(&membership.playlist_id)
    .bind(&membership.item_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(|error| MusicLibraryError::database("load playlist membership", error))?;
    let Some((id, version)) = existing else {
        let id = super::identity::membership_id(&membership.playlist_id, &membership.item_id)?;
        if membership.expected_version.is_some() {
            return Err(MusicLibraryError::not_found("playlist membership", &id));
        }
        let tail = super::order::membership_tail(transaction, &membership.playlist_id).await?;
        let order_key = super::order::key_after(tail.as_deref())?;
        sqlx::query(
            "INSERT INTO music_playlist_memberships
                (id, playlist_id, item_id, order_key, weight, enabled,
                 start_ms, end_ms, volume_milli, rate_milli, created_at_ms, updated_at_ms, version)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1)",
        )
        .bind(&id)
        .bind(&membership.playlist_id)
        .bind(&membership.item_id)
        .bind(order_key.as_str())
        .bind(membership.weight.as_ref())
        .bind(membership.enabled)
        .bind(membership.start_ms)
        .bind(membership.end_ms)
        .bind(membership.volume_milli)
        .bind(membership.rate_milli)
        .bind(membership.updated_at_ms)
        .bind(membership.updated_at_ms)
        .execute(&mut **transaction)
        .await
        .map_err(|error| map_database_error("save playlist membership", error))?;
        super::search::refresh_item(transaction, &membership.item_id).await?;
        return Ok(MusicWriteReceipt { id, version: 1 });
    };
    if membership
        .expected_version
        .is_some_and(|expected| expected != version)
    {
        return Err(MusicLibraryError::stale("playlist membership", &id));
    }
    let updated = sqlx::query(
        "UPDATE music_playlist_memberships
         SET weight = ?, enabled = ?, start_ms = ?, end_ms = ?,
             volume_milli = ?, rate_milli = ?, updated_at_ms = ?, version = version + 1
         WHERE id = ?
           AND (weight, enabled, start_ms, end_ms, volume_milli, rate_milli)
               IS NOT (?, ?, ?, ?, ?, ?)",
    )
    .bind(membership.weight.as_ref())
    .bind(membership.enabled)
    .bind(membership.start_ms)
    .bind(membership.end_ms)
    .bind(membership.volume_milli)
    .bind(membership.rate_milli)
    .bind(membership.updated_at_ms)
    .bind(&id)
    .bind(membership.weight.as_ref())
    .bind(membership.enabled)
    .bind(membership.start_ms)
    .bind(membership.end_ms)
    .bind(membership.volume_milli)
    .bind(membership.rate_milli)
    .execute(&mut **transaction)
    .await
    .map_err(|error| map_database_error("update playlist membership", error))?
    .rows_affected();
    if updated == 0 {
        return Ok(MusicWriteReceipt { id, version });
    }
    super::search::refresh_item(transaction, &membership.item_id).await?;
    Ok(MusicWriteReceipt {
        id,
        version: version + 1,
    })
}

pub async fn upsert_memberships(
    pool: &SqlitePool,
    request: MusicBulkMembershipWrite,
) -> MusicLibraryResult<Vec<MusicWriteReceipt>> {
    validate_bulk_membership_write(&request)?;
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| MusicLibraryError::database("begin save playlist memberships", error))?;
    let mut receipts = Vec::with_capacity(request.memberships.len());
    for membership in &request.memberships {
        receipts.push(upsert_membership_in_transaction(&mut transaction, membership).await?);
    }
    commit(transaction, "commit save playlist memberships").await?;
    Ok(receipts)
}

pub async fn save_advanced_membership(
    pool: &SqlitePool,
    request: MusicAdvancedMembershipWrite,
) -> MusicLibraryResult<MusicAdvancedMembershipReceipt> {
    validate_advanced_membership_write(&request)?;
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| MusicLibraryError::database("begin advanced membership update", error))?;
    let membership =
        upsert_membership_in_transaction(&mut transaction, &request.membership).await?;
    let skip_ranges =
        replace_skip_ranges(&mut transaction, &membership.id, &request.skip_ranges).await?;
    commit(transaction, "commit advanced membership update").await?;
    Ok(MusicAdvancedMembershipReceipt {
        membership,
        skip_ranges,
    })
}

/// Makes the membership's skip ranges equal `ranges` in order, writing only the ordinals whose
/// bounds changed and deleting the ones past the end.
pub(super) async fn replace_skip_ranges(
    transaction: &mut Transaction<'_, Sqlite>,
    membership_id: &str,
    ranges: &[MusicMembershipSkipRangeWrite],
) -> MusicLibraryResult<Vec<MusicMembershipSkipRange>> {
    let stored: Vec<(i64, i64, i64)> = sqlx::query_as(
        "SELECT ordinal, start_ms, end_ms FROM music_membership_skip_ranges
         WHERE membership_id = ? ORDER BY ordinal",
    )
    .bind(membership_id)
    .fetch_all(&mut **transaction)
    .await
    .map_err(|error| MusicLibraryError::database("load membership skip ranges", error))?;
    let stored = stored
        .into_iter()
        .map(|(ordinal, start_ms, end_ms)| (ordinal, (start_ms, end_ms)))
        .collect::<std::collections::HashMap<_, _>>();
    let count = i64::try_from(ranges.len())
        .map_err(|error| MusicLibraryError::runtime("count membership skip ranges", error))?;
    sqlx::query(
        "DELETE FROM music_membership_skip_ranges WHERE membership_id = ? AND ordinal >= ?",
    )
    .bind(membership_id)
    .bind(count)
    .execute(&mut **transaction)
    .await
    .map_err(|error| MusicLibraryError::database("remove membership skip ranges", error))?;
    let mut saved = Vec::with_capacity(ranges.len());
    for (ordinal, range) in (0_i64..).zip(ranges) {
        let bounds = (range.start_ms, range.end_ms);
        if stored.get(&ordinal) != Some(&bounds) {
            sqlx::query(
                "INSERT INTO music_membership_skip_ranges (membership_id, ordinal, start_ms, end_ms)
                 VALUES (?, ?, ?, ?)
                 ON CONFLICT(membership_id, ordinal)
                 DO UPDATE SET start_ms = excluded.start_ms, end_ms = excluded.end_ms",
            )
            .bind(membership_id)
            .bind(ordinal)
            .bind(range.start_ms)
            .bind(range.end_ms)
            .execute(&mut **transaction)
            .await
            .map_err(|error| MusicLibraryError::database("save membership skip range", error))?;
        }
        saved.push(MusicMembershipSkipRange {
            membership_id: membership_id.to_string(),
            ordinal,
            start_ms: range.start_ms,
            end_ms: range.end_ms,
        });
    }
    Ok(saved)
}

pub async fn remove_memberships(
    pool: &SqlitePool,
    request: MusicMembershipRemove,
) -> MusicLibraryResult<()> {
    validate_membership_remove(&request)?;
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| MusicLibraryError::database("begin remove playlist memberships", error))?;
    for membership_id in &request.membership_ids {
        let item_id: Option<String> =
            sqlx::query_scalar("SELECT item_id FROM music_playlist_memberships WHERE id = ?")
                .bind(membership_id)
                .fetch_optional(&mut *transaction)
                .await
                .map_err(|error| {
                    MusicLibraryError::database("load removed membership item", error)
                })?;
        let Some(item_id) = item_id else {
            return Err(MusicLibraryError::not_found(
                "playlist membership",
                membership_id,
            ));
        };
        let deleted = sqlx::query("DELETE FROM music_playlist_memberships WHERE id = ?")
            .bind(membership_id)
            .execute(&mut *transaction)
            .await
            .map_err(|error| MusicLibraryError::database("remove playlist membership", error))?;
        debug_assert_eq!(deleted.rows_affected(), 1);
        super::search::refresh_item(&mut transaction, &item_id).await?;
    }
    commit(transaction, "commit remove playlist memberships").await
}

pub async fn remove_snooze(
    pool: &SqlitePool,
    request: MusicSnoozeRemove,
) -> MusicLibraryResult<()> {
    validate_snooze_remove(&request)?;
    let result = sqlx::query("DELETE FROM music_snoozes WHERE id = ?")
        .bind(&request.snooze_id)
        .execute(pool)
        .await
        .map_err(|error| MusicLibraryError::database("remove music snooze", error))?;
    if result.rows_affected() == 0 {
        return Err(MusicLibraryError::not_found(
            "music snooze",
            &request.snooze_id,
        ));
    }
    Ok(())
}

pub async fn upsert_source_collection(
    pool: &SqlitePool,
    request: MusicCollectionWrite,
) -> MusicLibraryResult<MusicWriteReceipt> {
    let identity_key = validate_collection_write(&request)?;
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| MusicLibraryError::database("begin source collection save", error))?;
    let existing: Option<(String, i64)> =
        sqlx::query_as("SELECT id, version FROM music_source_collections WHERE identity_key = ?")
            .bind(&identity_key)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(|error| MusicLibraryError::database("load source collection", error))?;
    let receipt = match existing {
        Some((id, version)) => {
            let changed = sqlx::query(
                "UPDATE music_source_collections
                 SET name = ?, updated_at_ms = ?, version = version + 1
                 WHERE id = ? AND name IS NOT ?",
            )
            .bind(request.name.trim())
            .bind(request.updated_at_ms)
            .bind(&id)
            .bind(request.name.trim())
            .execute(&mut *transaction)
            .await
            .map_err(|error| map_database_error("rename music source collection", error))?
            .rows_affected();
            MusicWriteReceipt {
                id,
                version: version + i64::from(changed > 0),
            }
        }
        None => {
            let id = super::identity::collection_id(&identity_key);
            sqlx::query(
                "INSERT INTO music_source_collections
                    (id, kind, identity_key, name, local_root_id, youtube_playlist_id,
                     created_at_ms, updated_at_ms, version)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, 1)",
            )
            .bind(&id)
            .bind(request.kind.as_ref())
            .bind(&identity_key)
            .bind(request.name.trim())
            .bind(&request.local_root_id)
            .bind(&request.youtube_playlist_id)
            .bind(request.updated_at_ms)
            .bind(request.updated_at_ms)
            .execute(&mut *transaction)
            .await
            .map_err(|error| map_database_error("save music source collection", error))?;
            MusicWriteReceipt { id, version: 1 }
        }
    };
    commit(transaction, "commit source collection save").await?;
    Ok(receipt)
}

pub async fn create_local_root(
    pool: &SqlitePool,
    request: MusicLocalRootCreate,
) -> MusicLibraryResult<MusicWriteReceipt> {
    validate_local_root_create(&request)?;
    let identity_key = super::identity::local_root_identity_key(&request.root_id);
    let collection_id = super::identity::collection_id(&identity_key);
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| MusicLibraryError::database("begin local music root creation", error))?;
    sqlx::query(
        "INSERT INTO music_local_roots (id, name, created_at_ms, updated_at_ms)
         VALUES (?, ?, ?, ?)",
    )
    .bind(&request.root_id)
    .bind(request.name.trim())
    .bind(request.created_at_ms)
    .bind(request.created_at_ms)
    .execute(&mut *transaction)
    .await
    .map_err(|error| map_database_error("create local music root", error))?;
    sqlx::query(
        "INSERT INTO music_source_collections
            (id, kind, identity_key, name, local_root_id, created_at_ms, updated_at_ms)
         VALUES (?, 'local-root', ?, ?, ?, ?, ?)",
    )
    .bind(&collection_id)
    .bind(&identity_key)
    .bind(request.name.trim())
    .bind(&request.root_id)
    .bind(request.created_at_ms)
    .bind(request.created_at_ms)
    .execute(&mut *transaction)
    .await
    .map_err(|error| map_database_error("create local music source", error))?;
    commit(transaction, "commit local music root creation").await?;
    Ok(MusicWriteReceipt {
        id: collection_id,
        version: 1,
    })
}
