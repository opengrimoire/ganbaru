//! Cumulative source-data admission for a complete Notes graph copy.

use sqlx::{Sqlite, Transaction};
use std::collections::HashSet;

pub(super) const MAX_COPY_OBJECTS: usize = 10_000;
pub(super) const MAX_COPY_DEPTH: i64 = 64;
const MAX_COPY_BYTES: i64 = 32 * 1024 * 1024;

/// Allocation and identity admission shared by nested graph-copy planners.
pub(crate) struct CopyContext<'a> {
    pub(crate) reserved_ids: &'a mut HashSet<String>,
    pub(crate) budget: &'a mut CopyBudget,
}

/// Source-data budget shared by every nested page, database, and block copied by one operation.
///
/// Charges source rows before their variable-size values are loaded or parsed.
/// Identity allocation has its own bound because schema properties also need IDs.
pub(crate) struct CopyBudget {
    remaining_records: i64,
    remaining_bytes: i64,
}

impl Default for CopyBudget {
    fn default() -> Self {
        Self {
            remaining_records: MAX_COPY_OBJECTS as i64,
            remaining_bytes: MAX_COPY_BYTES,
        }
    }
}

impl CopyBudget {
    /// Admit a source batch before materializing its payloads.
    pub(super) fn charge(&mut self, records: i64, bytes: i64) -> Result<(), String> {
        if records < 0 || records > self.remaining_records {
            return Err("Notes copy exceeds the source record limit".to_string());
        }
        if bytes < 0 || bytes > self.remaining_bytes {
            return Err("Notes copy exceeds the source byte limit".to_string());
        }
        self.remaining_records -= records;
        self.remaining_bytes -= bytes;
        Ok(())
    }

    /// Check page metadata independently of its block graph.
    pub(super) async fn page(
        &mut self,
        tx: &mut Transaction<'_, Sqlite>,
        id: &str,
    ) -> Result<(), String> {
        let bytes: i64 = sqlx::query_scalar(
            "SELECT length(CAST(title AS BLOB)) + length(CAST(properties AS BLOB))
                + coalesce(length(CAST(icon AS BLOB)), 0)
                + coalesce(length(CAST(cover AS BLOB)), 0)
                + coalesce(length(CAST(source_provider AS BLOB)), 0)
                + coalesce(length(CAST(source_object_id AS BLOB)), 0)
                + coalesce(length(CAST(source_workspace_id AS BLOB)), 0)
                + coalesce(length(CAST(source_last_edited_time AS BLOB)), 0)
                + coalesce(length(CAST(url AS BLOB)), 0)
                + coalesce(length(CAST(public_url AS BLOB)), 0)
             FROM notes_pages WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|error| format!("measure copied page metadata: {error}"))?
        .ok_or("notes copy source page not found")?;
        self.charge(1, bytes)
    }

    /// Check metadata later copied with INSERT SELECT or parsed during ID remapping.
    pub(super) async fn database(
        &mut self,
        tx: &mut Transaction<'_, Sqlite>,
        id: &str,
    ) -> Result<(), String> {
        let (records, bytes): (i64, i64) = sqlx::query_as(
            "SELECT count(*), coalesce(sum(bytes), 0) FROM (
                SELECT length(CAST(title AS BLOB)) + length(CAST(title_rich_text AS BLOB))
                    + length(CAST(description AS BLOB))
                    + coalesce(length(CAST(icon AS BLOB)), 0)
                    + coalesce(length(CAST(cover AS BLOB)), 0) AS bytes
                FROM notes_databases WHERE id = ?
                UNION ALL
                SELECT length(CAST(name AS BLOB)) + coalesce(length(CAST(filter AS BLOB)), 0)
                    + length(CAST(sorts AS BLOB)) + coalesce(length(CAST(configuration AS BLOB)), 0)
                FROM notes_database_views WHERE database_id = ? LIMIT ?
             )",
        )
        .bind(id)
        .bind(id)
        .bind(self.remaining_records + 1)
        .fetch_one(&mut **tx)
        .await
        .map_err(|error| format!("measure copied database metadata: {error}"))?;
        self.charge(records, bytes)
    }

    /// Check template metadata and all template blocks as one bounded batch.
    pub(super) async fn template(
        &mut self,
        tx: &mut Transaction<'_, Sqlite>,
        id: &str,
    ) -> Result<(), String> {
        let (records, bytes): (i64, i64) = sqlx::query_as(
            "SELECT count(*), coalesce(sum(bytes), 0) FROM (
                SELECT length(CAST(name AS BLOB)) + length(CAST(properties AS BLOB)) AS bytes
                FROM notes_data_source_templates WHERE id = ?
                UNION ALL
                SELECT length(CAST(payload AS BLOB)) + length(CAST(plain_text AS BLOB))
                FROM notes_data_source_template_blocks WHERE template_id = ? LIMIT ?
             )",
        )
        .bind(id)
        .bind(id)
        .bind(self.remaining_records + 1)
        .fetch_one(&mut **tx)
        .await
        .map_err(|error| format!("measure copied template metadata: {error}"))?;
        self.charge(records, bytes)
    }

    /// Include discussion metadata and attachment references copied with selected blocks.
    pub(super) async fn block_comments(
        &mut self,
        tx: &mut Transaction<'_, Sqlite>,
        encoded_block_ids: &str,
    ) -> Result<(), String> {
        let (records, bytes): (i64, i64) = sqlx::query_as(
            "WITH selected_threads AS (
                SELECT thread.id, coalesce(length(CAST(thread.resolved_by AS BLOB)), 0) AS bytes
                FROM notes_comment_threads AS thread
                JOIN json_each(?) AS selected ON thread.parent_block_id = selected.value
                WHERE thread.parent_type = 'block_id' LIMIT ?
             )
             SELECT count(*), coalesce(sum(bytes), 0) FROM (
                SELECT bytes FROM selected_threads
                UNION ALL
                SELECT length(CAST(comment.rich_text AS BLOB)) + length(CAST(comment.plain_text AS BLOB))
                    + length(CAST(comment.created_by AS BLOB)) + length(CAST(comment.display_name AS BLOB))
                    + length(CAST(comment.attachments AS BLOB))
                FROM notes_comments AS comment JOIN selected_threads ON comment.thread_id = selected_threads.id
                LIMIT ?
             )",
        )
        .bind(encoded_block_ids)
        .bind(self.remaining_records + 1)
        .bind(self.remaining_records + 1)
        .fetch_one(&mut **tx)
        .await
        .map_err(|error| format!("measure copied block comments: {error}"))?;
        self.charge(records, bytes)
    }
}
