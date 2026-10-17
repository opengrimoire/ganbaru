use super::*;
use sqlx::SqlitePool;
use std::collections::HashSet;

pub async fn bulk_edit_memberships(
    pool: &SqlitePool,
    request: MusicBulkMembershipEdit,
) -> MusicLibraryResult<MusicBulkMembershipResult> {
    validate_bulk_membership_edit(&request)?;
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| MusicLibraryError::database("begin bulk playlist edit", error))?;
    let mut changed_count = 0_i64;
    for playlist_id in &request.add_playlist_ids {
        let mut next_position: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(position), -1) + 1 FROM music_playlist_memberships WHERE playlist_id = ?",
        )
        .bind(playlist_id)
        .fetch_one(&mut *transaction)
        .await
        .map_err(|error| MusicLibraryError::database("load next playlist position", error))?;
        for (item_index, item_id) in request.item_ids.iter().enumerate() {
            let membership_id = format!("{}:{}:{}", request.action_id, item_index, playlist_id);
            let result = sqlx::query(
                "INSERT INTO music_playlist_memberships
                    (id, playlist_id, item_id, position, weight, enabled,
                     created_at_ms, updated_at_ms, version)
                 VALUES (?, ?, ?, ?, 'normal', 1, ?, ?, 1)
                 ON CONFLICT(playlist_id, item_id) DO NOTHING",
            )
            .bind(membership_id)
            .bind(playlist_id)
            .bind(item_id)
            .bind(next_position)
            .bind(request.updated_at_ms)
            .bind(request.updated_at_ms)
            .execute(&mut *transaction)
            .await
            .map_err(|error| MusicLibraryError::database("bulk add playlist membership", error))?;
            if result.rows_affected() > 0 {
                next_position += 1;
                changed_count += result.rows_affected() as i64;
                super::search::refresh_item(&mut transaction, item_id).await?;
            }
        }
    }
    for playlist_id in &request.remove_playlist_ids {
        for item_id in &request.item_ids {
            let result = sqlx::query(
                "DELETE FROM music_playlist_memberships WHERE playlist_id = ? AND item_id = ?",
            )
            .bind(playlist_id)
            .bind(item_id)
            .execute(&mut *transaction)
            .await
            .map_err(|error| {
                MusicLibraryError::database("bulk remove playlist membership", error)
            })?;
            if result.rows_affected() > 0 {
                changed_count += result.rows_affected() as i64;
                super::search::refresh_item(&mut transaction, item_id).await?;
            }
        }
    }
    if let Some(weight) = request.weight {
        for playlist_id in &request.weight_playlist_ids {
            for item_id in &request.item_ids {
                let result = sqlx::query(
                    "UPDATE music_playlist_memberships
                     SET weight = ?, updated_at_ms = ?, version = version + 1
                     WHERE playlist_id = ? AND item_id = ?",
                )
                .bind(weight.as_ref())
                .bind(request.updated_at_ms)
                .bind(playlist_id)
                .bind(item_id)
                .execute(&mut *transaction)
                .await
                .map_err(|error| {
                    MusicLibraryError::database("bulk update membership weight", error)
                })?;
                if result.rows_affected() > 0 {
                    changed_count += result.rows_affected() as i64;
                    super::search::refresh_item(&mut transaction, item_id).await?;
                }
            }
        }
    }
    super::writes::commit(transaction, "commit bulk playlist edit").await?;
    Ok(MusicBulkMembershipResult { changed_count })
}

pub async fn reorder_playlist(
    pool: &SqlitePool,
    request: MusicPlaylistReorder,
) -> MusicLibraryResult<MusicPlaylistReorderResult> {
    validate_playlist_reorder(&request)?;
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| MusicLibraryError::database("begin playlist reorder", error))?;
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT id, item_id FROM music_playlist_memberships
         WHERE playlist_id = ? ORDER BY position, id",
    )
    .bind(&request.playlist_id)
    .fetch_all(&mut *transaction)
    .await
    .map_err(|error| MusicLibraryError::database("load playlist order", error))?;
    let Some(source_index) = rows
        .iter()
        .position(|(_, item_id)| item_id == &request.item_id)
    else {
        return Err(MusicLibraryError::not_found(
            "playlist membership item",
            &request.item_id,
        ));
    };
    if request.target_index as usize >= rows.len() {
        return Err(MusicLibraryError::validation(
            "targetIndex",
            format!("must be less than {}", rows.len()),
        ));
    }
    let mut reordered = rows;
    let moved = reordered.remove(source_index);
    reordered.insert(request.target_index as usize, moved);
    for (position, (membership_id, _)) in reordered.iter().enumerate() {
        sqlx::query(
            "UPDATE music_playlist_memberships
             SET position = ?, updated_at_ms = ?, version = version + 1 WHERE id = ?",
        )
        .bind(position as i64)
        .bind(request.updated_at_ms)
        .bind(membership_id)
        .execute(&mut *transaction)
        .await
        .map_err(|error| MusicLibraryError::database("persist playlist order", error))?;
    }
    super::writes::commit(transaction, "commit playlist reorder").await?;
    Ok(MusicPlaylistReorderResult {
        item_ids: reordered.into_iter().map(|(_, item_id)| item_id).collect(),
    })
}

