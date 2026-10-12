use crate::chat::ingestion::{ChatChangeEmitter, ChatEventIngestor};
use crate::chat::workspace::CreateProjectWorkingFolderRequest;
use ganbaru_chat::repository::attachments::{
    ChatAttachmentKind, import_attachment, run_due_attachment_cleanup,
};
use ganbaru_chat::repository::drafts::{ChatDraftWrite, delete_draft, read_draft, save_draft};
use ganbaru_chat::repository::events::{
    AppendCanonicalEventRequest, append_canonical_event, read_canonical_events,
};
use ganbaru_chat::repository::lifecycle::{
    permanently_delete_thread, resolve_project_deletion, set_thread_archived, set_thread_read,
};
use ganbaru_chat::repository::reads::{
    parse_timeline_cursor, read_project_shells, read_thread_shell, read_thread_shell_window,
    read_thread_shells, read_timeline_page, read_timeline_turn, search_thread_titles,
};
use ganbaru_chat::repository::rebuild::rebuild_thread_projections;
use ganbaru_chat::repository::receipts::{
    CommandReceiptClaim, CommandReceiptState, claim_command_receipt, complete_command_receipt,
};
use ganbaru_chat::repository::recovery::recover_orphaned_turns;
use ganbaru_chat::repository::resources::{
    ChatResourceKind, StoreBrowserArtifact, list_thread_resources, read_thread_resource_bytes,
    store_browser_artifact,
};
use ganbaru_chat::repository::workspaces::{create_workspace, list_workspaces, rename_workspace};
use ganbaru_chat_contracts::events::{
    CanonicalEvent, CanonicalRuntimeEvent, ChangedFileSummary, ContentDeltaEvent, DiffUpdatedEvent,
    ItemLifecycleEvent, RequestOpenedEvent, SessionExitedEvent, ThreadRevertedEvent,
    TurnAbortedEvent, TurnCompletedEvent, TurnStartedEvent, UserInputQuestion,
    UserInputRequestedEvent,
};
use ganbaru_chat_contracts::models::{
    ActivityStatus, CanonicalItemKind, CanonicalRequestKind, ChatAttachmentId, ChatCheckpointId,
    ChatCommandId, ChatEventId, ChatThreadId, ChatTurnId, ChatTurnState, ContentStreamKind,
    InteractionMode, ProjectWorkingFolderId, ProviderFamilyId, ProviderInstanceId,
    ProviderRequestId, ProviderSessionId, SafetyMode, TurnModeSnapshot, UtcTimestamp,
    VersionedJson,
};
use ganbaru_chat_contracts::models::{ChatChangeNotification, ChatError, ChatResult};
use sqlx::Row;
use std::collections::HashSet;
use std::fs;
use std::sync::{Arc, Mutex};

mod drafts;
mod events;
mod ingestion;
mod reads;
mod rebuild;
mod receipts;
mod recovery;
mod resources;
mod workspaces;

#[derive(Default)]
struct RecordingEmitter(Mutex<Vec<ChatChangeNotification>>);

impl ChatChangeEmitter for RecordingEmitter {
    fn emit(&self, notification: &ChatChangeNotification) -> ChatResult<()> {
        self.0
            .lock()
            .map_err(|_| ChatError::driver_unavailable("emitter lock"))?
            .push(notification.clone());
        Ok(())
    }
}

const NOW: &str = "2026-07-20T12:00:00Z";

pub(crate) async fn pool_with_thread() -> sqlx::SqlitePool {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::raw_sql("PRAGMA foreign_keys=ON")
        .execute(&pool)
        .await
        .unwrap();
    ganbaru_db::run_migrations(&pool).await.unwrap();
    sqlx::query("INSERT INTO project_groups (id, name) VALUES ('group-chat', 'Chat')")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO projects (id, group_id, name) VALUES ('project-chat', 'group-chat', 'Chat')",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO project_working_folders
            (id, project_id, display_name, kind, managed_relative_path,
             repository_kind, sort_order, created_at, updated_at)
         VALUES ('workspace-1', 'project-chat', 'Project files', 'managed',
                 'projects/project-chat', 'none', 0, ?, ?)",
    )
    .bind(NOW)
    .bind(NOW)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO chat_threads
            (id, project_id, working_folder_id, title, provider_family_id, provider_instance_id,
             continuation_group_id, safety_mode, interaction_mode, state,
             last_activity_at, created_at, updated_at)
         VALUES ('thread-1', 'project-chat', 'workspace-1', 'Chat', 'codex', 'codex-personal',
                 'continuation-1', 'ask_for_approval', 'build', 'idle', ?, ?, ?)",
    )
    .bind(NOW)
    .bind(NOW)
    .bind(NOW)
    .execute(&pool)
    .await
    .unwrap();
    pool
}

fn content_event(event_id: &str, delta: &str) -> AppendCanonicalEventRequest {
    AppendCanonicalEventRequest {
        runtime: CanonicalRuntimeEvent {
            schema_version: 1,
            event_id: ChatEventId::new(event_id).unwrap(),
            provider_family_id: ProviderFamilyId::new("codex").unwrap(),
            provider_instance_id: ProviderInstanceId::new("codex-personal").unwrap(),
            thread_id: ChatThreadId::new("thread-1").unwrap(),
            created_at: UtcTimestamp::new(NOW).unwrap(),
            turn_id: None,
            provider_turn_id: None,
            provider_item_id: None,
            provider_request_id: None,
            provider_task_id: None,
            provider_reference: None,
            event: CanonicalEvent::ContentDelta(ContentDeltaEvent {
                item_id: "assistant-message-1".to_string(),
                stream_kind: ContentStreamKind::AssistantText,
                content_index: 0,
                delta: delta.to_string(),
            }),
            redacted_diagnostic: None,
        },
        ingested_at: UtcTimestamp::new(NOW).unwrap(),
        diagnostic_expires_at: None,
    }
}

fn canonical_request(
    event_id: &str,
    turn_id: Option<&str>,
    event: CanonicalEvent,
) -> AppendCanonicalEventRequest {
    AppendCanonicalEventRequest {
        runtime: CanonicalRuntimeEvent {
            schema_version: 1,
            event_id: ChatEventId::new(event_id).unwrap(),
            provider_family_id: ProviderFamilyId::new("codex").unwrap(),
            provider_instance_id: ProviderInstanceId::new("codex-personal").unwrap(),
            thread_id: ChatThreadId::new("thread-1").unwrap(),
            created_at: UtcTimestamp::new(NOW).unwrap(),
            turn_id: turn_id.map(|value| ChatTurnId::new(value).unwrap()),
            provider_turn_id: None,
            provider_item_id: None,
            provider_request_id: None,
            provider_task_id: None,
            provider_reference: None,
            event,
            redacted_diagnostic: None,
        },
        ingested_at: UtcTimestamp::new(NOW).unwrap(),
        diagnostic_expires_at: None,
    }
}
