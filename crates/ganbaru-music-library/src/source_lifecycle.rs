use sqlx::{SqliteConnection, SqlitePool, Transaction};

use super::*;

pub async fn removal_impact(
    pool: &SqlitePool,
    collection_id: &str,
) -> MusicLibraryResult<MusicSourceRemovalImpact> {
    if collection_id.trim().is_empty() {
        return Err(MusicLibraryError::validation("collectionId", "is required"));
    }
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM music_source_collections WHERE id = ?)")
            .bind(collection_id)
            .fetch_one(pool)
            .await
            .map_err(|error| MusicLibraryError::database("load source removal impact", error))?;
    if !exists {
        return Err(MusicLibraryError::not_found(
            "music source collection",
            collection_id,
        ));
    }
    let mut connection = pool
        .acquire()
        .await
        .map_err(|error| MusicLibraryError::database("open source impact query", error))?;
    load_impact(&mut connection, collection_id).await
}

async fn load_impact(
    executor: &mut SqliteConnection,
    collection_id: &str,
) -> MusicLibraryResult<MusicSourceRemovalImpact> {
    let row: (i64, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT
            COUNT(DISTINCT source_item.item_id) AS item_count,
            COUNT(DISTINCT membership.id) AS membership_count,
            COUNT(DISTINCT CASE WHEN EXISTS (
                SELECT 1 FROM music_source_collection_items AS other
                WHERE other.item_id = source_item.item_id AND other.collection_id <> ?
            ) THEN source_item.item_id END) AS shared_item_count,
            COUNT(DISTINCT CASE WHEN NOT EXISTS (
                SELECT 1 FROM music_source_collection_items AS other
                WHERE other.item_id = source_item.item_id AND other.collection_id <> ?
            ) AND membership.id IS NULL THEN source_item.item_id END) AS orphaned_item_count,
            (SELECT COUNT(*) FROM music_refresh_jobs
             WHERE source_collection_id = ? AND state IN ('queued', 'running')) AS active_refresh_count
         FROM music_source_collection_items AS source_item
         LEFT JOIN music_playlist_memberships AS membership ON membership.item_id = source_item.item_id
         WHERE source_item.collection_id = ?",
    )
    .bind(collection_id)
    .bind(collection_id)
    .bind(collection_id)
    .bind(collection_id)
    .fetch_one(executor)
    .await
    .map_err(|error| MusicLibraryError::database("count source removal impact", error))?;
    Ok(MusicSourceRemovalImpact {
        collection_id: collection_id.to_string(),
        item_count: row.0,
        membership_count: row.1,
        shared_item_count: row.2,
        orphaned_item_count: row.3,
        active_refresh_count: row.4,
    })
}

pub async fn remove_source(
    pool: &SqlitePool,
    request: MusicSourceRemovalRequest,
) -> MusicLibraryResult<MusicSourceRemovalImpact> {
    validate_request(&request)?;
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| MusicLibraryError::database("begin source removal", error))?;
    let version: Option<i64> =
        sqlx::query_scalar("SELECT version FROM music_source_collections WHERE id = ?")
            .bind(&request.collection_id)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(|error| MusicLibraryError::database("load source version", error))?;
    let Some(version) = version else {
        return Err(MusicLibraryError::not_found(
            "music source collection",
            &request.collection_id,
        ));
    };
    if version != request.expected_version {
        return Err(MusicLibraryError::stale(
            "music source collection",
            &request.collection_id,
        ));
    }
    let impact = load_impact(&mut transaction, &request.collection_id).await?;
    if impact != request.expected_impact {
        return Err(MusicLibraryError::stale(
            "music source removal impact",
            &request.collection_id,
        ));
    }
    cancel_active_refreshes(&mut transaction, &request).await?;
    let result = sqlx::query(
        "UPDATE music_source_collections
         SET discovery_enabled = 0, removed_at_ms = ?, refresh_state = 'idle',
             last_refresh_error_code = NULL, updated_at_ms = ?, version = version + 1
         WHERE id = ? AND version = ?",
    )
    .bind(request.removed_at_ms)
    .bind(request.removed_at_ms)
    .bind(&request.collection_id)
    .bind(request.expected_version)
    .execute(&mut *transaction)
    .await
    .map_err(|error| MusicLibraryError::database("disable music source", error))?;
    if result.rows_affected() != 1 {
        return Err(MusicLibraryError::stale(
            "music source collection",
            &request.collection_id,
        ));
    }
    if request.remove_orphaned_items {
        prune_orphaned_items(&mut transaction, &request.collection_id).await?;
    }
    transaction
        .commit()
        .await
        .map_err(|error| MusicLibraryError::database("commit source removal", error))?;
    Ok(impact)
}

