use super::parents::ParentTarget;
use crate::validation::require_uuid;

pub(super) const DEFAULT_BLOCK_SORT_STEP: f64 = 1000.0;

pub async fn next_sort_orders(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    parent: &ParentTarget,
    after: Option<&str>,
    count: usize,
) -> Result<Vec<f64>, String> {
    if count == 0 {
        return Ok(Vec::new());
    }
    if let Some(after_id) = after.map(str::trim).filter(|value| !value.is_empty()) {
        require_uuid(after_id, "after")?;
        let after_row: Option<(f64,)> = if parent.parent_type == "page_id" {
            sqlx::query_as(
                "SELECT sort_order
                 FROM notes_blocks
                 WHERE id = ?
                   AND parent_type = 'page_id'
                   AND parent_page_id = ?
                   AND page_id = ?
                   AND in_trash = 0",
            )
            .bind(after_id)
            .bind(&parent.parent_page_id)
            .bind(&parent.page_id)
            .fetch_optional(&mut **tx)
            .await
        } else {
            sqlx::query_as(
                "SELECT sort_order
                 FROM notes_blocks
                 WHERE id = ?
                   AND parent_type = 'block_id'
                   AND parent_block_id = ?
                   AND page_id = ?
                   AND in_trash = 0",
            )
            .bind(after_id)
            .bind(&parent.parent_block_id)
            .bind(&parent.page_id)
            .fetch_optional(&mut **tx)
            .await
        }
        .map_err(|e| format!("load notes after block: {e}"))?;
        let after_order = after_row
            .map(|row| row.0)
            .ok_or_else(|| "after block not found".to_string())?;
        let next_order = next_sibling_order_after(tx, parent, after_order, after_id).await?;
        let step = match next_order {
            Some(next_order) if next_order > after_order => {
                (next_order - after_order) / (count as f64 + 1.0)
            }
            _ => DEFAULT_BLOCK_SORT_STEP,
        };
        return Ok((1..=count)
            .map(|index| after_order + step * index as f64)
            .collect());
    }
    let max_order: Option<f64> = if parent.parent_type == "page_id" {
        sqlx::query_scalar(
            "SELECT MAX(sort_order)
             FROM notes_blocks
             WHERE parent_type = 'page_id'
               AND parent_page_id = ?
               AND in_trash = 0",
        )
        .bind(&parent.parent_page_id)
        .fetch_one(&mut **tx)
        .await
    } else {
        sqlx::query_scalar(
            "SELECT MAX(sort_order)
             FROM notes_blocks
             WHERE parent_type = 'block_id'
               AND parent_block_id = ?
               AND in_trash = 0",
        )
        .bind(&parent.parent_block_id)
        .fetch_one(&mut **tx)
        .await
    }
    .map_err(|e| format!("load notes max sort order: {e}"))?;
    let base = max_order.unwrap_or(0.0);
    Ok((1..=count)
        .map(|index| base + DEFAULT_BLOCK_SORT_STEP * index as f64)
        .collect())
}

pub(super) async fn next_sibling_order_after(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    parent: &ParentTarget,
    after_order: f64,
    after_id: &str,
) -> Result<Option<f64>, String> {
    let next_order: Option<f64> = if parent.parent_type == "page_id" {
        sqlx::query_scalar(
            "SELECT sort_order
             FROM notes_blocks
             WHERE parent_type = 'page_id'
               AND parent_page_id = ?
               AND in_trash = 0
               AND (sort_order > ? OR (sort_order = ? AND id > ?))
             ORDER BY sort_order ASC, id ASC
             LIMIT 1",
        )
        .bind(&parent.parent_page_id)
        .bind(after_order)
        .bind(after_order)
        .bind(after_id)
        .fetch_optional(&mut **tx)
        .await
    } else {
        sqlx::query_scalar(
            "SELECT sort_order
             FROM notes_blocks
             WHERE parent_type = 'block_id'
               AND parent_block_id = ?
               AND in_trash = 0
               AND (sort_order > ? OR (sort_order = ? AND id > ?))
             ORDER BY sort_order ASC, id ASC
             LIMIT 1",
        )
        .bind(&parent.parent_block_id)
        .bind(after_order)
        .bind(after_order)
        .bind(after_id)
        .fetch_optional(&mut **tx)
        .await
    }
    .map_err(|e| format!("load next notes sibling: {e}"))?;
    Ok(next_order)
}

