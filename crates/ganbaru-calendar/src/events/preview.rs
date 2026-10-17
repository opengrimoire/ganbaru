//! Read-only visible projection from the exact rows the semantic Save would write.

use super::create::{CalendarIntent, CreatePreview};
use super::edit::PreparedEdit;
use crate::reads::native_window::{NativeCalendarWindow, NativeWindowRequest};
use crate::recurrence::canonical::{EditKind, EditScope, ScopePlan, SelectedTarget, Window};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::sync::Arc;

use sqlx::SqlitePool;

use super::scope::DeviceDate;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreviewRequest {
    command_id: String,
    edit: CalendarIntent,
    window: NativeWindowRequest,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EditPreview {
    command_id: String,
    pub(super) source_id: String,
    pub(super) review_revision: String,
    pub(super) edited_id: String,
    pub(super) changed: bool,
    pub(super) scope: ScopePlan,
    pub(super) window: NativeCalendarWindow,
    pub(super) previewed_ids: Vec<String>,
    pub(super) editing_id: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewResponse {
    vault_id: String,
    vault_generation: u64,
    preview: VisiblePreview,
}

#[derive(Serialize)]
#[serde(untagged)]
enum VisiblePreview {
    Edit(Box<EditPreview>),
    Create(Box<CreatePreview>),
    ScheduleTasks(Box<super::task_schedule::SchedulePreview>),
    Delete(Box<DeletePreview>),
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct DeletePreview {
    command_id: String,
    pub(super) source_id: String,
    pub(super) edited_id: String,
    pub(super) review_revision: String,
    changed: bool,
    scope: DeletePreviewScope,
    pub(super) window: NativeCalendarWindow,
    previewed_ids: Vec<String>,
    editing_id: Option<String>,
    outcome: crate::recurrence::canonical::DeleteOutcome,
    requires_active_stop: bool,
    history_only: bool,
    valid_until_ms: Option<i64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DeletePreviewScope {
    effective_scope: EditScope,
    selected_started: bool,
    selected_has_history: bool,
    selected_active: bool,
}

/// Deletion projects the exact retained rows through the persisted window
/// expander. Removing a source has an empty family, never the no-op fallback.
pub(super) fn project_delete(
    snapshot: super::scope::ScopeSnapshot,
    selected: chrono::NaiveDate,
    scope: EditScope,
    clock: crate::recurrence::canonical::ScopeClock,
    command_id: &str,
    window: &Window,
) -> Result<DeletePreview, String> {
    let active = snapshot
        .evidence
        .resolve(
            &snapshot.geometry.source.id,
            snapshot.geometry.template(true)?.anchor_date(),
        )?
        .active;
    let reviewed = super::deletion::prepare_review(&snapshot, selected, scope, clock)?;
    let plan = reviewed.plan;
    snapshot.metadata.reserve_deletion_copies(
        plan.archive_occurrences.len()
            + usize::from(plan.archive_source)
            + usize::from(plan.source_after.is_some())
            + 1,
    )?;
    let projected = if let Some(side) = &plan.source_after {
        snapshot
            .metadata
            .retained_deletion(side, clock.epoch_ms)?
            .project(&snapshot.metadata, window)?
    } else {
        crate::reads::native_window::WindowSource::from_prepared(Vec::new(), Vec::new(), Vec::new())
            .expand(window)?
    };
    if let Some(diagnostic) = projected.diagnostics.first() {
        return Err(format!(
            "Calendar deletion preview cannot expand {}: {}",
            diagnostic.event_id, diagnostic.message
        ));
    }
    let source_id = snapshot.geometry.source.id;
    Ok(DeletePreview {
        command_id: command_id.into(),
        edited_id: source_id.clone(),
        source_id,
        review_revision: reviewed.review_revision,
        changed: true,
        scope: DeletePreviewScope {
            effective_scope: plan.effective_scope,
            selected_started: plan.selected_started,
            selected_has_history: plan.selected_has_history,
            selected_active: active.is_some_and(|(_, date)| date == selected),
        },
        window: projected,
        previewed_ids: Vec::new(),
        editing_id: None,
        outcome: plan.outcome,
        requires_active_stop: plan.active_run_to_stop.is_some(),
        history_only: plan.history_only,
        valid_until_ms: plan.valid_until_ms,
    })
}

/// Project only the affected source family. The client retains unrelated cached
/// occurrences and discards responses belonging to an older draft generation.
pub(super) fn project(
    prepared: PreparedEdit,
    command_id: &str,
    window: &Window,
    now_ms: i64,
) -> Result<EditPreview, String> {
    let rows =
        prepared
            .metadata
            .prepare_mutation(&prepared.plan, &prepared.draft, command_id, now_ms)?;
    let projected = rows.project(&prepared.metadata, window)?;
    if let Some(diagnostic) = projected.diagnostics.first() {
        return Err(format!(
            "Calendar preview cannot expand {}: {}",
            diagnostic.event_id, diagnostic.message
        ));
    }
    let changed = prepared.plan.kind != EditKind::Unchanged;
    let editing_id = projected
        .occurrences
        .iter()
        .find(|occurrence| match &prepared.plan.selected_target {
            Some(SelectedTarget::Original) => occurrence.id == prepared.scope.selected.id,
            Some(SelectedTarget::Edited { recurrence_date }) => {
                occurrence.template_id == rows.edited_id
                    && &occurrence.recurrence_date == recurrence_date
            }
            Some(SelectedTarget::Preserved { recurrence_date }) => rows
                .preserved_ids
                .iter()
                .any(|(date, id)| date == recurrence_date && &occurrence.id == id),
            None => false,
        })
        .map(|occurrence| occurrence.id.clone());
    let protected: BTreeSet<_> = prepared
        .scope
        .preserve
        .iter()
        .map(|item| item.recurrence_date.as_str())
        .collect();
    let mutable_from = match prepared.scope.effective_scope {
        EditScope::Following => Some(&prepared.scope.selected.recurrence_date),
        EditScope::All => prepared
            .scope
            .first_mutable
            .as_ref()
            .map(|row| &row.recurrence_date),
        EditScope::This => None,
    };
    let previewed_ids = projected
        .occurrences
        .iter()
        .filter(|occurrence| {
            if changed {
                occurrence.template_id == rows.edited_id
            } else if prepared.scope.effective_scope != EditScope::This {
                mutable_from.is_some_and(|first| &occurrence.recurrence_date >= first)
                    && !protected.contains(occurrence.recurrence_date.as_str())
            } else {
                Some(&occurrence.id) == editing_id.as_ref()
            }
        })
        .map(|occurrence| occurrence.id.clone())
        .collect();
    Ok(EditPreview {
        command_id: command_id.into(),
        source_id: prepared.source_id,
        review_revision: prepared.review_revision,
        edited_id: rows.edited_id,
        changed,
        scope: prepared.scope,
        window: projected,
        previewed_ids,
        editing_id,
    })
}

/// Vault identity and effective Focus owner clock captured for one review.
pub struct ReviewContext {
    /// Active vault identity.
    pub vault_id: String,
    /// Active vault generation, which changes when the vault is replaced.
    pub vault_generation: u64,
    /// Effective owner clock, never earlier than its last observation.
    pub clock_ms: i64,
}

/// Application services that bind a preview to the active vault and its Focus owner.
pub trait PreviewHost: Clone + Send + Sync + 'static {
    /// Captures the active vault and the effective Focus owner clock.
    fn review_context(&self) -> impl Future<Output = Result<ReviewContext, String>> + Send;
    /// Opens the authorized vault database.
    fn connect(&self) -> impl Future<Output = Result<SqlitePool, String>> + Send;
    /// Returns the active vault identity. Blocking preview workers call it.
    fn active_vault_id(&self) -> Result<String, String>;
    /// Returns the platform source for floating all-day protection.
    fn device_date(&self) -> Arc<dyn DeviceDate>;
}

/// Preview retains no write transaction and never persists its prepared rows.
pub async fn preview_edit<H: PreviewHost>(
    host: H,
    request: PreviewRequest,
) -> Result<PreviewResponse, String> {
    super::commit::validate_command_id(&request.command_id)?;
    request.edit.check_limits()?;
    if request.window.window_start_date.len() != 10
        || request.window.window_end_date.len() != 10
        || request.window.render_zone.len() > 255
    {
        return Err("Calendar preview window exceeds its field limits".into());
    }
    let ReviewContext {
        vault_id,
        vault_generation,
        clock_ms: clock_floor_ms,
    } = host.review_context().await?;
    let edit = match request.edit {
        CalendarIntent::Delete(delete) => {
            let expected_vault = vault_id.clone();
            let verify = host.clone();
            let pool = host.connect().await?;
            let preview = super::scope::prepare_request_with_clock(
                &pool,
                host.device_date(),
                delete.selection(),
                clock_floor_ms,
                move |snapshot, selected, scope, clock| {
                    let window = Window::new(
                        &request.window.window_start_date,
                        &request.window.window_end_date,
                        &ganbaru_civil_time::zone(&request.window.render_zone)?,
                    )?;
                    let preview = project_delete(
                        snapshot,
                        selected,
                        scope,
                        clock,
                        &request.command_id,
                        &window,
                    )?;
                    if verify.active_vault_id()? != expected_vault {
                        return Err("Calendar vault changed while reviewing deletion".into());
                    }
                    Ok(preview)
                },
            )
            .await?;
            let current = host.review_context().await?;
            if current.vault_id != vault_id
                || current.vault_generation != vault_generation
                || preview
                    .valid_until_ms
                    .is_some_and(|deadline| current.clock_ms >= deadline)
            {
                return Err("Calendar vault or protection changed while reviewing deletion".into());
            }
            return Ok(PreviewResponse {
                vault_id,
                vault_generation,
                preview: VisiblePreview::Delete(Box::new(preview)),
            });
        }
        CalendarIntent::ScheduleTasks(schedule) => {
            let permit = super::scope::SCOPE_GATE
                .try_acquire()
                .map_err(|_| "A Calendar preview is being prepared; retry after it finishes")?;
            let pool = host.connect().await?;
            let mut tx = pool
                .begin()
                .await
                .map_err(|error| format!("begin Project scheduling preview: {error}"))?;
            let timestamp = super::time::current_utc_iso(&mut tx).await?;
            let now_ms = super::time::calendar_timestamp_millis(&timestamp)
                .ok_or("Project scheduling preview requires a native clock")?
                .max(clock_floor_ms);
            let snapshot = schedule.read(&mut tx).await?;
            tx.commit()
                .await
                .map_err(|error| format!("finish Project scheduling preview: {error}"))?;
            let expected_vault = vault_id.clone();
            let verify = host.clone();
            let worker = tokio::task::spawn_blocking(move || -> Result<VisiblePreview, String> {
                let _permit = permit;
                let window = Window::new(
                    &request.window.window_start_date,
                    &request.window.window_end_date,
                    &ganbaru_civil_time::zone(&request.window.render_zone)?,
                )?;
                let preview = schedule
                    .prepare(snapshot, &request.command_id, now_ms)?
                    .project(&request.command_id, &window)?;
                if verify.active_vault_id()? != expected_vault {
                    return Err("Calendar vault changed while preparing Project scheduling".into());
                }
                Ok(VisiblePreview::ScheduleTasks(Box::new(preview)))
            });
            let preview = tokio::time::timeout(super::scope::SCOPE_WORKER_TIMEOUT, worker)
                .await
                .map_err(|_| "Project scheduling preview timed out")?
                .map_err(|error| format!("Project scheduling preview worker: {error}"))??;
            return Ok(PreviewResponse {
                vault_id,
                vault_generation,
                preview,
            });
        }
        CalendarIntent::Create(create) => {
            let permit = super::scope::SCOPE_GATE
                .try_acquire()
                .map_err(|_| "A Calendar preview is being prepared; retry after it finishes")?;
            // Authorize the supplied URL before doing CPU work. Creation has no
            // source snapshot, but its visible timestamps still use native time.
            let pool = host.connect().await?;
            let mut tx = pool
                .begin()
                .await
                .map_err(|error| format!("begin Calendar creation preview: {error}"))?;
            let timestamp = super::time::current_utc_iso(&mut tx).await?;
            let now_ms = super::time::calendar_timestamp_millis(&timestamp)
                .ok_or("Calendar creation preview requires a native clock")?
                .max(clock_floor_ms);
            tx.commit()
                .await
                .map_err(|error| format!("finish Calendar creation preview: {error}"))?;
            let expected_vault = vault_id.clone();
            let verify = host.clone();
            let worker = tokio::task::spawn_blocking(move || -> Result<VisiblePreview, String> {
                let _permit = permit;
                let window = Window::new(
                    &request.window.window_start_date,
                    &request.window.window_end_date,
                    &ganbaru_civil_time::zone(&request.window.render_zone)?,
                )?;
                let preview = create
                    .prepare(&request.command_id, now_ms)?
                    .project(&request.command_id, &window)?;
                if verify.active_vault_id()? != expected_vault {
                    return Err("Calendar vault changed while preparing creation".into());
                }
                Ok(VisiblePreview::Create(Box::new(preview)))
            });
            let preview = tokio::time::timeout(super::scope::SCOPE_WORKER_TIMEOUT, worker)
                .await
                .map_err(|_| "Calendar creation preview timed out")?
                .map_err(|error| format!("Calendar creation preview worker: {error}"))??;
            return Ok(PreviewResponse {
                vault_id,
                vault_generation,
                preview,
            });
        }
        CalendarIntent::Edit(edit) => edit,
        CalendarIntent::UndoDelete(_) => {
            return Err("Calendar Undo requires its live native preimage".into());
        }
    };
    let expected_vault = vault_id.clone();
    let verify = host.clone();
    let pool = host.connect().await?;
    let preview = super::scope::prepare_request_with_clock(
        &pool,
        host.device_date(),
        edit.selection,
        clock_floor_ms,
        move |snapshot, selected, scope, clock| {
            let window = Window::new(
                &request.window.window_start_date,
                &request.window.window_end_date,
                &ganbaru_civil_time::zone(&request.window.render_zone)?,
            )?;
            let prepared = super::edit::prepare_action(
                snapshot,
                selected,
                scope,
                clock,
                edit.draft,
                edit.action,
            )?;
            let result = project(prepared, &request.command_id, &window, clock.epoch_ms)?;
            if verify.active_vault_id()? != expected_vault {
                return Err("Calendar vault changed while preparing the preview".into());
            }
            Ok(result)
        },
    )
    .await?;
    Ok(PreviewResponse {
        vault_id,
        vault_generation,
        preview: VisiblePreview::Edit(Box::new(preview)),
    })
}
