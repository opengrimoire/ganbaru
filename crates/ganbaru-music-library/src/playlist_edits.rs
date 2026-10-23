use super::*;
use sqlx::SqlitePool;
use std::collections::{HashMap, HashSet};

/// Appends a default membership of `item_id` after `tail` unless the playlist already holds the
/// item, advancing `tail` when a row is inserted. Returns whether a row was inserted.
async fn append_membership(
    connection: &mut sqlx::SqliteConnection,
    playlist_id: &str,
    item_id: &str,
    tail: &mut Option<String>,
    updated_at_ms: i64,
) -> MusicLibraryResult<bool> {
    let order_key = super::order::key_after(tail.as_deref())?;
    let result = sqlx::query(
        "INSERT INTO music_playlist_memberships
            (id, playlist_id, item_id, order_key, weight, enabled,
             created_at_ms, updated_at_ms, version)
         VALUES (?, ?, ?, ?, 'normal', 1, ?, ?, 1)
         ON CONFLICT(playlist_id, item_id) DO NOTHING",
    )
    .bind(super::identity::membership_id(playlist_id, item_id)?)
    .bind(playlist_id)
    .bind(item_id)
    .bind(order_key.as_str())
    .bind(updated_at_ms)
    .bind(updated_at_ms)
    .execute(connection)
    .await
    .map_err(|error| MusicLibraryError::database("add playlist membership", error))?;
    let inserted = result.rows_affected() > 0;
    if inserted {
        *tail = Some(order_key.as_str().to_string());
    }
    Ok(inserted)
}

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
        let mut tail = super::order::membership_tail(&mut transaction, playlist_id).await?;
        for item_id in &request.item_ids {
            let inserted = append_membership(
                &mut transaction,
                playlist_id,
                item_id,
                &mut tail,
                request.updated_at_ms,
            )
            .await?;
            if inserted {
                changed_count += 1;
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
                     WHERE playlist_id = ? AND item_id = ? AND weight IS NOT ?",
                )
                .bind(weight.as_ref())
                .bind(request.updated_at_ms)
                .bind(playlist_id)
                .bind(item_id)
                .bind(weight.as_ref())
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
    let mut item_ids: Vec<String> = sqlx::query_scalar(
        "SELECT item_id FROM music_playlist_memberships
         WHERE playlist_id = ? ORDER BY order_key, id",
    )
    .bind(&request.playlist_id)
    .fetch_all(&mut *transaction)
    .await
    .map_err(|error| MusicLibraryError::database("load playlist order", error))?;
    let Some(source_index) = item_ids
        .iter()
        .position(|item_id| item_id == &request.item_id)
    else {
        return Err(MusicLibraryError::not_found(
            "playlist membership item",
            &request.item_id,
        ));
    };
    if request.target_index as usize >= item_ids.len() {
        return Err(MusicLibraryError::validation(
            "targetIndex",
            format!("must be less than {}", item_ids.len()),
        ));
    }
    let moved = item_ids.remove(source_index);
    item_ids.insert(request.target_index as usize, moved);
    let desired = item_ids.iter().map(String::as_str).collect::<Vec<_>>();
    super::order::order_memberships(
        &mut transaction,
        &request.playlist_id,
        &desired,
        request.updated_at_ms,
    )
    .await?;
    super::writes::commit(transaction, "commit playlist reorder").await?;
    Ok(MusicPlaylistReorderResult { item_ids })
}

pub async fn reorder_playlists(
    pool: &SqlitePool,
    request: MusicPlaylistsReorder,
) -> MusicLibraryResult<Vec<MusicOrderReceipt>> {
    validate_playlists_reorder(&request)?;
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| MusicLibraryError::database("begin playlists reorder", error))?;
    let existing: Vec<(String, String, i64)> =
        sqlx::query_as("SELECT id, order_key, version FROM music_playlists ORDER BY order_key, id")
            .fetch_all(&mut *transaction)
            .await
            .map_err(|error| MusicLibraryError::database("load playlists for reorder", error))?;
    let mut stored_ids = existing
        .iter()
        .map(|(id, _, _)| id.as_str())
        .collect::<Vec<_>>();
    stored_ids.sort_unstable();
    let mut requested = request
        .playlists
        .iter()
        .map(|playlist| playlist.playlist_id.as_str())
        .collect::<Vec<_>>();
    requested.sort_unstable();
    if stored_ids != requested {
        return Err(MusicLibraryError::conflict(
            "the playlist collection changed while it was being reordered",
        ));
    }
    let versions = existing
        .iter()
        .map(|(id, _, version)| (id.as_str(), *version))
        .collect::<HashMap<_, _>>();
    if let Some(playlist) = request.playlists.iter().find(|playlist| {
        versions.get(playlist.playlist_id.as_str()) != Some(&playlist.expected_version)
    }) {
        return Err(MusicLibraryError::conflict(format!(
            "playlist {} changed while it was being reordered",
            playlist.playlist_id
        )));
    }

    let current = existing
        .iter()
        .map(|(id, order_key, _)| (id.clone(), order_key.clone()))
        .collect::<Vec<_>>();
    let desired = request
        .playlists
        .iter()
        .map(|playlist| playlist.playlist_id.as_str())
        .collect::<Vec<_>>();
    let moved = super::order::reorder(&current, &desired)?;
    for (playlist_id, order_key) in &moved {
        sqlx::query(
            "UPDATE music_playlists
             SET order_key = ?, updated_at_ms = ?, version = version + 1 WHERE id = ?",
        )
        .bind(order_key.as_str())
        .bind(request.updated_at_ms)
        .bind(playlist_id)
        .execute(&mut *transaction)
        .await
        .map_err(|error| MusicLibraryError::database("persist playlist order", error))?;
    }
    let moved_keys = moved
        .iter()
        .map(|(playlist_id, order_key)| (playlist_id.as_str(), order_key.as_str()))
        .collect::<HashMap<_, _>>();
    let current_keys = current
        .iter()
        .map(|(playlist_id, order_key)| (playlist_id.as_str(), order_key.as_str()))
        .collect::<HashMap<_, _>>();
    let receipts = request
        .playlists
        .iter()
        .map(|playlist| {
            let id = playlist.playlist_id.as_str();
            let moved_key = moved_keys.get(id);
            MusicOrderReceipt {
                id: playlist.playlist_id.clone(),
                order_key: moved_key
                    .or_else(|| current_keys.get(id))
                    .map_or_else(String::new, |key| (*key).to_string()),
                version: playlist.expected_version + i64::from(moved_key.is_some()),
            }
        })
        .collect();
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
        let mut tail = super::order::membership_tail(&mut transaction, playlist_id).await?;
        for item in &request.items {
            let inserted = append_membership(
                &mut transaction,
                playlist_id,
                &item.item_id,
                &mut tail,
                request.updated_at_ms,
            )
            .await?;
            if inserted {
                membership_changed_count += 1;
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
