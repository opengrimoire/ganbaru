//! Bounded process-local Calendar preimages, independent of execution history.

use serde::{Deserialize, Serialize};
use sqlx::{Sqlite, Transaction};
use std::sync::Arc;

use super::super::metadata::{Metadata, PreparedMutation, revision};
use super::super::occurrence::ReadBudget;
use super::super::scope::{SCOPE_GATE, SCOPE_WORKER_TIMEOUT};

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct UndoRequest {
    kind: UndoKind,
    pub(in crate::events) delete_command_id: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum UndoKind {
    UndoDelete,
}

impl UndoRequest {
    pub(in crate::events) fn check_limits(&self) -> Result<(), String> {
        super::super::commit::validate_command_id(&self.delete_command_id)
    }
}

#[derive(Serialize)]
pub(in crate::events) struct UndoRunReference {
    pub(in crate::events) id: String,
    pub(in crate::events) event_id: String,
    pub(in crate::events) current_identity: String,
    pub(in crate::events) previous_archive_id: Option<String>,
    pub(in crate::events) expected_archive_id: String,
}

#[derive(Serialize)]
pub(in crate::events) struct UndoSegmentReference {
    pub(in crate::events) id: String,
    pub(in crate::events) run_id: String,
    pub(in crate::events) event_id: String,
}

#[derive(Serialize)]
pub(in crate::events) struct UndoReferences {
    pub(in crate::events) runs: Vec<UndoRunReference>,
    pub(in crate::events) segments: Vec<UndoSegmentReference>,
}

/// This preimage is never serialized to disk or accepted from IPC. The existing
/// Focus owner retains one expiring slot and fences it to its vault generation.
pub struct UndoPreimage {
    before: Metadata,
    source_id: String,
    source_removed: bool,
    expected_source_revision: Option<String>,
    sealed: bool,
    references: UndoReferences,
    archives: Vec<String>,
    pub review_revision: String,
}

pub(in crate::events) struct PreparedUndo {
    rows: PreparedMutation,
    preimage: Arc<UndoPreimage>,
}

impl UndoPreimage {
    pub(in crate::events) fn capture(
        before: Metadata,
        source_id: String,
        source_removed: bool,
        references: UndoReferences,
        archives: Vec<String>,
        command_id: &str,
    ) -> Result<Self, String> {
        let review_revision = revision(&(
            "calendar-delete-undo-v1",
            command_id,
            before.revision()?,
            &references,
            &archives,
        ))?;
        Ok(Self {
            before,
            source_id,
            source_removed,
            references,
            archives,
            expected_source_revision: None,
            sealed: false,
            review_revision,
        })
    }

    /// Read the exact postimage in the deletion transaction, then hash it on an
    /// admitted worker. A failed seal rolls back the whole operation.
    pub async fn seal(&mut self, tx: &mut Transaction<'_, Sqlite>) -> Result<(), String> {
        let source = self
            .read_current_source(tx, &mut ReadBudget::default())
            .await?;
        if source.is_some() == self.source_removed {
            return Err("Calendar deletion has an unexpected source postimage".into());
        }
        let permit = SCOPE_GATE
            .try_acquire()
            .map_err(|_| "Calendar Undo sealing is busy")?;
        let worker = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            source.map(|source| source.revision()).transpose()
        });
        self.expected_source_revision = tokio::time::timeout(SCOPE_WORKER_TIMEOUT, worker)
            .await
            .map_err(|_| "Calendar Undo sealing timed out")?
            .map_err(|error| format!("Calendar Undo sealing worker: {error}"))??;
        self.sealed = true;
        Ok(())
    }

    async fn read_current_source(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        budget: &mut ReadBudget,
    ) -> Result<Option<Metadata>, String> {
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM calendar_events WHERE id=?)")
                .bind(&self.source_id)
                .fetch_one(&mut **tx)
                .await
                .map_err(|error| format!("read Calendar Undo source: {error}"))?;
        if exists {
            Ok(Some(Metadata::read(tx, &self.source_id, budget).await?))
        } else {
            Ok(None)
        }
    }

    /// Receipt confirmation and native expiry checks precede this preparation.
    /// Every affected projection is checked before any restoration is written.
    pub async fn prepare(
        self: Arc<Self>,
        tx: &mut Transaction<'_, Sqlite>,
        command_id: &str,
    ) -> Result<super::super::commit::PreparedCommit, String> {
        if !self.sealed {
            return Err("Calendar Undo has no confirmed postimage".into());
        }
        let mut budget = ReadBudget::default();
        let current = self.read_current_source(tx, &mut budget).await?;
        let original_graph = if current.is_none() {
            Some(
                self.before
                    .read_original_preservation(tx, &mut budget)
                    .await?,
            )
        } else {
            None
        };
        let archives = serde_json::to_string(&self.archives)
            .map_err(|error| format!("encode Calendar Undo archives: {error}"))?;
        let missing_archive: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM json_each(?1) expected
            LEFT JOIN calendar_event_archives archive ON archive.id=expected.value
            WHERE archive.id IS NULL OR archive.source_event_id IS NOT ?2)",
        )
        .bind(archives)
        .bind(&self.source_id)
        .fetch_one(&mut **tx)
        .await
        .map_err(|error| format!("verify Calendar Undo archives: {error}"))?;
        if missing_archive {
            return Err("Calendar archive changed after deletion; Undo cannot overwrite it".into());
        }
        for run in &self.references.runs {
            let unchanged: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM pomodoro_runs WHERE id=?1
                AND event_id IS NULL AND calendar_archive_id=?2 AND ended_at IS NOT NULL
                AND COALESCE(current_occurrence_id, original_event_id)=?3)",
            )
            .bind(&run.id)
            .bind(&run.expected_archive_id)
            .bind(&run.current_identity)
            .fetch_one(&mut **tx)
            .await
            .map_err(|error| format!("verify Calendar Undo run: {error}"))?;
            if !unchanged {
                return Err(
                    "Calendar Focus reference changed after deletion; refresh before restoring"
                        .into(),
                );
            }
        }
        for segment in &self.references.segments {
            let unchanged: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pomodoro_segments WHERE id=?1 AND run_id=?2 AND event_id IS NULL)")
                .bind(&segment.id).bind(&segment.run_id).fetch_one(&mut **tx).await.map_err(|error| format!("verify Calendar Undo segment: {error}"))?;
            if !unchanged {
                return Err(
                    "Calendar segment reference changed after deletion; refresh before restoring"
                        .into(),
                );
            }
        }
        let permit = SCOPE_GATE
            .try_acquire()
            .map_err(|_| "Calendar Undo preparation is busy")?;
        let command_id = command_id.to_owned();
        let worker = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let actual = current.as_ref().map(Metadata::revision).transpose()?;
            if actual != self.expected_source_revision {
                return Err(
                    "Calendar source changed after deletion; Undo cannot overwrite later work"
                        .into(),
                );
            }
            if let Some(graph) = original_graph
                && graph.preservation_revision()? != self.before.preservation_revision()?
            {
                return Err(
                    "Calendar imported metadata changed after deletion; Undo cannot overwrite it"
                        .into(),
                );
            }
            let rows = self.before.prepare_restore(current.is_some())?;
            let source_id = self.source_id.clone();
            Ok(super::super::commit::PreparedCommit::from_undo(
                PreparedUndo {
                    rows,
                    preimage: self,
                },
                &command_id,
                source_id,
            ))
        });
        tokio::time::timeout(SCOPE_WORKER_TIMEOUT, worker)
            .await
            .map_err(|_| "Calendar Undo preparation timed out")?
            .map_err(|error| format!("Calendar Undo preparation worker: {error}"))?
    }
}