pub async fn reorder_playlists(
    pool: &SqlitePool,
    request: MusicPlaylistsReorder,
) -> MusicLibraryResult<Vec<MusicWriteReceipt>> {
    validate_playlists_reorder(&request)?;
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| MusicLibraryError::database("begin playlists reorder", error))?;
    let existing: Vec<String> = sqlx::query_scalar("SELECT id FROM music_playlists ORDER BY id")
        .fetch_all(&mut *transaction)
        .await
        .map_err(|error| MusicLibraryError::database("load playlists for reorder", error))?;
    let mut requested = request
        .playlists
        .iter()
        .map(|playlist| playlist.playlist_id.as_str())
        .collect::<Vec<_>>();
    requested.sort_unstable();
    if existing.iter().map(String::as_str).collect::<Vec<_>>() != requested {
        return Err(MusicLibraryError::conflict(
            "the playlist collection changed while it was being reordered",
        ));
    }

    let mut receipts = Vec::with_capacity(request.playlists.len());
    for (position, playlist) in request.playlists.iter().enumerate() {
        let result = sqlx::query(
            "UPDATE music_playlists
             SET sort_order = ?, updated_at_ms = ?, version = version + 1
             WHERE id = ? AND version = ?",
        )
        .bind(position as i64)
        .bind(request.updated_at_ms)
        .bind(&playlist.playlist_id)
        .bind(playlist.expected_version)
        .execute(&mut *transaction)
        .await
        .map_err(|error| MusicLibraryError::database("persist playlist order", error))?;
        if result.rows_affected() != 1 {
            return Err(MusicLibraryError::conflict(format!(
                "playlist {} changed while it was being reordered",
                playlist.playlist_id
            )));
        }
        receipts.push(MusicWriteReceipt {
            id: playlist.playlist_id.clone(),
            version: playlist.expected_version + 1,
        });
    }
    super::writes::commit(transaction, "commit playlists reorder").await?;
    Ok(receipts)
}

pub async fn bulk_set_review_state(
    pool: &SqlitePool,
    request: MusicBulkReviewWrite,
) -> MusicLibraryResult<MusicBulkMembershipResult> {
    validate_bulk_review_write(&request)?;
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| MusicLibraryError::database("begin bulk review update", error))?;
    for item in &request.items {
        let updated = sqlx::query(
            "UPDATE music_library_items
             SET review_state = ?, review_changed_at_ms = ?, review_deferred_until_ms = ?,
                 updated_at_ms = ?, version = version + 1
             WHERE id = ? AND version = ?",
        )
        .bind(request.review_state.as_ref())
        .bind(request.updated_at_ms)
        .bind(request.deferred_until)
        .bind(request.updated_at_ms)
        .bind(&item.item_id)
        .bind(item.expected_version)
        .execute(&mut *transaction)
        .await
        .map_err(|error| MusicLibraryError::database("bulk update review state", error))?;
        if updated.rows_affected() == 0 {
            return Err(MusicLibraryError::stale(
                "music library item",
                &item.item_id,
            ));
        }
        super::search::refresh_item(&mut transaction, &item.item_id).await?;
    }
    super::writes::commit(transaction, "commit bulk review update").await?;
    Ok(MusicBulkMembershipResult {
        changed_count: request.items.len() as i64,
    })
}

