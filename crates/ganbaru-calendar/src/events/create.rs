//! Semantic creation reviewed and committed through Calendar's native authority.

use super::edit::{EditRequest, EventDraft};
use super::metadata::{Metadata, PreparedMutation, revision};
use super::scope::{SCOPE_GATE, SCOPE_WORKER_TIMEOUT};
use crate::reads::native_window::NativeCalendarWindow;
use crate::recurrence::canonical::{EditScope, Window};
use serde::{Deserialize, Serialize};

/// Untagged, so edit JSON and its durable receipt hash keep the plain `EditRequest` shape.
#[derive(Deserialize, Serialize)]
#[serde(untagged)]
pub(crate) enum CalendarIntent {
    Edit(EditRequest),
    Create(CreateRequest),
    ScheduleTasks(super::task_schedule::ScheduleRequest),
    Delete(super::deletion::DeleteRequest),
    UndoDelete(super::deletion::UndoRequest),
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CreateRequest {
    kind: CreateKind,
    pub(super) draft: EventDraft,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum CreateKind {
    Create,
}

impl CalendarIntent {
    pub(super) fn check_limits(&self) -> Result<(), String> {
        match self {
            Self::Edit(edit) => edit.draft.check_limits(),
            Self::Create(create) => create.draft.check_limits(),
            Self::ScheduleTasks(schedule) => schedule.check_limits(),
            Self::Delete(delete) => delete.check_limits(),
            Self::UndoDelete(undo) => undo.check_limits(),
        }
    }
}

pub(super) struct PreparedCreate {
    pub(super) rows: PreparedMutation,
    pub(super) review_revision: String,
}

impl CreateRequest {
    /// Hash normalized intent at a fixed timestamp, while persisted rows use
    /// the native acceptance clock. Moving time alone cannot invalidate creation.
    pub(super) fn prepare(&self, command_id: &str, now_ms: i64) -> Result<PreparedCreate, String> {
        super::commit::validate_command_id(command_id)?;
        let mut draft = self.draft.clone();
        draft.validate()?;
        let id = format!("calendar-create-{}", revision(&(command_id, "created"))?);
        let reviewed = Metadata::create(id.clone(), &draft, draft.timing.resolve_creation()?, 0)?;
        let review_revision =
            revision(&("calendar-creation-v1", command_id, reviewed.revision()?))?;
        let metadata = if now_ms == 0 {
            reviewed
        } else {
            Metadata::create(id, &draft, draft.timing.resolve_creation()?, now_ms)?
        };
        Ok(PreparedCreate {
            rows: PreparedMutation::creation(metadata)?,
            review_revision,
        })
    }

    pub(super) async fn prepare_commit(
        &self,
        command_id: String,
        review_revision: String,
        now_ms: i64,
    ) -> Result<super::commit::PreparedCommit, String> {
        self.draft.check_limits()?;
        let permit = SCOPE_GATE
            .try_acquire()
            .map_err(|_| "A Calendar operation is being prepared; retry after it finishes")?;
        let request = self.clone();
        let worker = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let prepared = request.prepare(&command_id, now_ms)?;
            if prepared.review_revision != review_revision {
                return Err(
                    "Calendar creation changed; review the current draft before saving".into(),
                );
            }
            Ok(super::commit::PreparedCommit::from_creation(
                prepared.rows,
                &command_id,
            ))
        });
        tokio::time::timeout(SCOPE_WORKER_TIMEOUT, worker)
            .await
            .map_err(|_| "Calendar creation preparation timed out")?
            .map_err(|error| format!("Calendar creation worker: {error}"))?
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CreatePreview {
    command_id: String,
    source_id: String,
    edited_id: String,
    review_revision: String,
    changed: bool,
    scope: CreateScope,
    window: NativeCalendarWindow,
    previewed_ids: Vec<String>,
    editing_id: Option<String>,
}

/// New sources have no recorded execution or source partition to protect.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CreateScope {
    pub(super) effective_scope: EditScope,
    pub(super) selected_started: bool,
    pub(super) selected_has_history: bool,
    pub(super) selected_active: bool,
}

impl PreparedCreate {
    pub(super) fn project(
        self,
        command_id: &str,
        window: &Window,
    ) -> Result<CreatePreview, String> {
        let projected = self.rows.project(&Metadata::default(), window)?;
        if let Some(diagnostic) = projected.diagnostics.first() {
            return Err(format!(
                "Calendar creation preview cannot expand {}: {}",
                diagnostic.event_id, diagnostic.message
            ));
        }
        let editing_id = projected
            .occurrences
            .iter()
            .find(|row| row.id == self.rows.edited_id)
            .map(|row| row.id.clone());
        let previewed_ids = projected
            .occurrences
            .iter()
            .map(|row| row.id.clone())
            .collect();
        Ok(CreatePreview {
            command_id: command_id.into(),
            source_id: self.rows.edited_id.clone(),
            edited_id: self.rows.edited_id,
            review_revision: self.review_revision,
            changed: true,
            scope: CreateScope {
                effective_scope: EditScope::This,
                selected_started: false,
                selected_has_history: false,
                selected_active: false,
            },
            window: projected,
            previewed_ids,
            editing_id,
        })
    }
}
