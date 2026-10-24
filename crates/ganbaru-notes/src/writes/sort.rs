//! Order keys for Notes blocks.
//!
//! Siblings sort by `(order_key, id)`. The sibling group of a block is every block, live or
//! trashed, that shares its `parent_page_id` and `parent_block_id`, so a trashed block keeps its
//! place for a later restore. New keys are computed with the shared placement in
//! `ganbaru-sync-contracts`, which usually writes only the placed rows and re-keys the group
//! only when the key space between two neighbors is exhausted.

use super::parents::ParentTarget;
use crate::validation::require_uuid;
use ganbaru_sync_contracts::order_placement;
use std::collections::HashSet;

/// Prefix of the in-memory ids that stand for rows being placed. Real block ids are UUIDs and
/// never start with it.
const PLACED_ROW_PREFIX: &str = "~placed:";

/// Where new or moved blocks go among their siblings.
enum Anchor<'a> {
    /// Directly after this live sibling.
    After(&'a str),
    /// Directly before this live sibling.
    Before(&'a str),
    /// After every sibling.
    End,
}

/// Every block of the sibling group of `parent` as `(id, order_key)`, sorted by `(order_key, id)`.
async fn sibling_group(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    parent: &ParentTarget,
) -> Result<Vec<(String, String)>, String> {
    sqlx::query_as(
        "SELECT id, order_key
         FROM notes_blocks
         WHERE parent_page_id IS ? AND parent_block_id IS ?
         ORDER BY order_key ASC, id ASC",
    )
    .bind(&parent.parent_page_id)
    .bind(&parent.parent_block_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|e| format!("load notes block order: {e}"))
}

/// Requires `anchor_id` to be a live sibling in `parent` on the parent's page.
async fn require_live_anchor(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    parent: &ParentTarget,
    anchor_id: &str,
    field: &str,
) -> Result<(), String> {
    let exists: Option<i64> = sqlx::query_scalar(
        "SELECT 1
         FROM notes_blocks
         WHERE id = ?
           AND parent_page_id IS ?
           AND parent_block_id IS ?
           AND page_id = ?
           AND in_trash = 0",
    )
    .bind(anchor_id)
    .bind(&parent.parent_page_id)
    .bind(&parent.parent_block_id)
    .bind(&parent.page_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| format!("load notes {field} block: {e}"))?;
    exists
        .map(|_| ())
        .ok_or_else(|| format!("{field} block not found"))
}

/// Computes keys for `count` rows placed at `anchor` in `parent`, writes the sibling keys the
/// placement rewrites, and returns the new keys in placement order. Rows in `moving` are left out
/// of the group because their old keys are about to be replaced.
async fn place_rows(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    parent: &ParentTarget,
    anchor: Anchor<'_>,
    count: usize,
    moving: &[&str],
) -> Result<Vec<String>, String> {
    if count == 0 {
        return Ok(Vec::new());
    }
    let moving: HashSet<&str> = moving.iter().copied().collect();
    let mut group: Vec<(String, String)> = sibling_group(tx, parent)
        .await?
        .into_iter()
        .filter(|(id, _)| !moving.contains(id.as_str()))
        .collect();
    let original: Vec<(String, String)> = group.clone();
    let index = match anchor {
        Anchor::End => group.len(),
        Anchor::After(id) | Anchor::Before(id) => {
            let field = if matches!(anchor, Anchor::After(_)) {
                "after"
            } else {
                "before"
            };
            require_uuid(id, field)?;
            require_live_anchor(tx, parent, id, field).await?;
            let position = group
                .iter()
                .position(|(row_id, _)| row_id == id)
                .ok_or_else(|| format!("{field} block not found"))?;
            if field == "after" {
                position + 1
            } else {
                position
            }
        }
    };
    for offset in 0..count {
        let placed_id = format!("{PLACED_ROW_PREFIX}{offset}");
        let updates = order_placement::placement(&group, index + offset, &placed_id)
            .map_err(|e| format!("place notes block order key: {e}"))?;
        let (key, rewritten) = order_placement::take_key(updates, &placed_id)
            .map_err(|e| format!("place notes block order key: {e}"))?;
        for (row_id, row_key) in rewritten {
            if let Some(row) = group.iter_mut().find(|row| row.0 == row_id) {
                row.1 = row_key.into_string();
            }
        }
        group.insert(index + offset, (placed_id, key.into_string()));
    }
    for (row_id, row_key) in &group {
        if row_id.starts_with(PLACED_ROW_PREFIX) {
            continue;
        }
        let unchanged = original
            .iter()
            .any(|(original_id, original_key)| original_id == row_id && original_key == row_key);
        if !unchanged {
            sqlx::query("UPDATE notes_blocks SET order_key = ? WHERE id = ?")
                .bind(row_key)
                .bind(row_id)
                .execute(&mut **tx)
                .await
                .map_err(|e| format!("rewrite notes block order key: {e}"))?;
        }
    }
    Ok(group[index..index + count]
        .iter()
        .map(|(_, key)| key.clone())
        .collect())
}

fn anchor_after(after: Option<&str>) -> Anchor<'_> {
    match after.map(str::trim).filter(|value| !value.is_empty()) {
        Some(after) => Anchor::After(after),
        None => Anchor::End,
    }
}

/// Order keys for `count` new blocks placed directly after the live sibling `after`, or after
/// every sibling when `after` is absent.
pub async fn next_order_keys(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    parent: &ParentTarget,
    after: Option<&str>,
    count: usize,
) -> Result<Vec<String>, String> {
    place_rows(tx, parent, anchor_after(after), count, &[]).await
}

/// Order keys for `count` new blocks placed directly before the live sibling `before`.
pub(super) async fn order_keys_before(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    parent: &ParentTarget,
    before: &str,
    count: usize,
) -> Result<Vec<String>, String> {
    place_rows(tx, parent, Anchor::Before(before.trim()), count, &[]).await
}

/// Order keys for existing blocks `moving` placed, in order, after `after` or before `before`
/// (`before` wins when both are given), or after every sibling when neither is given.
pub(super) async fn moved_order_keys(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    parent: &ParentTarget,
    after: Option<&str>,
    before: Option<&str>,
    moving: &[&str],
) -> Result<Vec<String>, String> {
    let anchor = match before {
        Some(before) => Anchor::Before(before.trim()),
        None => anchor_after(after),
    };
    place_rows(tx, parent, anchor, moving.len(), moving).await
}