impl PreparedUndo {
    /// Restore original rows and nullable references only. The stopped run and
    /// its accepted interruption, pauses, segments and suppression remain intact.
    pub(in crate::events) async fn write(
        self,
        tx: &mut Transaction<'_, Sqlite>,
    ) -> Result<(), String> {
        self.rows.write(tx).await?;
        for run in &self.preimage.references.runs {
            let result = sqlx::query("UPDATE pomodoro_runs SET event_id=?1, calendar_archive_id=?2
                WHERE id=?3 AND event_id IS NULL AND calendar_archive_id=?4
                    AND COALESCE(current_occurrence_id, original_event_id)=?5 AND ended_at IS NOT NULL")
                .bind(&run.event_id).bind(&run.previous_archive_id).bind(&run.id).bind(&run.expected_archive_id).bind(&run.current_identity)
                .execute(&mut **tx).await.map_err(|error| format!("restore Calendar Undo run reference: {error}"))?;
            if result.rows_affected() != 1 {
                return Err("Calendar Undo run changed before restoration".into());
            }
        }
        for segment in &self.preimage.references.segments {
            let result = sqlx::query("UPDATE pomodoro_segments SET event_id=?1 WHERE id=?2 AND run_id=?3 AND event_id IS NULL")
                .bind(&segment.event_id).bind(&segment.id).bind(&segment.run_id).execute(&mut **tx).await.map_err(|error| format!("restore Calendar Undo segment reference: {error}"))?;
            if result.rows_affected() != 1 {
                return Err("Calendar Undo segment changed before restoration".into());
            }
        }
        for archive in &self.preimage.archives {
            super::super::restore::remove_restored_archive(tx, archive).await?;
        }
        Ok(())
    }
}
