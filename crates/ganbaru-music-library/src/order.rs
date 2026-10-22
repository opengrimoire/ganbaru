//! Order keys for playlists, memberships, and preset sounds over the shared placement in
//! `ganbaru-sync-contracts`. Rows sort by `(order_key, id)` compared byte-wise.

use super::{MusicLibraryError, MusicLibraryResult};
use ganbaru_sync_contracts::order_placement::{self, OrderPlacementError};
use ganbaru_sync_contracts::{OrderKey, OrderKeyError};
use sqlx::SqliteConnection;

fn key_error(error: OrderKeyError) -> MusicLibraryError {
    MusicLibraryError::runtime("generate music order key", error)
}

fn placement_error(error: OrderPlacementError) -> MusicLibraryError {
    match error {
        OrderPlacementError::RowMismatch | OrderPlacementError::IndexOutOfRange => {
            MusicLibraryError::conflict(error.to_string())
        }
        _ => MusicLibraryError::runtime("place music order key", error),
    }
}

/// Key that sorts after `last`, or the first key of an empty group.
pub(crate) fn key_after(last: Option<&str>) -> MusicLibraryResult<OrderKey> {
    let last = last.map(OrderKey::parse).transpose().map_err(key_error)?;
    OrderKey::between(last.as_ref(), None).map_err(key_error)
}

/// Keys for `count` rows appended after `last`, in order.
pub(crate) fn keys_after(last: Option<&str>, count: usize) -> MusicLibraryResult<Vec<OrderKey>> {
    let mut keys = Vec::with_capacity(count);
    let mut current = last.map(OrderKey::parse).transpose().map_err(key_error)?;
    for _ in 0..count {
        let key = OrderKey::between(current.as_ref(), None).map_err(key_error)?;
        keys.push(key.clone());
        current = Some(key);
    }
    Ok(keys)
}

/// Rank keys for `count` rows written as a new group, in order.
pub(crate) fn rank_keys(count: usize) -> MusicLibraryResult<Vec<OrderKey>> {
    (0..count)
        .map(|position| {
            u32::try_from(position)
                .map_err(|_| key_error(OrderKeyError::Exhausted))
                .and_then(|position| OrderKey::rank(position).map_err(key_error))
        })
        .collect()
}

/// Keys to write so the group sorts as `desired`; `current` is `(id, order_key)` sorted by
/// `(order_key, id)`. Only rows whose key changes are returned.
pub(crate) fn reorder(
    current: &[(String, String)],
    desired: &[&str],
) -> MusicLibraryResult<Vec<(String, OrderKey)>> {
    order_placement::reorder(current, desired).map_err(placement_error)
}

/// Largest order key among the memberships of `playlist_id`.
pub(crate) async fn membership_tail(
    connection: &mut SqliteConnection,
    playlist_id: &str,
) -> MusicLibraryResult<Option<String>> {
    sqlx::query_scalar(
        "SELECT order_key FROM music_playlist_memberships WHERE playlist_id = ?
         ORDER BY order_key DESC, id DESC LIMIT 1",
    )
    .bind(playlist_id)
    .fetch_optional(connection)
    .await
    .map_err(|error| MusicLibraryError::database("load last playlist membership key", error))
}

/// Largest order key among playlists.
pub(crate) async fn playlist_tail(
    connection: &mut SqliteConnection,
) -> MusicLibraryResult<Option<String>> {
    sqlx::query_scalar(
        "SELECT order_key FROM music_playlists ORDER BY order_key DESC, id DESC LIMIT 1",
    )
    .fetch_optional(connection)
    .await
    .map_err(|error| MusicLibraryError::database("load last music playlist key", error))
}

/// Writes membership keys so `playlist_id` sorts as `desired_item_ids`, which lists each of its
/// membership items once. Only memberships whose key changes are written and versioned.
pub(crate) async fn order_memberships(
    connection: &mut SqliteConnection,
    playlist_id: &str,
    desired_item_ids: &[&str],
    updated_at_ms: i64,
) -> MusicLibraryResult<()> {
    let rows: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT id, item_id, order_key FROM music_playlist_memberships
         WHERE playlist_id = ? ORDER BY order_key, id",
    )
    .bind(playlist_id)
    .fetch_all(&mut *connection)
    .await
    .map_err(|error| MusicLibraryError::database("load playlist order", error))?;
    let membership_ids = rows
        .iter()
        .map(|(id, item_id, _)| (item_id.as_str(), id.as_str()))
        .collect::<std::collections::HashMap<_, _>>();
    let desired = desired_item_ids
        .iter()
        .map(|item_id| {
            membership_ids
                .get(item_id)
                .copied()
                .ok_or_else(|| MusicLibraryError::not_found("playlist membership item", item_id))
        })
        .collect::<MusicLibraryResult<Vec<_>>>()?;
    let current = rows
        .iter()
        .map(|(id, _, key)| (id.clone(), key.clone()))
        .collect::<Vec<_>>();
    for (membership_id, key) in reorder(&current, &desired)? {
        sqlx::query(
            "UPDATE music_playlist_memberships
             SET order_key = ?, updated_at_ms = ?, version = version + 1 WHERE id = ?",
        )
        .bind(key.as_str())
        .bind(updated_at_ms)
        .bind(&membership_id)
        .execute(&mut *connection)
        .await
        .map_err(|error| MusicLibraryError::database("persist playlist order", error))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appended_and_ranked_keys_ascend() {
        let appended = keys_after(Some("a5"), 3).unwrap();
        assert!(appended[0].as_str() > "a5");
        assert!(appended.windows(2).all(|pair| pair[0] < pair[1]));
        assert_eq!(key_after(None).unwrap(), OrderKey::first());
        let ranked = rank_keys(3).unwrap();
        assert!(ranked.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(key_after(Some("")).is_err());
    }
}
