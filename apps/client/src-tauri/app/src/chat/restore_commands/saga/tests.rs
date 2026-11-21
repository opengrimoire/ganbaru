use super::*;
use crate::chat::tests::fake_driver::{
    FakeDriverControl, RecordingEventSink, fake_driver, start_request,
};
use crate::chat::tests::repository::{canonical_request, pool_with_thread};
use ganbaru_chat::repository::events::append_canonical_event;
use ganbaru_chat_contracts::events::{NotificationEvent, TurnStartedEvent};
use ganbaru_chat_contracts::models::{
    ChatCommandId, ChatTurnState, InteractionMode, SafetyMode, TurnModeSnapshot,
};
use ganbaru_chat_providers::ProviderEventSink;
use std::sync::Arc;

const NOW: &str = "2026-07-20T12:30:00Z";
const CHECKPOINT_OID: &str = "1111111111111111111111111111111111111111";
const CURRENT_OID: &str = "2222222222222222222222222222222222222222";

/// Appends one Codex event to `turn_id`, with `codex_turn_id` as the native rewind point.
async fn append_codex_event(
    pool: &SqlitePool,
    event_id: &str,
    turn_id: &str,
    codex_turn_id: Option<&str>,
    event: CanonicalEvent,
) {
    let mut request = canonical_request(event_id, Some(turn_id), event);
    let reference = match codex_turn_id {
        Some(codex_turn_id) => json!({ "method": "turn/started", "turnId": codex_turn_id }),
        None => json!({ "method": "thread/tokenUsage/updated" }),
    };
    request.runtime.provider_reference = Some(VersionedJson {
        schema_version: 1,
        value: reference,
    });
    append_canonical_event(pool, request).await.unwrap();
}

fn turn_started() -> CanonicalEvent {
    CanonicalEvent::TurnStarted(TurnStartedEvent {
        provider_turn_id: None,
        state: ChatTurnState::Active,
        modes: TurnModeSnapshot {
            safety_mode: SafetyMode::AskForApproval,
            interaction_mode: InteractionMode::Build,
        },
        model_id: None,
        model_options: Vec::new(),
    })
}

fn turn_warning() -> CanonicalEvent {
    CanonicalEvent::RuntimeWarning(NotificationEvent {
        code: "provider_warning".to_string(),
        title: "Provider warning".to_string(),
        detail: None,
    })
}

/// Two Codex turns: the checkpoint keeps the first, whose native turn is `codex-turn-a`.
async fn pool_with_two_codex_turns(first_turn_has_cursor: bool) -> SqlitePool {
    let pool = pool_with_thread().await;
    let first_cursor = first_turn_has_cursor.then_some("codex-turn-a");
    append_codex_event(
        &pool,
        "event-turn-1",
        "turn-1",
        first_cursor,
        turn_started(),
    )
    .await;
    append_codex_event(&pool, "event-turn-1-usage", "turn-1", None, turn_warning()).await;
    append_codex_event(
        &pool,
        "event-turn-2",
        "turn-2",
        Some("codex-turn-b"),
        turn_started(),
    )
    .await;
    pool
}

async fn insert_checkpoint_and_preview(
    pool: &SqlitePool,
) -> (StoredCheckpoint, StoredRestorePreview) {
    sqlx::query(
        "INSERT INTO chat_checkpoints
            (id, thread_id, turn_count, repository_identity, hidden_ref_name, git_object_id,
             status, created_at)
         VALUES ('checkpoint-1', 'thread-1', 1, 'repository-1',
                 'refs/ganbaru/chat/checkpoint-1', ?, 'available', ?)",
    )
    .bind(CHECKPOINT_OID)
    .bind(NOW)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO chat_restore_previews
            (id, thread_id, checkpoint_id, expected_thread_revision, repository_identity,
             current_worktree_tree_oid, current_index_tree_oid, current_index_fingerprint,
             affected_files_data, state, expires_at, created_at, updated_at)
         VALUES ('preview-1', 'thread-1', 'checkpoint-1', 0, 'repository-1', ?, ?,
                 'fingerprint-0000001', '[]', 'executing', '2026-07-20T12:45:00Z', ?, ?)",
    )
    .bind(CURRENT_OID)
    .bind(CURRENT_OID)
    .bind(NOW)
    .bind(NOW)
    .execute(pool)
    .await
    .unwrap();
    let target = StoredCheckpoint {
        id: ChatCheckpointId::new("checkpoint-1").unwrap(),
        thread_id: ChatThreadId::new("thread-1").unwrap(),
        turn_count: 1,
        repository_identity: "repository-1".to_string(),
        hidden_ref_name: "refs/ganbaru/chat/checkpoint-1".to_string(),
        git_object_id: CHECKPOINT_OID.to_string(),
        index_tree_oid: CHECKPOINT_OID.to_string(),
        worktree_tree_oid: CHECKPOINT_OID.to_string(),
        head_oid: None,
        head_ref: None,
    };
    let preview = StoredRestorePreview {
        id: "preview-1".to_string(),
        thread_id: target.thread_id.clone(),
        checkpoint_id: target.id.clone(),
        expected_thread_revision: 0,
        repository_identity: "repository-1".to_string(),
        current: CurrentGitSnapshot {
            worktree_commit_oid: CURRENT_OID.to_string(),
            worktree_tree_oid: CURRENT_OID.to_string(),
            index_tree_oid: CURRENT_OID.to_string(),
            index_fingerprint: "fingerprint-0000001".to_string(),
            head_oid: None,
            head_ref: None,
        },
        expires_at: UtcTimestamp::new("2026-07-20T12:45:00Z").unwrap(),
    };
    (target, preview)
}

