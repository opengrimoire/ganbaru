//! Recovery offers of the active vault. Listing reads the engine directly; restoring and
//! discarding seal with the local writer, so the service runs them inside a pass.

use std::sync::LazyLock;

use ganbaru_sync::manifest::vault::{music, projects, quick_notes, themes};
use ganbaru_sync::{DeviceRef, Engine, local};
pub(crate) use ganbaru_sync_replica::recovery::{
    RecoveredRow, RecoveryAction, RecoveryChoice, RecoveryRestorers, RestoreFuture, close,
};
use serde::Serialize;
use sqlx::SqliteConnection;
use tauri::{AppHandle, Runtime};
use tokio::sync::oneshot;

/// Restorers of the domains whose rows can root a recovery offer, one line per table.
static RESTORERS: LazyLock<RecoveryRestorers> = LazyLock::new(|| {
    RecoveryRestorers::new()
        .with(&quick_notes::NOTES, restore_quick_note)
        .with(&themes::THEMES, restore_theme)
        .with(&music::PLAYLISTS, restore_music_playlist)
        .with(&music::ITEMS, refuse_music_item)
        .with(&projects::GROUPS, refuse_projects_row)
        .with(&projects::PROJECTS, refuse_projects_row)
        .with(&projects::SECTIONS, refuse_projects_row)
        .with(&projects::STATUSES, refuse_projects_row)
        .with(&projects::TASKS, refuse_projects_row)
});

/// The recovery restorers of the vault domains.
pub(crate) fn restorers() -> &'static RecoveryRestorers {
    &RESTORERS
}

/// Restores a deleted Quick note as a new note. Notes have no cascade descendants, so a
/// recovered note that carries some was grouped by a manifest this version does not know.
fn restore_quick_note(conn: &mut SqliteConnection, row: RecoveredRow) -> RestoreFuture<'_> {
    Box::pin(async move {
        if !row.descendants.is_empty() {
            return Err("a recovered quick note has deleted descendants".to_string());
        }
        ganbaru_quick_notes::restore_recovered(conn, &row.values).await
    })
}

/// Restores a deleted theme as a new theme. Its token and palette rows are owned by the theme
/// row rather than cascade descendants, so a recovered theme that carries some was grouped by
/// a manifest this version does not know.
fn restore_theme(conn: &mut SqliteConnection, row: RecoveredRow) -> RestoreFuture<'_> {
    Box::pin(async move {
        if !row.descendants.is_empty() {
            return Err("a recovered theme has deleted descendants".to_string());
        }
        ganbaru_themes::restore_recovered(conn, &row.values).await
    })
}

/// Restores a deleted playlist as a new playlist with copies of its deleted intended uses and
/// memberships.
fn restore_music_playlist(conn: &mut SqliteConnection, row: RecoveredRow) -> RestoreFuture<'_> {
    Box::pin(async move {
        let now_ms = crate::vault::now_utc().timestamp_millis();
        ganbaru_music_library::recovery::restore_recovered_playlist(
            conn,
            &row.values,
            &row.descendants,
            now_ms,
        )
        .await
        .map_err(|error| error.to_string())
    })
}

/// Refuses to restore a deleted library item. An item's key is derived from its source, so it
/// comes back by adding that source again; its offer, which a concurrent membership edit opens,
/// can only be discarded.
fn refuse_music_item(_: &mut SqliteConnection, _: RecoveredRow) -> RestoreFuture<'_> {
    Box::pin(async {
        Err("a deleted Music library item comes back by adding its source again".to_string())
    })
}

/// Refuses to restore a deleted Projects row, which keeps its offer discardable. Restoring a
/// project with its retained subtree belongs to the Projects recovery presentation.
fn refuse_projects_row(_: &mut SqliteConnection, _: RecoveredRow) -> RestoreFuture<'_> {
    Box::pin(async {
        Err("deleted Projects rows cannot be restored from the recovery list".to_string())
    })
}

/// A deleted row with edits the deletion did not see.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RecoveryEntryView {
    /// Domain table, by name.
    pub table: String,
    pub row_key: String,
    pub title: String,
    pub preview: String,
    /// Deleted descendants that restoring the row brings back with it.
    pub descendants: usize,
    /// Device of the newest unseen edit.
    pub device: DeviceRef,
    /// When that edit was made, in Unix milliseconds.
    pub edited_at_ms: u64,
}

/// A recovery action waiting for the service, which owns the local writer.
pub(crate) struct RecoveryRequest {
    pub choice: RecoveryChoice,
    /// Receives the id of the restored row, if any.
    pub reply: oneshot::Sender<Result<Option<String>, String>>,
}

/// Recovery offers of the active vault, newest edit first.
pub(crate) async fn list<R: Runtime>(app: &AppHandle<R>) -> Result<Vec<RecoveryEntryView>, String> {
    let Some(pool) = crate::db::connect_active_vault_for_sync(app).await? else {
        return Ok(Vec::new());
    };
    let vault_id = crate::vault::active_vault_id(app)?;
    let engine = Engine::vault();
    let mut conn = pool
        .acquire()
        .await
        .map_err(|error| format!("acquire sync connection: {error}"))?;
    let coordinator = super::service::coordinator_device_id(app, &vault_id)?;
    let Some(ctx) = local::space_context(&mut conn, &vault_id, &coordinator)
        .await
        .map_err(|error| format!("read sync space: {error}"))?
    else {
        return Ok(Vec::new());
    };
    let entries = engine
        .recovery_entries(&mut conn, &ctx)
        .await
        .map_err(|error| format!("read recovery offers: {error}"))?;
    drop(conn);
    let devices = super::devices::read(app)?;
    let mut views: Vec<RecoveryEntryView> = entries
        .into_iter()
        .filter_map(|entry| {
            let table = engine.manifest().table(entry.table)?;
            Some(RecoveryEntryView {
                table: table.name.to_string(),
                row_key: entry.row_key,
                title: entry.presentation.title,
                preview: entry.presentation.preview,
                descendants: entry.descendants,
                device: devices.describe(&entry.device_id),
                edited_at_ms: entry.clock.physical_ms(),
            })
        })
        .collect();
    views.sort_by(|left, right| {
        right
            .edited_at_ms
            .cmp(&left.edited_at_ms)
            .then_with(|| left.row_key.cmp(&right.row_key))
    });
    Ok(views)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_recovery_restorers_cover_every_table_that_can_root_an_offer() {
        let mut roots: Vec<&str> = Engine::vault()
            .manifest()
            .recovery_root_tables()
            .into_iter()
            .map(|table| table.name)
            .collect();
        roots.sort_unstable();
        assert_eq!(restorers().tables().collect::<Vec<_>>(), roots);
    }
}