pub(super) async fn sort_order_before(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    parent: &ParentTarget,
    before_id: &str,
) -> Result<f64, String> {
    let before_id = before_id.trim();
    require_uuid(before_id, "before")?;
    let before_order: f64 = if parent.parent_type == "page_id" {
        sqlx::query_scalar(
            "SELECT sort_order
             FROM notes_blocks
             WHERE id = ?
               AND parent_type = 'page_id'
               AND parent_page_id = ?
               AND page_id = ?
               AND in_trash = 0",
        )
        .bind(before_id)
        .bind(&parent.parent_page_id)
        .bind(&parent.page_id)
        .fetch_optional(&mut **tx)
        .await
    } else {
        sqlx::query_scalar(
            "SELECT sort_order
             FROM notes_blocks
             WHERE id = ?
               AND parent_type = 'block_id'
               AND parent_block_id = ?
               AND page_id = ?
               AND in_trash = 0",
        )
        .bind(before_id)
        .bind(&parent.parent_block_id)
        .bind(&parent.page_id)
        .fetch_optional(&mut **tx)
        .await
    }
    .map_err(|e| format!("load notes before block: {e}"))?
    .ok_or_else(|| "before block not found".to_string())?;
    let previous_order = previous_sibling_order_before(tx, parent, before_order, before_id).await?;
    Ok(match previous_order {
        Some(previous_order) if previous_order < before_order => {
            previous_order + (before_order - previous_order) / 2.0
        }
        _ if before_order > 0.0 => before_order / 2.0,
        _ => 0.0,
    })
}

pub(super) async fn sort_orders_before(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    parent: &ParentTarget,
    before_id: &str,
    count: usize,
) -> Result<Vec<f64>, String> {
    if count == 0 {
        return Ok(Vec::new());
    }
    let before_id = before_id.trim();
    require_uuid(before_id, "before")?;
    let before_order: f64 = if parent.parent_type == "page_id" {
        sqlx::query_scalar(
            "SELECT sort_order
             FROM notes_blocks
             WHERE id = ?
               AND parent_type = 'page_id'
               AND parent_page_id = ?
               AND page_id = ?
               AND in_trash = 0",
        )
        .bind(before_id)
        .bind(&parent.parent_page_id)
        .bind(&parent.page_id)
        .fetch_optional(&mut **tx)
        .await
    } else {
        sqlx::query_scalar(
            "SELECT sort_order
             FROM notes_blocks
             WHERE id = ?
               AND parent_type = 'block_id'
               AND parent_block_id = ?
               AND page_id = ?
               AND in_trash = 0",
        )
        .bind(before_id)
        .bind(&parent.parent_block_id)
        .bind(&parent.page_id)
        .fetch_optional(&mut **tx)
        .await
    }
    .map_err(|e| format!("load notes before block: {e}"))?
    .ok_or_else(|| "before block not found".to_string())?;
    let previous_order = previous_sibling_order_before(tx, parent, before_order, before_id).await?;
    let (base, step) = match previous_order {
        Some(previous_order) if previous_order < before_order => (
            previous_order,
            (before_order - previous_order) / (count as f64 + 1.0),
        ),
        _ if before_order > 0.0 => (0.0, before_order / (count as f64 + 1.0)),
        _ => (0.0, 0.0),
    };
    Ok((1..=count)
        .map(|index| base + step * index as f64)
        .collect())
}

pub(super) async fn previous_sibling_order_before(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    parent: &ParentTarget,
    before_order: f64,
    before_id: &str,
) -> Result<Option<f64>, String> {
    let previous_order: Option<f64> = if parent.parent_type == "page_id" {
        sqlx::query_scalar(
            "SELECT sort_order
             FROM notes_blocks
             WHERE parent_type = 'page_id'
               AND parent_page_id = ?
               AND in_trash = 0
               AND (sort_order < ? OR (sort_order = ? AND id < ?))
             ORDER BY sort_order DESC, id DESC
             LIMIT 1",
        )
        .bind(&parent.parent_page_id)
        .bind(before_order)
        .bind(before_order)
        .bind(before_id)
        .fetch_optional(&mut **tx)
        .await
    } else {
        sqlx::query_scalar(
            "SELECT sort_order
             FROM notes_blocks
             WHERE parent_type = 'block_id'
               AND parent_block_id = ?
               AND in_trash = 0
               AND (sort_order < ? OR (sort_order = ? AND id < ?))
             ORDER BY sort_order DESC, id DESC
             LIMIT 1",
        )
        .bind(&parent.parent_block_id)
        .bind(before_order)
        .bind(before_order)
        .bind(before_id)
        .fetch_optional(&mut **tx)
        .await
    }
    .map_err(|e| format!("load previous notes sibling: {e}"))?;
    Ok(previous_order)
}
