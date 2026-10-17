//! Bounded canonical preimages and sibling anchors for selective editor undo.

use super::{block_tree, compound::NoteEditPlacement};
use crate::models::{NoteBlockDto, NoteBlockRow, NoteParent};
use sqlx::{Sqlite, Transaction};
use std::collections::{BTreeMap, BTreeSet};

/// Block preimages, each captured once before the row's first change in the transaction.
#[derive(Default)]
pub(super) struct Preimages {
    rows: BTreeMap<String, (NoteBlockRow, Option<NoteEditPlacement>)>,
    bytes: usize,
}

impl Preimages {
    pub(super) fn bytes(&self) -> usize {
        self.bytes
    }

    /// Account for typed database results without allocating an intermediate JSON buffer.
    pub(super) fn charge_result(&mut self, result: &impl serde::Serialize) -> Result<(), String> {
        struct ByteCounter(usize);
        impl std::io::Write for ByteCounter {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.0 = self.0.saturating_add(bytes.len());
                if self.0 > super::compound::MAX_RESULT_BYTES {
                    return Err(std::io::Error::other(
                        "Notes edit receipt exceeds its byte limit",
                    ));
                }
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let mut counter = ByteCounter(self.bytes);
        serde_json::to_writer(&mut counter, result)
            .map_err(|error| format!("bound Notes database result: {error}"))?;
        self.bytes = counter.0;
        Ok(())
    }

    pub(super) async fn capture(
        &mut self,
        tx: &mut Transaction<'_, Sqlite>,
        ids: &[String],
        created: &BTreeSet<String>,
    ) -> Result<(), String> {
        for id in ids {
            if created.contains(id) || self.rows.contains_key(id) {
                continue;
            }
            if self.rows.len() >= super::compound::MAX_GRAPH_BLOCKS as usize {
                return Err("Notes edit preimage exceeds its block limit".to_string());
            }
            self.bytes = charge_row(tx, id, self.bytes).await?;
            let row = block_tree::load_block_row_tx(tx, id, true).await?;
            let placement = placement(tx, &row).await?;
            self.rows.insert(id.clone(), (row, placement));
        }
        Ok(())
    }

    pub(super) fn into_parts(self) -> Result<(Vec<NoteBlockDto>, Vec<NoteEditPlacement>), String> {
        let mut rows = self.rows.into_values().collect::<Vec<_>>();
        rows.sort_by(|(left, _), (right, _)| {
            (&left.parent_page_id, &left.parent_block_id)
                .cmp(&(&right.parent_page_id, &right.parent_block_id))
                .then(left.sort_order.total_cmp(&right.sort_order))
                .then(left.id.cmp(&right.id))
        });
        let mut blocks = Vec::with_capacity(rows.len());
        let mut placements = Vec::new();
        for (row, placement) in rows {
            blocks.push(NoteBlockDto::new(row)?);
            placements.extend(placement);
        }
        Ok((blocks, placements))
    }
}

/// Charge canonical string storage before loading or parsing a row into memory.
pub(super) async fn charge_row(
    tx: &mut Transaction<'_, Sqlite>,
    id: &str,
    bytes: usize,
) -> Result<usize, String> {
    let row_bytes: i64 = sqlx::query_scalar("SELECT length(CAST(payload AS BLOB)) + length(CAST(plain_text AS BLOB)) + 1024 FROM notes_blocks WHERE id = ?")
        .bind(id).fetch_one(&mut **tx).await.map_err(|error| format!("bound Notes edit receipt: {error}"))?;
    let next = bytes.saturating_add(row_bytes as usize);
    if next > super::compound::MAX_RESULT_BYTES {
        return Err("Notes edit receipt exceeds its byte limit".to_string());
    }
    Ok(next)
}

/// Return stable sibling placement for a canonical active row.
pub(super) async fn placement(
    tx: &mut Transaction<'_, Sqlite>,
    row: &NoteBlockRow,
) -> Result<Option<NoteEditPlacement>, String> {
    if row.in_trash != 0 {
        return Ok(None);
    }
    let after: Option<String> = sqlx::query_scalar("SELECT id FROM notes_blocks WHERE parent_page_id IS ? AND parent_block_id IS ? AND in_trash = 0 AND (sort_order < ? OR (sort_order = ? AND id < ?)) ORDER BY sort_order DESC, id DESC LIMIT 1")
        .bind(&row.parent_page_id).bind(&row.parent_block_id).bind(row.sort_order).bind(row.sort_order).bind(&row.id)
        .fetch_optional(&mut **tx).await.map_err(|error| format!("read Notes edit placement: {error}"))?;
    let before = if after.is_none() {
        sqlx::query_scalar("SELECT id FROM notes_blocks WHERE parent_page_id IS ? AND parent_block_id IS ? AND in_trash = 0 AND (sort_order > ? OR (sort_order = ? AND id > ?)) ORDER BY sort_order, id LIMIT 1")
            .bind(&row.parent_page_id).bind(&row.parent_block_id).bind(row.sort_order).bind(row.sort_order).bind(&row.id)
            .fetch_optional(&mut **tx).await.map_err(|error| format!("read Notes first placement: {error}"))?
    } else {
        None
    };
    let parent = if let Some(id) = &row.parent_block_id {
        NoteParent::BlockId {
            block_id: id.clone(),
        }
    } else {
        NoteParent::PageId {
            page_id: row.page_id.clone(),
        }
    };
    Ok(Some(NoteEditPlacement {
        block_id: row.id.clone(),
        parent,
        after,
        before,
    }))
}
