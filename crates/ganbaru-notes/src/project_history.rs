mod bundles;
mod checkpoint;
pub mod commands;
mod contracts;
mod dirty;
pub mod reads;
mod restore;
pub mod retention;
pub mod schedule;
mod scope;
pub(crate) use dirty::resolve_project_id_for_page_tx;

use checkpoint::create_checkpoint;
#[cfg(test)]
use checkpoint::create_checkpoint_after_graph_load;
#[allow(unused_imports)]
pub use commands::{notes_preview_project_history_restore, notes_restore_project_history_version};
use contracts::ProjectHistoryManifest;
pub use contracts::{
    NotesHistoricalPageDto, NotesHistoricalPageSummaryDto, NotesHistoryRetentionImpactDto,
    NotesMutationResultDto, NotesProjectHistoryRestorePlanDto, NotesProjectHistoryScheduleDto,
    NotesProjectHistoryTreeDto, NotesProjectHistoryVersionDto, NotesProjectHistoryVersionListDto,
};
pub use dirty::{
    create_safety_checkpoint_for_page, ensure_blocks_baseline_for_mutation,
    ensure_data_source_baseline_for_mutation, ensure_page_baseline_for_mutation,
    ensure_parent_baseline_for_mutation, ensure_project_baseline_for_mutation,
    mark_data_source_dirty_tx, mark_page_dirty_tx, mark_project_dirty_tx, page_history_enabled_tx,
};
use reads::{json_optional_string, load_manifest_rows_tx, load_manifest_tx, version_from_row};
#[allow(unused_imports)]
pub use reads::{
    notes_list_project_history_versions, notes_load_project_history_page,
    notes_load_project_history_tree,
};
pub use retention::prune_project_history_tx;
use retention::{effective_retention_days_tx, run_due_maintenance, validate_retention_days};
#[allow(unused_imports)]
pub use retention::{notes_history_retention_impact, notes_prune_project_history};
pub use schedule::mutation_result;
#[cfg(test)]
use schedule::{
    checkpoint_is_due, flush_due_checkpoints, history_schedule, initialize_project_history,
};
#[allow(unused_imports)]
pub use schedule::{notes_flush_due_project_history, notes_initialize_project_history};

use crate::{NoteParent, local_user, writes};
use bundles::{garbage_collect_bundles_tx, load_bundle_tx, store_bundle_tx};
use scope::{ProjectHistoryGraph, load_project_graph};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{Row, Sqlite, SqlitePool, Transaction};
use std::collections::{BTreeMap, HashSet};

const DEFAULT_RETENTION_DAYS: i64 = 30;
const HISTORY_SCHEMA_VERSION: i64 = 1;
const ACTIVE_CHECKPOINT_MINUTES: i64 = 10;
const IDLE_CHECKPOINT_MINUTES: i64 = 2;
const DEFAULT_PAGE_SIZE: i64 = 40;
const MAX_PAGE_SIZE: i64 = 100;
const MAINTENANCE_INTERVAL_HOURS: i64 = 6;
const SUPPORTED_RETENTION_DAYS: [i64; 6] = [0, 7, 30, 90, 180, 365];

pub async fn store_page_history_blocks_tx(
    tx: &mut Transaction<'_, Sqlite>,
    blocks_json: &str,
) -> Result<String, String> {
    store_bundle_tx(tx, "row", blocks_json.as_bytes()).await
}

pub async fn load_page_history_blocks_tx(
    tx: &mut Transaction<'_, Sqlite>,
    hash: &str,
) -> Result<String, String> {
    let raw = load_bundle_tx(tx, hash).await?;
    String::from_utf8(raw).map_err(|_| "Notes page history block bundle is not UTF-8".to_string())
}

pub fn page_history_blocks_hash(blocks_json: &str) -> String {
    bundles::sha256_hex(blocks_json.as_bytes())
}

pub async fn garbage_collect_history_storage_tx(
    tx: &mut Transaction<'_, Sqlite>,
) -> Result<(), String> {
    garbage_collect_bundles_tx(tx).await
}

fn validate_project_id(project_id: &str) -> Result<String, String> {
    let project_id = project_id.trim();
    if project_id.is_empty() || project_id.len() > 120 {
        return Err("project_id is invalid".to_string());
    }
    Ok(project_id.to_string())
}

#[cfg(test)]
mod tests;
