//! Receipt-backed semantic edits, serialized with native Focus execution.

use super::create::CalendarIntent;
use super::edit::{EditAction, PreparedEdit, prepare_action};
use super::metadata::{PreparedMutation, revision};
use super::scope::{DeviceDate, SCOPE_GATE, SCOPE_WORKER_TIMEOUT, read_snapshot};
use crate::recurrence::canonical::{ActiveTarget, EditKind, EditScope, ScopeClock, parse_date};
use ganbaru_pomodoro::{
    FocusCalendarReferenceChange, FocusCommand, FocusExecutionSnapshot, FocusIntent,
};
use serde::{Deserialize, Serialize};
use sqlx::{Sqlite, Transaction};
use std::sync::Arc;

pub const MAX_RECEIPT_BYTES: usize = 16 * 1024 * 1024;

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommitRequest {
    pub vault_id: String,
    pub vault_generation: u64,
    command_id: String,
    review_revision: String,
    edit: CalendarIntent,
}

/// Immutable result of the accepted operation, including deterministic identities.
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommitReceipt {
    pub command_id: String,
    pub edited_id: String,
    pub changed: bool,
    preserved_ids: Vec<(String, String)>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    scheduled_tasks: Vec<super::task_schedule::ScheduledTaskIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub undo_review_revision: Option<String>,
}

/// Operational availability accompanies the immutable result. It is never
/// stored in the receipt, so retries cannot renew a process-local Undo lease.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitReply {
    #[serde(flatten)]
    pub receipt: CommitReceipt,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub undo_available_for_ms: Option<u64>,
}

/// A rejected operation can be reviewed again. An unknown outcome must retain
/// its exact request until the durable receipt resolves the accepted result.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitFailure {
    outcome: CommitOutcome,
    message: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum CommitOutcome {
    Rejected,
    Unknown,
}

impl CommitFailure {
    pub fn unknown(message: impl Into<String>) -> Self {
        Self {
            outcome: CommitOutcome::Unknown,
            message: message.into(),
        }
    }

    pub fn mark_uncertain(mut self) -> Self {
        self.outcome = CommitOutcome::Unknown;
        self
    }
}

impl From<String> for CommitFailure {
    fn from(message: String) -> Self {
        Self {
            outcome: CommitOutcome::Rejected,
            message,
        }
    }
}

impl From<&str> for CommitFailure {
    fn from(message: &str) -> Self {
        message.to_owned().into()
    }
}

pub struct PreparedCommit {
    rows: PreparedWrite,
    receipt: CommitReceipt,
    active: Option<(String, String, String)>,
    historical: Vec<(String, String, String)>,
    scheduled_tasks: Vec<ganbaru_projects::scheduling::ScheduledTaskWrite>,
    pub clock_fence: CommitClockFence,
    pub completion_window: Option<(i64, i64)>,
    pub start_occurrence: Option<String>,
}

enum PreparedWrite {
    Mutation(PreparedMutation),
    Deletion(Box<super::deletion::PreparedDeletion>),
    Undo(super::deletion::PreparedUndo),
}

#[derive(Clone, Copy)]
pub struct CommitClockFence {
    valid_until_ms: Option<i64>,
    floating_today: Option<chrono::NaiveDate>,
}

impl CommitClockFence {
    /// Guard the review against a start boundary or device-date rollover while
    /// worker and SQL work were in flight. A timed source needs no platform call.
    pub async fn verify(self, device: Arc<dyn DeviceDate>, now_ms: i64) -> Result<(), String> {
        if self
            .valid_until_ms
            .is_some_and(|deadline| now_ms >= deadline)
        {
            return Err("Calendar protection changed during saving; review again".into());
        }
        if let Some(expected) = self.floating_today {
            let permit = SCOPE_GATE
                .try_acquire()
                .map_err(|_| "Calendar device-date verification is busy; retry Save")?;
            let worker = tokio::task::spawn_blocking(move || -> Result<(), String> {
                let _permit = permit;
                if device.local_date(now_ms)? != expected {
                    return Err("Calendar device date changed during saving; review again".into());
                }
                Ok(())
            });
            tokio::time::timeout(SCOPE_WORKER_TIMEOUT, worker)
                .await
                .map_err(|_| "Calendar device-date verification timed out")?
                .map_err(|error| format!("Calendar device-date worker: {error}"))??;
        }
        Ok(())
    }
}

impl CommitRequest {
    pub fn is_delete(&self) -> bool {
        matches!(self.edit, CalendarIntent::Delete(_))
    }

