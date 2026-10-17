//! Native deletion review over complete metadata and execution evidence.

use std::sync::Arc;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::recurrence::canonical::{DeletePlan, EditScope, ScopeClock};

use super::metadata::revision;
use super::scope::{DeviceDate, ScopeRequest, ScopeSnapshot};

mod undo;
mod write;
pub use undo::UndoPreimage;
pub(super) use undo::{
    PreparedUndo, UndoReferences, UndoRequest, UndoRunReference, UndoSegmentReference,
};
pub(super) use write::PreparedDeletion;

/// Semantic selection and explicit permission to stop the reviewed native run.
/// No client-authored operation list, clock or execution identity is accepted.
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DeleteRequest {
    kind: DeleteKind,
    selection: ScopeRequest,
    stop_active: bool,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum DeleteKind {
    Delete,
}

impl DeleteRequest {
    pub(super) fn selection(&self) -> ScopeRequest {
        self.selection.clone()
    }

    pub(super) fn check_limits(&self) -> Result<(), String> {
        super::occurrence::validate_source_id(&self.selection.template_id)?;
        if self.selection.recurrence_date.len() != 10 {
            return Err("Calendar deletion requires an original YYYY-MM-DD identity".into());
        }
        crate::recurrence::canonical::parse_date(&self.selection.recurrence_date)?;
        Ok(())
    }

    pub(super) async fn prepare_commit(
        &self,
        device: Arc<dyn DeviceDate>,
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        command_id: String,
        review_revision: String,
        now_ms: i64,
    ) -> Result<super::commit::PreparedCommit, String> {
        self.check_limits()?;
        let permit = super::scope::SCOPE_GATE
            .try_acquire()
            .map_err(|_| "A Calendar operation is being prepared; retry after it finishes")?;
        let snapshot = super::scope::read_snapshot(tx, &self.selection.template_id).await?;
        let selected = crate::recurrence::canonical::parse_date(&self.selection.recurrence_date)?;
        let scope = self.selection.scope;
        let stop_active = self.stop_active;
        let worker = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let clock = ScopeClock {
                epoch_ms: now_ms,
                floating_today: if snapshot.geometry.source.all_day != 0 {
                    Some(device.local_date(now_ms)?)
                } else {
                    None
                },
            };
            prepare_commit(
                snapshot,
                selected,
                scope,
                clock,
                &command_id,
                &review_revision,
                stop_active,
            )
        });
        tokio::time::timeout(super::scope::SCOPE_WORKER_TIMEOUT, worker)
            .await
            .map_err(|_| "Calendar deletion preparation timed out")?
            .map_err(|error| format!("Calendar deletion worker: {error}"))?
    }
}

/// An informational review. Mutation must recapture the source and prepare the
/// same semantic selection inside the Calendar/Focus owner's transaction.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PreparedDelete {
    pub(super) review_revision: String,
    pub(super) plan: DeletePlan,
}

/// Read-only preparation accepts neither browser clocks nor mutation operations.
/// Complete metadata contributes to the review digest even when geometry matches.
#[cfg(test)]
pub(super) fn prepare(
    snapshot: ScopeSnapshot,
    selected: NaiveDate,
    scope: EditScope,
    clock: ScopeClock,
) -> Result<PreparedDelete, String> {
    prepare_review(&snapshot, selected, scope, clock)
}

pub(super) fn prepare_review(
    snapshot: &ScopeSnapshot,
    selected: NaiveDate,
    scope: EditScope,
    clock: ScopeClock,
) -> Result<PreparedDelete, String> {
    let source_revision = snapshot.metadata.revision()?;
    let template = snapshot.geometry.template(true)?;
    let evidence = snapshot
        .evidence
        .resolve(&snapshot.geometry.source.id, template.anchor_date())?;
    let plan = template.plan_delete(
        selected,
        scope,
        evidence,
        clock,
        snapshot.metadata.has_durable_references(),
    )?;
    Ok(PreparedDelete {
        review_revision: revision(&(
            &source_revision,
            snapshot.evidence.revision()?,
            &plan,
            clock.floating_today,
        ))?,
        plan,
    })
}

/// Reprepare inside the shared owner transaction and bind complete write rows
/// to the native review before performing any mutation.
pub(super) fn prepare_commit(
    snapshot: ScopeSnapshot,
    selected: NaiveDate,
    scope: EditScope,
    clock: ScopeClock,
    command_id: &str,
    expected_review: &str,
    stop_active: bool,
) -> Result<super::commit::PreparedCommit, String> {
    super::commit::validate_command_id(command_id)?;
    let reviewed = prepare_review(&snapshot, selected, scope, clock)?;
    if reviewed.review_revision != expected_review {
        return Err("Calendar source or protection changed; review deletion again".into());
    }
    if reviewed.plan.active_run_to_stop.is_some() && !stop_active {
        return Err("Calendar deletion must explicitly stop its reviewed Focus run".into());
    }
    let valid_until = reviewed.plan.valid_until_ms;
    let deletion = PreparedDeletion::prepare(snapshot, reviewed.plan, command_id, clock.epoch_ms)?;
    Ok(super::commit::PreparedCommit::from_deletion(
        deletion,
        command_id,
        valid_until,
        clock.floating_today,
    ))
}
