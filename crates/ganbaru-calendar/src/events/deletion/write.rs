//! Prepared scoped deletion rows over complete canonical metadata.

use chrono::{DateTime, SecondsFormat};
use sqlx::{Sqlite, Transaction};

use super::super::metadata::{PreparedArchive, PreparedMutation, revision};
use super::super::scope::ScopeSnapshot;
use super::super::types::CalendarEventMutationContext;
use crate::recurrence::canonical::DeletePlan;

pub(in crate::events) struct PreparedDeletion {
    source_id: String,
    archives: Vec<PreparedArchive>,
    retained: Option<PreparedMutation>,
    references: Vec<(String, String, String)>,
    archived_at: String,
    pub(in crate::events) selected_id: String,
    pub(in crate::events) active_run_to_stop: Option<String>,
    pub(in crate::events) undo: Option<super::UndoPreimage>,
}

fn instant(value: i64) -> Result<String, String> {
    DateTime::from_timestamp_millis(value)
        .map(|time| time.to_rfc3339_opts(SecondsFormat::Millis, true))
        .ok_or_else(|| "Calendar deletion instant is invalid".into())
}

impl PreparedDeletion {
    pub(in crate::events) fn prepare(
        snapshot: ScopeSnapshot,
        plan: DeletePlan,
        command_id: &str,
        now_ms: i64,
    ) -> Result<Self, String> {
        let source = &snapshot.geometry.source;
        let source_id = source.id.clone();
        let template = snapshot.geometry.template(true)?;
        snapshot.metadata.reserve_deletion_copies(
            plan.archive_occurrences.len()
                + usize::from(plan.archive_source)
                + usize::from(plan.source_after.is_some())
                + 1,
        )?;
        let mut archives =
            Vec::with_capacity(plan.archive_occurrences.len() + usize::from(plan.archive_source));
        let mut targets = Vec::with_capacity(plan.archive_occurrences.len());
        let archive_id = |identity: &str| -> Result<String, String> {
            Ok(format!(
                "calendar-archive-{}",
                revision(&(command_id, &source_id, identity))?
            ))
        };
        let master = if plan.archive_source {
            let id = archive_id("master")?;
            let context = CalendarEventMutationContext {
                id: source_id.clone(),
                canonical_id: source_id.clone(),
                source_event_id: source_id.clone(),
                occurrence_date: None,
                start_time: source.start_time.clone(),
                end_time: source.end_time.clone(),
                rrule: source.rrule.clone(),
                repeat_until: source.repeat_until.clone(),
                synthetic: false,
            };
            archives.push(snapshot.metadata.clone().prepare_archive(&id, &context)?);
            Some(id)
        } else {
            None
        };
        for occurrence in &plan.archive_occurrences {
            let id = archive_id(&occurrence.recurrence_date)?;
            let context = CalendarEventMutationContext {
                id: occurrence.id.clone(),
                canonical_id: occurrence.id.clone(),
                source_event_id: source_id.clone(),
                occurrence_date: Some(occurrence.recurrence_date.clone()),
                start_time: instant(occurrence.start_ms)?,
                end_time: instant(occurrence.end_ms)?,
                rrule: source.rrule.clone(),
                repeat_until: source.repeat_until.clone(),
                synthetic: true,
            };
            archives.push(snapshot.metadata.clone().prepare_archive(&id, &context)?);
            targets.push((occurrence.recurrence_date.clone(), id));
        }
        let references = snapshot.evidence.archived_references(
            &source_id,
            template.anchor_date(),
            &targets,
            master.as_deref(),
            plan.source_after.is_none(),
        )?;
        let retained = plan
            .source_after
            .as_ref()
            .map(|side| snapshot.metadata.retained_deletion(side, now_ms))
            .transpose()?;
        let undo_references = snapshot.evidence.deletion_undo_references(&references)?;
        let undo = super::UndoPreimage::capture(
            snapshot.metadata,
            source_id.clone(),
            retained.is_none(),
            undo_references,
            archives
                .iter()
                .map(|archive| archive.id().to_owned())
                .collect(),
            command_id,
        )?;
        Ok(Self {
            source_id,
            archives,
            retained,
            references,
            archived_at: instant(now_ms)?,
            selected_id: plan.selected.id,
            active_run_to_stop: plan.active_run_to_stop,
            undo: Some(undo),
        })
    }

    /// Stopping Focus precedes this writer in the same transaction. Original
    /// execution IDs, timestamps, pauses and segment outcomes are never rewritten.
    pub(in crate::events) async fn write(
        self,
        tx: &mut Transaction<'_, Sqlite>,
    ) -> Result<(), String> {
        for archive in &self.archives {
            archive.write(tx, &self.archived_at).await?;
        }
        for (run, expected, archive) in &self.references {
            let changed = sqlx::query("UPDATE pomodoro_runs SET event_id=NULL, calendar_archive_id=?1
                WHERE id=?2 AND event_id=?3 AND COALESCE(current_occurrence_id, original_event_id)=?4 AND ended_at IS NOT NULL")
                .bind(archive).bind(run).bind(&self.source_id).bind(expected).execute(&mut **tx).await
                .map_err(|error| format!("archive Calendar Focus reference: {error}"))?;
            if changed.rows_affected() != 1 {
                return Err("Calendar Focus history changed or its run was not stopped".into());
            }
            sqlx::query(
                "UPDATE pomodoro_segments SET event_id=NULL WHERE run_id=?1 AND
                (event_id=?2 OR (event_id>=?2 || '::' AND event_id<?2 || ':;'))",
            )
            .bind(run)
            .bind(&self.source_id)
            .execute(&mut **tx)
            .await
            .map_err(|error| format!("archive Calendar segment references: {error}"))?;
        }
        if let Some(retained) = self.retained {
            retained.write(tx).await?;
        } else {
            sqlx::query("DELETE FROM music_context_assignments WHERE owner_id=?1 AND owner_kind IN ('event-snapshot', 'event-override')")
                .bind(&self.source_id).execute(&mut **tx).await.map_err(|error| format!("remove deleted Calendar Music assignments: {error}"))?;
            let deleted = sqlx::query("DELETE FROM calendar_events WHERE id=?1")
                .bind(&self.source_id)
                .execute(&mut **tx)
                .await
                .map_err(|error| format!("delete Calendar source: {error}"))?;
            if deleted.rows_affected() != 1 {
                return Err("Calendar source disappeared before deletion".into());
            }
        }
        Ok(())
    }
}