    pub fn undo_target(&self) -> Option<&str> {
        match &self.edit {
            CalendarIntent::UndoDelete(undo) => Some(&undo.delete_command_id),
            _ => None,
        }
    }

    pub fn verify_undo_review(&self, revision: &str) -> Result<(), String> {
        if self.review_revision != revision {
            return Err("Calendar Undo belongs to a different deletion preimage".into());
        }
        Ok(())
    }

    pub fn command_id(&self) -> &str {
        &self.command_id
    }

    pub fn receipt_identity(&self) -> Result<String, String> {
        self.intent_hash()
    }

    /// Read an immutable retry result through a deferred, non-mutating transaction.
    /// Absence never authorizes a new write; the caller still enforces its vault context.
    pub async fn read_only_retry(
        &self,
        pool: &sqlx::SqlitePool,
    ) -> Result<Option<CommitReceipt>, CommitFailure> {
        let mut tx = pool.begin().await.map_err(|error| {
            CommitFailure::unknown(format!("begin Calendar receipt read: {error}"))
        })?;
        let receipt = self
            .read_receipt(&mut tx)
            .await
            .map_err(CommitFailure::unknown)?;
        tx.rollback().await.map_err(|error| {
            CommitFailure::unknown(format!("finish Calendar receipt read: {error}"))
        })?;
        Ok(receipt)
    }

    /// The Calendar receipt owns retry recovery; the nested Focus receipt shares
    /// its transaction and has a distinct, bounded identity.
    pub fn start_focus_command(
        &self,
        occurrence_id: String,
        snapshot: &FocusExecutionSnapshot,
    ) -> Result<FocusCommand, String> {
        if snapshot
            .run
            .as_ref()
            .is_some_and(|run| run.ended_at_ms.is_none())
        {
            return Err("Another Focus run is active; refresh before enabling Focus".into());
        }
        Ok(FocusCommand {
            command_id: format!(
                "calendar-focus:{}",
                revision(&(&self.command_id, "enable_focus"))?
            ),
            expected_revision: snapshot.revision,
            intent: FocusIntent::StartScheduled {
                occurrence_id: Some(occurrence_id),
            },
        })
    }

    /// Checks request identity and size bounds before any vault work.
    pub fn validate(&self) -> Result<(), String> {
        validate_command_id(&self.command_id)?;
        if self.vault_id.trim().is_empty()
            || self.vault_id.len() > 1024
            || self.vault_id.chars().any(char::is_control)
        {
            return Err("Calendar vaultId is empty or exceeds its bounds".into());
        }
        if self.review_revision.len() != 64
            || !self
                .review_revision
                .bytes()
                .all(|value| value.is_ascii_hexdigit())
        {
            return Err("Calendar edit requires its native review revision".into());
        }
        self.edit.check_limits()
    }

    fn intent_hash(&self) -> Result<String, String> {
        // Generation changes after restart. An accepted retry remains bound to
        // the same vault and intent, independently of today's process generation.
        revision(&(
            &self.vault_id,
            &self.command_id,
            &self.review_revision,
            &self.edit,
        ))
    }

    /// Receipts are checked before source lookup, recurrence expansion or recovery.
    pub async fn read_receipt(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
    ) -> Result<Option<CommitReceipt>, String> {
        self.validate()?;
        let row: Option<(Option<String>, Option<String>)> = sqlx::query_as(
            "SELECT CASE WHEN length(CAST(intent_hash AS BLOB)) = 64 THEN intent_hash END,
                    CASE WHEN length(CAST(result_json AS BLOB)) <= ? THEN result_json END
             FROM calendar_edit_receipts WHERE command_id = ?",
        )
        .bind(MAX_RECEIPT_BYTES as i64)
        .bind(&self.command_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|error| format!("read Calendar edit receipt: {error}"))?;
        row.map(|(hash, result)| {
            let hash = hash.ok_or("Calendar receipt has an invalid intent hash")?;
            let result = result.ok_or("Calendar receipt exceeds its byte limit")?;
            if hash != self.intent_hash()? {
                return Err("Calendar commandId was already used with different intent".into());
            }
            serde_json::from_str(&result)
                .map_err(|error| format!("decode Calendar edit receipt: {error}"))
        })
        .transpose()
    }