fn restore_request() -> ExecuteChatRestoreRequest {
    ExecuteChatRestoreRequest {
        command: ChatCommandContext {
            client_command_id: ChatCommandId::new("restore-command").unwrap(),
            expected_thread_revision: None,
        },
        thread_id: ChatThreadId::new("thread-1").unwrap(),
        preview_id: "preview-1".to_string(),
        confirmed: true,
    }
}

async fn owner_with_session(control: &Arc<FakeDriverControl>) -> Arc<ThreadRuntimeOwner> {
    let owner = ChatRuntimeRegistry::default()
        .owner(ChatThreadId::new("thread-1").unwrap())
        .unwrap();
    let sink: Arc<dyn ProviderEventSink> = Arc::new(RecordingEventSink::new(Arc::clone(control)));
    owner
        .start_session(
            fake_driver(Arc::clone(control)),
            start_request(),
            sink,
            operation_context("restore-start", Duration::from_secs(1)),
        )
        .await
        .unwrap();
    owner
}

async fn invalidated_turn_events(pool: &SqlitePool, turn_id: &str) -> (i64, i64) {
    let row = sqlx::query(
        "SELECT COUNT(*) AS total, COUNT(invalidated_at) AS invalidated
         FROM chat_events WHERE thread_id = 'thread-1' AND turn_id = ?",
    )
    .bind(turn_id)
    .fetch_one(pool)
    .await
    .unwrap();
    (row.get("total"), row.get("invalidated"))
}

#[test]
fn codex_restore_rewinds_to_the_kept_turn_and_invalidates_only_later_turns() {
    tauri::async_runtime::block_on(async {
        let pool = pool_with_two_codex_turns(true).await;
        let (target, preview) = insert_checkpoint_and_preview(&pool).await;
        let control = Arc::new(FakeDriverControl::default());
        control.enable_native_rollback();
        let owner = owner_with_session(&control).await;
        let request = restore_request();
        let mut failure_phase = RestoreFailurePhase::SafeRetry;

        let alignment =
            align_provider_history(&owner, &pool, &request, &target, &mut failure_phase)
                .await
                .unwrap();

        assert_eq!(alignment.action, "rolled_back");
        assert!(alignment.had_session);
        assert_eq!(failure_phase, RestoreFailurePhase::ProviderRollback);
        let rollbacks = control.rollback_requests();
        assert_eq!(rollbacks.len(), 1);
        assert_eq!(
            rollbacks[0].target_turn_id.as_ref().map(ChatTurnId::as_str),
            Some("turn-1")
        );
        assert_eq!(rollbacks[0].checkpoint_id.as_ref(), Some(&target.id));
        let cursor = rollbacks[0].provider_cursor.as_ref().unwrap();
        assert_eq!(cursor.schema_version, 1);
        assert_eq!(
            cursor.value["turnId"], "codex-turn-a",
            "the latest turn event carrying a Codex turn id is the rewind point"
        );

        let reverted = persist_restore(
            &pool,
            &request,
            &preview,
            &target,
            alignment.action,
            &UtcTimestamp::new(NOW).unwrap(),
        )
        .await
        .unwrap();

        assert_eq!(
            reverted.iter().map(ChatTurnId::as_str).collect::<Vec<_>>(),
            ["turn-2"]
        );
        assert_eq!(invalidated_turn_events(&pool, "turn-1").await, (2, 0));
        assert_eq!(invalidated_turn_events(&pool, "turn-2").await, (1, 1));
        let recorded_action: Option<String> =
            sqlx::query_scalar("SELECT provider_history_action FROM chat_restore_operations")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(recorded_action.as_deref(), Some("rolled_back"));
    });
}

#[test]
fn codex_restore_without_a_native_turn_cursor_forks_the_conversation() {
    tauri::async_runtime::block_on(async {
        let pool = pool_with_two_codex_turns(false).await;
        let (target, _preview) = insert_checkpoint_and_preview(&pool).await;
        let control = Arc::new(FakeDriverControl::default());
        control.enable_native_rollback();
        let owner = owner_with_session(&control).await;
        let mut failure_phase = RestoreFailurePhase::SafeRetry;

        let alignment = align_provider_history(
            &owner,
            &pool,
            &restore_request(),
            &target,
            &mut failure_phase,
        )
        .await
        .unwrap();

        assert_eq!(alignment.action, "fork_required");
        assert!(alignment.had_session);
        assert_eq!(failure_phase, RestoreFailurePhase::WorkspaceRestore);
        assert!(control.rollback_requests().is_empty());
    });
}

#[test]
fn restore_without_native_rollback_support_forks_the_conversation() {
    tauri::async_runtime::block_on(async {
        let pool = pool_with_two_codex_turns(true).await;
        let (target, _preview) = insert_checkpoint_and_preview(&pool).await;
        let control = Arc::new(FakeDriverControl::default());
        let owner = owner_with_session(&control).await;
        let mut failure_phase = RestoreFailurePhase::SafeRetry;

        let alignment = align_provider_history(
            &owner,
            &pool,
            &restore_request(),
            &target,
            &mut failure_phase,
        )
        .await
        .unwrap();

        assert_eq!(alignment.action, "fork_required");
        assert!(control.rollback_requests().is_empty());
    });
}