pub async fn apply_review_selection(
    pool: &SqlitePool,
    request: MusicReviewSelectionWrite,
) -> MusicLibraryResult<MusicReviewSelectionResult> {
    validate_review_selection_write(&request)?;
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| MusicLibraryError::database("begin review selection", error))?;

    let mut review_changes = Vec::with_capacity(request.items.len());
    for item in &request.items {
        let current = sqlx::query_as::<_, (i64, String, i64)>(
            "SELECT version, review_state,
                    (SELECT COUNT(*) FROM music_playlist_memberships WHERE item_id = music_library_items.id)
             FROM music_library_items WHERE id = ?",
        )
        .bind(&item.item_id)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(|error| MusicLibraryError::database("load review selection item", error))?;
        let Some((version, review_state, membership_count)) = current else {
            return Err(MusicLibraryError::not_found(
                "music library item",
                &item.item_id,
            ));
        };
        if version != item.expected_version {
            return Err(MusicLibraryError::stale(
                "music library item",
                &item.item_id,
            ));
        }
        if request.review_state == MusicReviewState::Ignored && membership_count > 0 {
            return Err(MusicLibraryError::validation(
                "items",
                "cannot ignore tracks assigned to a playlist",
            ));
        }
        review_changes.push(review_state != request.review_state.as_ref());
    }

    let mut membership_changed_count = 0_i64;
    let mut changed_item_ids = HashSet::new();
    for playlist_id in &request.add_playlist_ids {
        let mut next_position: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(position), -1) + 1 FROM music_playlist_memberships WHERE playlist_id = ?",
        )
        .bind(playlist_id)
        .fetch_one(&mut *transaction)
        .await
        .map_err(|error| MusicLibraryError::database("load review playlist position", error))?;
        for (item_index, item) in request.items.iter().enumerate() {
            let membership_id = format!("{}:{}:{}", request.action_id, item_index, playlist_id);
            let result = sqlx::query(
                "INSERT INTO music_playlist_memberships
                    (id, playlist_id, item_id, position, weight, enabled,
                     created_at_ms, updated_at_ms, version)
                 VALUES (?, ?, ?, ?, 'normal', 1, ?, ?, 1)
                 ON CONFLICT(playlist_id, item_id) DO NOTHING",
            )
            .bind(membership_id)
            .bind(playlist_id)
            .bind(&item.item_id)
            .bind(next_position)
            .bind(request.updated_at_ms)
            .bind(request.updated_at_ms)
            .execute(&mut *transaction)
            .await
            .map_err(|error| {
                MusicLibraryError::database("add review playlist membership", error)
            })?;
            if result.rows_affected() > 0 {
                next_position += 1;
                membership_changed_count += result.rows_affected() as i64;
                changed_item_ids.insert(item.item_id.clone());
            }
        }
    }
    for playlist_id in &request.remove_playlist_ids {
        for item in &request.items {
            let result = sqlx::query(
                "DELETE FROM music_playlist_memberships WHERE playlist_id = ? AND item_id = ?",
            )
            .bind(playlist_id)
            .bind(&item.item_id)
            .execute(&mut *transaction)
            .await
            .map_err(|error| {
                MusicLibraryError::database("remove review playlist membership", error)
            })?;
            if result.rows_affected() > 0 {
                membership_changed_count += result.rows_affected() as i64;
                changed_item_ids.insert(item.item_id.clone());
            }
        }
    }

    let mut review_changed_count = 0_i64;
    let mut receipts = Vec::with_capacity(request.items.len());
    for (item, should_update_review) in request.items.iter().zip(review_changes) {
        let version = if should_update_review {
            let result = sqlx::query(
                "UPDATE music_library_items
                 SET review_state = ?, review_changed_at_ms = ?, review_deferred_until_ms = NULL,
                     updated_at_ms = ?, version = version + 1
                 WHERE id = ? AND version = ?",
            )
            .bind(request.review_state.as_ref())
            .bind(request.updated_at_ms)
            .bind(request.updated_at_ms)
            .bind(&item.item_id)
            .bind(item.expected_version)
            .execute(&mut *transaction)
            .await
            .map_err(|error| MusicLibraryError::database("update review selection state", error))?;
            if result.rows_affected() == 0 {
                return Err(MusicLibraryError::stale(
                    "music library item",
                    &item.item_id,
                ));
            }
            review_changed_count += 1;
            changed_item_ids.insert(item.item_id.clone());
            item.expected_version + 1
        } else {
            item.expected_version
        };
        receipts.push(MusicWriteReceipt {
            id: item.item_id.clone(),
            version,
        });
    }

    for item_id in changed_item_ids {
        super::search::refresh_item(&mut transaction, &item_id).await?;
    }
    super::writes::commit(transaction, "commit review selection").await?;
    Ok(MusicReviewSelectionResult {
        membership_changed_count,
        review_changed_count,
        items: receipts,
    })
}

pub async fn bulk_snooze(
    pool: &SqlitePool,
    request: MusicBulkSnoozeWrite,
) -> MusicLibraryResult<MusicBulkMembershipResult> {
    validate_bulk_snooze_write(&request)?;
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| MusicLibraryError::database("begin bulk snooze", error))?;
    for (index, item_id) in request.item_ids.iter().enumerate() {
        sqlx::query(
            "INSERT INTO music_snoozes
                (id, item_id, scope, playlist_id, starts_at_ms, ends_at_ms, reason, created_at_ms)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(format!("{}:{}", request.action_id, index))
        .bind(item_id)
        .bind(request.scope.as_ref())
        .bind(&request.playlist_id)
        .bind(request.starts_at_ms)
        .bind(request.ends_at_ms)
        .bind(&request.reason)
        .bind(request.created_at_ms)
        .execute(&mut *transaction)
        .await
        .map_err(|error| MusicLibraryError::database("bulk save snooze", error))?;
        super::search::refresh_item(&mut transaction, item_id).await?;
    }
    super::writes::commit(transaction, "commit bulk snooze").await?;
    Ok(MusicBulkMembershipResult {
        changed_count: request.item_ids.len() as i64,
    })
}