    pub async fn record_receipt(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        receipt: &CommitReceipt,
        now_ms: i64,
    ) -> Result<(), String> {
        let result = serde_json::to_string(receipt).map_err(|error| error.to_string())?;
        if result.len() > MAX_RECEIPT_BYTES {
            return Err("Calendar receipt exceeds its byte limit".into());
        }
        sqlx::query("INSERT INTO calendar_edit_receipts (command_id, intent_hash, result_json, created_at_ms) VALUES (?, ?, ?, ?)")
            .bind(&self.command_id).bind(self.intent_hash()?).bind(result)
            .bind(now_ms).execute(&mut **tx).await.map_err(|error| format!("record Calendar edit receipt: {error}"))?;
        Ok(())
    }

    pub async fn prepare(
        &self,
        device: Arc<dyn DeviceDate>,
        tx: &mut Transaction<'_, Sqlite>,
        now_ms: i64,
    ) -> Result<PreparedCommit, String> {
        self.validate()?;
        let edit = match &self.edit {
            CalendarIntent::ScheduleTasks(schedule) => {
                return schedule
                    .prepare_commit(
                        tx,
                        self.command_id.clone(),
                        self.review_revision.clone(),
                        now_ms,
                    )
                    .await;
            }
            CalendarIntent::Create(create) => {
                return create
                    .prepare_commit(
                        self.command_id.clone(),
                        self.review_revision.clone(),
                        now_ms,
                    )
                    .await;
            }
            CalendarIntent::Delete(delete) => {
                return delete
                    .prepare_commit(
                        device,
                        tx,
                        self.command_id.clone(),
                        self.review_revision.clone(),
                        now_ms,
                    )
                    .await;
            }
            CalendarIntent::UndoDelete(_) => {
                return Err("Calendar Undo requires its live native preimage".into());
            }
            CalendarIntent::Edit(edit) => edit,
        };
        let permit = SCOPE_GATE
            .try_acquire()
            .map_err(|_| "A Calendar edit is being prepared; retry when it finishes")?;
        let snapshot = read_snapshot(tx, &edit.selection.template_id).await?;
        // Copy a bounded owned draft for the worker. It never accepts prepared
        // write operations, clocks, source rows or execution identity from IPC.
        let draft = edit.draft.clone();
        let action = edit.action;
        let selected = parse_date(&edit.selection.recurrence_date)?;
        let scope = edit.selection.scope;
        let command_id = self.command_id.clone();
        let expected_review = self.review_revision.clone();
        let worker = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let floating_today = if snapshot.geometry.source.all_day != 0 {
                Some(device.local_date(now_ms)?)
            } else {
                None
            };
            let clock = ScopeClock {
                epoch_ms: now_ms,
                floating_today,
            };
            let prepared = prepare_action(snapshot, selected, scope, clock, draft, action)?;
            check_review(prepared, clock, &command_id, &expected_review)
        });
        tokio::time::timeout(SCOPE_WORKER_TIMEOUT, worker)
            .await
            .map_err(|_| "Calendar commit preparation timed out")?
            .map_err(|error| format!("Calendar commit worker: {error}"))?
    }
}