async fn cancel_active_refreshes(
    transaction: &mut Transaction<'_, sqlx::Sqlite>,
    request: &MusicSourceRemovalRequest,
) -> MusicLibraryResult<()> {
    sqlx::query(
        "UPDATE music_refresh_jobs SET state = 'cancelled',
             status_message = 'Source discovery was stopped. Existing catalog data was preserved.',
             finished_at_ms = ?, updated_at_ms = ?
         WHERE source_collection_id = ? AND state IN ('queued', 'running')",
    )
    .bind(request.removed_at_ms)
    .bind(request.removed_at_ms)
    .bind(&request.collection_id)
    .execute(&mut **transaction)
    .await
    .map_err(|error| MusicLibraryError::database("cancel removed source refreshes", error))?;
    Ok(())
}

async fn prune_orphaned_items(
    transaction: &mut Transaction<'_, sqlx::Sqlite>,
    collection_id: &str,
) -> MusicLibraryResult<()> {
    sqlx::query(
        "DELETE FROM music_search_fts WHERE item_id IN (
            SELECT source_item.item_id FROM music_source_collection_items AS source_item
            WHERE source_item.collection_id = ?
              AND NOT EXISTS (
                  SELECT 1 FROM music_source_collection_items AS other
                  WHERE other.item_id = source_item.item_id AND other.collection_id <> ?
              )
              AND NOT EXISTS (
                  SELECT 1 FROM music_playlist_memberships AS membership
                  WHERE membership.item_id = source_item.item_id
              )
         )",
    )
    .bind(collection_id)
    .bind(collection_id)
    .execute(&mut **transaction)
    .await
    .map_err(|error| MusicLibraryError::database("remove orphaned music search rows", error))?;
    sqlx::query(
        "DELETE FROM music_library_items WHERE id IN (
            SELECT source_item.item_id FROM music_source_collection_items AS source_item
            WHERE source_item.collection_id = ?
              AND NOT EXISTS (
                  SELECT 1 FROM music_source_collection_items AS other
                  WHERE other.item_id = source_item.item_id AND other.collection_id <> ?
              )
              AND NOT EXISTS (
                  SELECT 1 FROM music_playlist_memberships AS membership
                  WHERE membership.item_id = source_item.item_id
              )
         )",
    )
    .bind(collection_id)
    .bind(collection_id)
    .execute(&mut **transaction)
    .await
    .map_err(|error| MusicLibraryError::database("remove orphaned catalog items", error))?;
    Ok(())
}

fn validate_request(request: &MusicSourceRemovalRequest) -> MusicLibraryResult<()> {
    if request.collection_id.trim().is_empty()
        || request.expected_version <= 0
        || request.removed_at_ms <= 0
    {
        return Err(MusicLibraryError::validation(
            "collectionId",
            "a collection id, expected version, and removed timestamp are required",
        ));
    }
    if request.expected_impact.collection_id != request.collection_id {
        return Err(MusicLibraryError::validation(
            "expectedImpact",
            "must describe the selected source collection",
        ));
    }
    Ok(())
}