/// Preview and commit use the same bounded operation identity for derived row IDs.
pub fn validate_command_id(value: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > 128 || value.chars().any(char::is_control) {
        return Err("Calendar commandId is empty or exceeds its bounds".into());
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn prepare_rows(
    snapshot: super::scope::ScopeSnapshot,
    selected: chrono::NaiveDate,
    scope: crate::recurrence::canonical::EditScope,
    clock: ScopeClock,
    draft: super::edit::EventDraft,
    command_id: &str,
    expected_review: &str,
) -> Result<PreparedCommit, String> {
    let prepared = prepare_action(snapshot, selected, scope, clock, draft, EditAction::Save)?;
    check_review(prepared, clock, command_id, expected_review)
}

pub(super) fn check_review(
    prepared: PreparedEdit,
    clock: ScopeClock,
    command_id: &str,
    expected_review: &str,
) -> Result<PreparedCommit, String> {
    if prepared.review_revision != expected_review {
        return Err(
            "Calendar source or protection changed; review the current edit before saving".into(),
        );
    }
    let mut commit = PreparedCommit::from_edit(prepared, command_id, clock.epoch_ms)?;
    commit.clock_fence.floating_today = clock.floating_today;
    Ok(commit)
}

impl PreparedCommit {
    pub(super) fn from_undo(
        undo: super::deletion::PreparedUndo,
        command_id: &str,
        source_id: String,
    ) -> Self {
        Self {
            receipt: CommitReceipt {
                command_id: command_id.into(),
                edited_id: source_id,
                changed: true,
                preserved_ids: Vec::new(),
                scheduled_tasks: Vec::new(),
                undo_review_revision: None,
            },
            rows: PreparedWrite::Undo(undo),
            active: None,
            historical: Vec::new(),
            scheduled_tasks: Vec::new(),
            completion_window: None,
            start_occurrence: None,
            clock_fence: CommitClockFence {
                valid_until_ms: None,
                floating_today: None,
            },
        }
    }

    /// Move the bounded preimage into the existing owner without another clone.
    pub fn into_write(mut self) -> (Self, Option<super::deletion::UndoPreimage>) {
        let undo = match &mut self.rows {
            PreparedWrite::Deletion(deletion) => deletion.undo.take(),
            _ => None,
        };
        (self, undo)
    }

    pub(super) fn from_deletion(
        deletion: super::deletion::PreparedDeletion,
        command_id: &str,
        valid_until_ms: Option<i64>,
        floating_today: Option<chrono::NaiveDate>,
    ) -> Self {
        Self {
            receipt: CommitReceipt {
                command_id: command_id.into(),
                edited_id: deletion.selected_id.clone(),
                changed: true,
                preserved_ids: Vec::new(),
                scheduled_tasks: Vec::new(),
                undo_review_revision: deletion
                    .undo
                    .as_ref()
                    .map(|undo| undo.review_revision.clone()),
            },
            rows: PreparedWrite::Deletion(Box::new(deletion)),
            active: None,
            historical: Vec::new(),
            scheduled_tasks: Vec::new(),
            completion_window: None,
            start_occurrence: None,
            clock_fence: CommitClockFence {
                valid_until_ms,
                floating_today,
            },
        }
    }

    /// Only the exact run identified by reviewed canonical deletion can stop.
    /// Its native Stop receipt is distinct and commits with the Calendar receipt.
    pub fn stop_focus_command(
        &self,
        request: &CommitRequest,
        snapshot: &FocusExecutionSnapshot,
    ) -> Result<Option<FocusCommand>, String> {
        let PreparedWrite::Deletion(deletion) = &self.rows else {
            return Ok(None);
        };
        let Some(expected) = &deletion.active_run_to_stop else {
            return Ok(None);
        };
        if snapshot
            .run
            .as_ref()
            .is_none_or(|run| &run.id != expected || run.ended_at_ms.is_some())
        {
            return Err("Calendar reviewed Focus run changed before deletion".into());
        }
        Ok(Some(FocusCommand {
            command_id: format!(
                "calendar-focus:{}",
                revision(&(&request.command_id, "delete_stop"))?
            ),
            expected_revision: snapshot.revision,
            intent: FocusIntent::Stop,
        }))
    }

    pub(super) fn from_creation(rows: PreparedMutation, command_id: &str) -> Self {
        Self {
            receipt: CommitReceipt {
                command_id: command_id.into(),
                edited_id: rows.edited_id.clone(),
                changed: true,
                preserved_ids: Vec::new(),
                scheduled_tasks: Vec::new(),
                undo_review_revision: None,
            },
            rows: PreparedWrite::Mutation(rows),
            active: None,
            historical: Vec::new(),
            scheduled_tasks: Vec::new(),
            completion_window: None,
            start_occurrence: None,
            clock_fence: CommitClockFence {
                valid_until_ms: None,
                floating_today: None,
            },
        }
    }

    fn from_edit(prepared: PreparedEdit, command_id: &str, now_ms: i64) -> Result<Self, String> {
        let review_valid_until_ms = std::iter::once(prepared.scope.selected.start_ms)
            .chain(
                prepared
                    .scope
                    .first_mutable
                    .as_ref()
                    .map(|value| value.start_ms),
            )
            .filter(|value| *value > now_ms)
            .chain(
                ((prepared.scope.selected_active && prepared.action == EditAction::Save)
                    || prepared.action == EditAction::EnableFocus)
                    .then_some(prepared.selected_after.end_ms),
            )
            .min();
        let rows = prepared.metadata.prepare_mutation(
            &prepared.plan,
            &prepared.draft,
            command_id,
            now_ms,
        )?;
        let mut history_targets = rows.preserved_ids.clone();
        if prepared.scope.effective_scope == EditScope::This && rows.edited_id != prepared.source_id
        {
            // Earlier closed runs on this still-ongoing occurrence follow its
            // new visible owner while their original execution facts stay intact.
            history_targets.push((
                prepared.scope.selected.recurrence_date.clone(),
                rows.edited_id.clone(),
            ));
        }
        let historical = prepared.evidence.historical_retargets(
            &prepared.source_id,
            prepared.source_anchor,
            &history_targets,
        )?;
        let mut active = None;
        if let Some(transfer) = &prepared.plan.active_transfer {
            let id = match &transfer.target {
                ActiveTarget::Edited => rows.edited_id.clone(),
                ActiveTarget::Preserved { recurrence_date } => rows
                    .preserved_ids
                    .iter()
                    .find(|(date, _)| date == recurrence_date)
                    .map(|(_, id)| id.clone())
                    .ok_or("Calendar active preservation target is missing")?,
            };
            let start_ms = match &transfer.target {
                ActiveTarget::Edited => prepared.selected_after.start_ms,
                ActiveTarget::Preserved { recurrence_date } => prepared
                    .plan
                    .materialize
                    .iter()
                    .find(|value| &value.recurrence_date == recurrence_date)
                    .map(|value| value.start_ms)
                    .ok_or("Calendar active preservation geometry is missing")?,
            };
            let occurrence_id = rows.active_anchor_id(&id, start_ms)?;
            active = Some((transfer.run_id.clone(), id, occurrence_id));
        } else if prepared.scope.selected_active && prepared.plan.kind != EditKind::Unchanged {
            let run = prepared
                .scope
                .active_run_id
                .clone()
                .ok_or("Calendar active edit lost its run")?;
            let occurrence_id =
                rows.active_anchor_id(&rows.edited_id, prepared.selected_after.start_ms)?;
            active = Some((run, rows.edited_id.clone(), occurrence_id));
        }
        let receipt = CommitReceipt {
            command_id: command_id.into(),
            edited_id: rows.edited_id.clone(),
            changed: prepared.plan.kind != EditKind::Unchanged,
            preserved_ids: rows.preserved_ids.clone(),
            scheduled_tasks: Vec::new(),
            undo_review_revision: None,
        };
        Ok(Self {
            start_occurrence: if prepared.action == EditAction::EnableFocus {
                Some(rows.active_anchor_id(&rows.edited_id, prepared.selected_after.start_ms)?)
            } else {
                None
            },
            completion_window: (prepared.action == EditAction::EndNow).then_some((
                prepared.selected_after.start_ms,
                prepared.selected_after.end_ms,
            )),
            rows: PreparedWrite::Mutation(rows),
            receipt,
            active,
            historical,
            scheduled_tasks: Vec::new(),
            clock_fence: CommitClockFence {
                valid_until_ms: review_valid_until_ms,
                floating_today: None,
            },
        })
    }

    /// Match the exact native run after preparation; never select the first open run.
    pub fn active_change(
        &self,
        snapshot: &ganbaru_pomodoro::FocusExecutionSnapshot,
    ) -> Result<Option<FocusCalendarReferenceChange>, String> {
        self.active
            .as_ref()
            .map(|(run_id, event_id, occurrence_id)| {
                let run = snapshot
                    .run
                    .as_ref()
                    .filter(|run| &run.id == run_id)
                    .ok_or("Calendar active run changed before its write")?;
                Ok(FocusCalendarReferenceChange {
                    run_id: run_id.clone(),
                    expected_occurrence_id: run.occurrence_id.clone(),
                    target_event_id: event_id.clone(),
                    target_occurrence_id: occurrence_id.clone(),
                    target_event_date: run.event_date.clone(),
                })
            })
            .transpose()
    }

    pub async fn write(self, tx: &mut Transaction<'_, Sqlite>) -> Result<CommitReceipt, String> {
        match self.rows {
            PreparedWrite::Mutation(rows) => rows.write(tx).await?,
            PreparedWrite::Deletion(deletion) => (*deletion).write(tx).await?,
            PreparedWrite::Undo(undo) => undo.write(tx).await?,
        }
        ganbaru_projects::scheduling::write(tx, self.scheduled_tasks).await?;
        for (run_id, expected, target) in self.historical {
            let changed = sqlx::query("UPDATE pomodoro_runs SET event_id = ?1, current_occurrence_id = ?1 WHERE id = ?2 AND ended_at IS NOT NULL AND COALESCE(current_occurrence_id, original_event_id) = ?3")
                .bind(target).bind(run_id).bind(expected).execute(&mut **tx).await.map_err(|error| format!("retarget preserved Calendar history: {error}"))?;
            if changed.rows_affected() != 1 {
                return Err("Calendar history changed before its prepared write".into());
            }
        }
        Ok(self.receipt)
    }

    pub(super) fn attach_scheduled_tasks(
        &mut self,
        tasks: Vec<ganbaru_projects::scheduling::ScheduledTaskWrite>,
        identities: Vec<super::task_schedule::ScheduledTaskIdentity>,
    ) {
        self.scheduled_tasks = tasks;
        self.receipt.scheduled_tasks = identities;
    }
}
