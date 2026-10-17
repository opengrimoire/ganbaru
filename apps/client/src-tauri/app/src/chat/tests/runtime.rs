use crate::chat::ingestion::{ChatChangeEmitter, ChatEventIngestor};
use crate::chat::tests::fake_driver::{
    FakeDriverControl, RecordingEventSink, approval_request, fake_driver, operation_context,
    send_request, start_request,
};
use crate::chat::workspace::mutation::{
    ChatWorkspaceMutationRegistry, ProviderTurnReservationHandoff,
};
use ganbaru_chat::repository::events::AppendCanonicalEventRequest;
use ganbaru_chat::repository::rebuild::rebuild_thread_projections;
use ganbaru_chat::repository::recovery::recover_orphaned_turns;
use ganbaru_chat::runtime::{ChatRuntimeRegistry, ThreadRuntimeCommand, ThreadRuntimeSnapshot};
use ganbaru_chat_contracts::events::CanonicalEvent;
use ganbaru_chat_contracts::models::{ChatChangeNotification, ChatError, ChatResult, UtcTimestamp};
use ganbaru_chat_contracts::models::{
    ChatCheckpointId, ChatCommandContext, ChatCommandId, ChatErrorCode, ChatThreadId,
    ProviderSessionState, RollbackRequest,
};
use ganbaru_chat_providers::ProviderEventSink;
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

struct NoopEmitter;

impl ChatChangeEmitter for NoopEmitter {
    fn emit(&self, _notification: &ChatChangeNotification) -> ChatResult<()> {
        Ok(())
    }
}

struct DurableTestSink {
    ingestor: tokio::sync::Mutex<ChatEventIngestor>,
}

impl DurableTestSink {
    fn new(pool: sqlx::SqlitePool) -> Self {
        Self {
            ingestor: tokio::sync::Mutex::new(ChatEventIngestor::new(pool, Arc::new(NoopEmitter))),
        }
    }
}

impl ProviderEventSink for DurableTestSink {
    fn emit<'a>(
        &'a self,
        event: ganbaru_chat_contracts::events::CanonicalRuntimeEvent,
    ) -> ganbaru_chat_providers::DriverFuture<'a, ()> {
        Box::pin(async move {
            self.ingestor
                .lock()
                .await
                .ingest(AppendCanonicalEventRequest {
                    runtime: event,
                    ingested_at: UtcTimestamp::new("2026-07-20T12:00:00Z")
                        .map_err(ChatError::driver_unavailable)?,
                    diagnostic_expires_at: None,
                })
                .await
        })
    }

    fn flush(&self) -> ganbaru_chat_providers::DriverFuture<'_, ()> {
        Box::pin(async move { self.ingestor.lock().await.flush().await })
    }
}

fn turn_reservation(thread_id: &str, turn_id: &str) -> ProviderTurnReservationHandoff {
    let registry = ChatWorkspaceMutationRegistry::default();
    registry
        .begin_provider_turn(
            Path::new("/tmp/ganbaru-runtime-reservation-test"),
            &ChatThreadId::new(thread_id).unwrap(),
            &ganbaru_chat_contracts::models::ChatTurnId::new(turn_id).unwrap(),
        )
        .unwrap()
        .handoff_to_runtime()
        .unwrap()
}

#[test]
fn registry_returns_one_owner_and_one_operation_lock_per_thread() {
    tauri::async_runtime::block_on(async {
        let registry = ChatRuntimeRegistry::default();
        let thread_id = ChatThreadId::new("thread-runtime").unwrap();
        let first = registry.owner(thread_id.clone()).unwrap();
        let second = registry.owner(thread_id).unwrap();
        assert!(Arc::ptr_eq(&first, &second));

        let guard = first.lock_operation().await;
        assert!(
            tokio::time::timeout(Duration::from_millis(10), second.lock_operation())
                .await
                .is_err()
        );
        drop(guard);
        tokio::time::timeout(Duration::from_millis(50), second.lock_operation())
            .await
            .unwrap();
    });
}

#[test]
fn bounded_runtime_queue_reports_busy_without_dropping_the_first_command() {
    tauri::async_runtime::block_on(async {
        let registry = ChatRuntimeRegistry::default();
        let (owner, start_worker) = registry
            .owner_for_test(ChatThreadId::new("thread-busy").unwrap(), 1)
            .unwrap();
        owner
            .try_command(ThreadRuntimeCommand::SessionState(
                ProviderSessionState::Ready,
            ))
            .unwrap();
        let error = owner.try_command(ThreadRuntimeCommand::Touch).unwrap_err();
        assert_eq!(error.code, ChatErrorCode::Busy);
        start_worker.send(()).unwrap();
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert_eq!(
            owner.snapshot().unwrap().session_state,
            ProviderSessionState::Ready
        );
    });
}

#[test]
fn idle_stop_requires_ready_unprotected_session_and_shutdown_is_visible() {
    tauri::async_runtime::block_on(async {
        let now = Instant::now();
        let mut snapshot = ThreadRuntimeSnapshot {
            session_id: None,
            session_state: ProviderSessionState::Ready,
            capabilities: ganbaru_chat_contracts::models::ProviderCapabilities::default(),
            active_turn_id: None,
            turn_active: false,
            pending_request: false,
            terminal_linked_operation: false,
            accepting_commands: true,
            generation: 1,
            last_activity: now - Duration::from_secs(60),
        };
        assert!(snapshot.can_idle_stop(now, Duration::from_secs(30)));
        snapshot.turn_active = true;
        assert!(!snapshot.can_idle_stop(now, Duration::from_secs(30)));
        snapshot.turn_active = false;
        snapshot.pending_request = true;
        assert!(!snapshot.can_idle_stop(now, Duration::from_secs(30)));

        let registry = ChatRuntimeRegistry::default();
        let mutations = ChatWorkspaceMutationRegistry::default();
        let owner = registry
            .owner(ChatThreadId::new("thread-shutdown").unwrap())
            .unwrap();
        registry
            .shutdown_and_wait(Duration::from_millis(50), &mutations)
            .await
            .unwrap();
        let stopped = owner.snapshot().unwrap();
        assert!(!stopped.accepting_commands);
        assert_eq!(stopped.session_state, ProviderSessionState::Stopped);
    });
}

#[test]
fn maintenance_stop_drains_owners_and_allows_new_sessions() {
    tauri::async_runtime::block_on(async {
        let registry = ChatRuntimeRegistry::default();
        let mutations = ChatWorkspaceMutationRegistry::default();
        let thread_id = ChatThreadId::new("thread-maintenance").unwrap();
        let previous = registry.owner(thread_id.clone()).unwrap();
        mutations
            .begin_provider_turn(
                Path::new("/tmp/ganbaru-runtime-maintenance-test"),
                &thread_id,
                &ganbaru_chat_contracts::models::ChatTurnId::new("turn-maintenance").unwrap(),
            )
            .unwrap()
            .handoff_to_runtime()
            .unwrap()
            .handoff_to_event_sink();
        assert!(
            mutations
                .try_mutation(Path::new("/tmp/ganbaru-runtime-maintenance-test"))
                .is_err()
        );
        assert_eq!(registry.process_counts().unwrap(), (0, 0));
        assert_eq!(
            registry
                .stop_all_and_reset(Duration::from_millis(50), &mutations)
                .await
                .unwrap(),
            0
        );
        assert!(
            mutations
                .try_mutation(Path::new("/tmp/ganbaru-runtime-maintenance-test"))
                .is_ok()
        );
        assert!(!previous.snapshot().unwrap().accepting_commands);
        let replacement = registry.owner(thread_id).unwrap();
        assert!(replacement.snapshot().unwrap().accepting_commands);
        assert!(!Arc::ptr_eq(&previous, &replacement));
        registry
            .shutdown_and_wait(Duration::from_millis(50), &mutations)
            .await
            .unwrap();
    });
}

#[test]
fn permanent_thread_shutdown_removes_the_owner_and_releases_its_workspace() {
    tauri::async_runtime::block_on(async {
        let registry = ChatRuntimeRegistry::default();
        let mutations = ChatWorkspaceMutationRegistry::default();
        let thread_id = ChatThreadId::new("thread-delete").unwrap();
        let previous = registry.owner(thread_id.clone()).unwrap();
        let root = Path::new("/tmp/ganbaru-runtime-delete-test");
        mutations
            .begin_provider_turn(
                root,
                &thread_id,
                &ganbaru_chat_contracts::models::ChatTurnId::new("turn-delete").unwrap(),
            )
            .unwrap()
            .handoff_to_runtime()
            .unwrap()
            .handoff_to_event_sink();

        registry
            .shutdown_thread_and_remove(&thread_id, Duration::from_millis(50), &mutations)
            .await
            .unwrap();

        assert!(mutations.try_mutation(root).is_ok());
        assert!(!previous.snapshot().unwrap().accepting_commands);
        let replacement = registry.owner(thread_id).unwrap();
        assert!(!Arc::ptr_eq(&previous, &replacement));
        registry
            .shutdown_and_wait(Duration::from_millis(50), &mutations)
            .await
            .unwrap();
    });
}

#[test]
fn fake_driver_streams_approval_stops_restarts_and_rejects_late_events() {
    tauri::async_runtime::block_on(async {
        let registry = ChatRuntimeRegistry::default();
        let owner = registry
            .owner(ChatThreadId::new("thread-1").unwrap())
            .unwrap();
        let first = Arc::new(FakeDriverControl::default());
        let sink: Arc<dyn ProviderEventSink> =
            Arc::new(RecordingEventSink::new(Arc::clone(&first)));
        owner
            .start_session(
                fake_driver(Arc::clone(&first)),
                start_request(),
                sink,
                operation_context("start", Duration::from_secs(1)),
            )
            .await
            .unwrap();
        let cross_thread_error = first.emit_cross_thread().await.unwrap_err();
        assert_eq!(cross_thread_error.code, ChatErrorCode::Conflict);
        assert!(first.events().iter().any(|event| {
            event.thread_id.as_str() == "thread-1"
                && matches!(&event.event, CanonicalEvent::RuntimeWarning(warning) if warning.code == "late_provider_event")
        }));
        owner
            .send_turn(
                send_request("fake-turn"),
                turn_reservation("thread-1", "fake-turn"),
                operation_context("send", Duration::from_secs(1)),
            )
            .await
            .unwrap();
        assert_eq!(
            owner.snapshot().unwrap().session_state,
            ProviderSessionState::WaitingForApproval
        );
        owner
            .resolve_approval(
                approval_request("fake-turn"),
                operation_context("approve", Duration::from_secs(1)),
            )
            .await
            .unwrap();
        assert_eq!(
            owner.snapshot().unwrap().session_state,
            ProviderSessionState::Ready
        );
        let late_turn_error = first.emit_late_content("fake-turn").await.unwrap_err();
        assert_eq!(late_turn_error.code, ChatErrorCode::Conflict);
        assert!(first.events().iter().any(|event| {
            matches!(&event.event, CanonicalEvent::RuntimeWarning(warning) if warning.code == "late_provider_event")
        }));

        owner
            .send_turn(
                send_request("stop-turn"),
                turn_reservation("thread-1", "stop-turn"),
                operation_context("send-before-stop", Duration::from_secs(1)),
            )
            .await
            .unwrap();
        owner
            .stop_session(false, operation_context("stop", Duration::from_secs(1)))
            .await
            .unwrap();
        assert!(first.events().iter().any(|event| {
            matches!(&event.event, CanonicalEvent::ContentDelta(delta) if delta.delta.contains("while stopping"))
        }));
        let late_error = first.emit_late().await.unwrap_err();
        assert_eq!(late_error.code, ChatErrorCode::Conflict);

        let crashed = Arc::new(FakeDriverControl::default());
        crashed.crash_next_send();
        let sink: Arc<dyn ProviderEventSink> =
            Arc::new(RecordingEventSink::new(Arc::clone(&crashed)));
        owner
            .start_session(
                fake_driver(Arc::clone(&crashed)),
                start_request(),
                sink,
                operation_context("restart", Duration::from_secs(1)),
            )
            .await
            .unwrap();
        assert!(
            owner
                .send_turn(
                    send_request("crash-turn"),
                    turn_reservation("thread-1", "crash-turn"),
                    operation_context("crash", Duration::from_secs(1)),
                )
                .await
                .is_err()
        );
        assert_eq!(
            owner.snapshot().unwrap().session_state,
            ProviderSessionState::Failed
        );

        let restarted = Arc::new(FakeDriverControl::default());
        let sink: Arc<dyn ProviderEventSink> =
            Arc::new(RecordingEventSink::new(Arc::clone(&restarted)));
        owner
            .start_session(
                fake_driver(restarted),
                start_request(),
                sink,
                operation_context("restart-after-crash", Duration::from_secs(1)),
            )
            .await
            .unwrap();
        assert_eq!(
            owner.snapshot().unwrap().session_state,
            ProviderSessionState::Ready
        );
    });
}

#[test]
fn unsupported_provider_rollback_fails_without_corrupting_the_session() {
    tauri::async_runtime::block_on(async {
        let registry = ChatRuntimeRegistry::default();
        let owner = registry
            .owner(ChatThreadId::new("thread-1").unwrap())
            .unwrap();
        let control = Arc::new(FakeDriverControl::default());
        let sink: Arc<dyn ProviderEventSink> =
            Arc::new(RecordingEventSink::new(Arc::clone(&control)));
        let session = owner
            .start_session(
                fake_driver(control),
                start_request(),
                sink,
                operation_context("rollback-start", Duration::from_secs(1)),
            )
            .await
            .unwrap();
        let error = owner
            .rollback(
                RollbackRequest {
                    command: ChatCommandContext {
                        client_command_id: ChatCommandId::new("rollback-command").unwrap(),
                        expected_thread_revision: Some(1),
                    },
                    session_id: session.session_id,
                    checkpoint_id: Some(ChatCheckpointId::new("checkpoint-1").unwrap()),
                    provider_cursor: None,
                    target_turn_id: None,
                },
                operation_context("rollback", Duration::from_secs(1)),
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, ChatErrorCode::CapabilityUnsupported);
        assert_eq!(
            owner.snapshot().unwrap().session_state,
            ProviderSessionState::Ready
        );
    });
}

#[test]
fn fake_driver_crash_flushes_and_rebuilds_durable_output() {
    tauri::async_runtime::block_on(async {
        let pool = crate::chat::tests::repository::pool_with_thread().await;
        let registry = ChatRuntimeRegistry::default();
        let owner = registry
            .owner(ChatThreadId::new("thread-1").unwrap())
            .unwrap();
        let control = Arc::new(FakeDriverControl::default());
        control.crash_next_send();
        let sink: Arc<dyn ProviderEventSink> = Arc::new(DurableTestSink::new(pool.clone()));
        owner
            .start_session(
                fake_driver(Arc::clone(&control)),
                start_request(),
                sink,
                operation_context("durable-start", Duration::from_secs(1)),
            )
            .await
            .unwrap();
        assert!(
            owner
                .send_turn(
                    send_request("durable-crash-turn"),
                    turn_reservation("thread-1", "durable-crash-turn"),
                    operation_context("durable-send", Duration::from_secs(1)),
                )
                .await
                .is_err()
        );
        owner
            .stop_session(
                true,
                operation_context("durable-stop", Duration::from_secs(1)),
            )
            .await
            .unwrap();
        let before: String = sqlx::query_scalar(
            "SELECT normalized_markdown FROM chat_messages WHERE id = 'fake-assistant-item'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(before.contains("Streaming response"));
        recover_orphaned_turns(
            &pool,
            &HashSet::new(),
            &UtcTimestamp::new("2026-07-20T12:00:03Z").unwrap(),
        )
        .await
        .unwrap();
        rebuild_thread_projections(&pool, &ChatThreadId::new("thread-1").unwrap())
            .await
            .unwrap();
        let after: String = sqlx::query_scalar(
            "SELECT normalized_markdown FROM chat_messages WHERE id = 'fake-assistant-item'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(after, before);
    });
}

#[test]
fn output_emitted_before_a_hung_stop_is_flushed_durably() {
    tauri::async_runtime::block_on(async {
        let pool = crate::chat::tests::repository::pool_with_thread().await;
        let registry = ChatRuntimeRegistry::default();
        let owner = registry
            .owner(ChatThreadId::new("thread-1").unwrap())
            .unwrap();
        let control = Arc::new(FakeDriverControl::default());
        let sink: Arc<dyn ProviderEventSink> = Arc::new(DurableTestSink::new(pool.clone()));
        owner
            .start_session(
                fake_driver(Arc::clone(&control)),
                start_request(),
                sink,
                operation_context("hung-output-start", Duration::from_secs(1)),
            )
            .await
            .unwrap();
        owner
            .send_turn(
                send_request("hung-output-turn"),
                turn_reservation("thread-1", "hung-output-turn"),
                operation_context("hung-output-send", Duration::from_secs(1)),
            )
            .await
            .unwrap();
        control.hang_on_stop();
        assert!(
            owner
                .stop_session(
                    true,
                    operation_context("hung-output-stop", Duration::from_millis(120)),
                )
                .await
                .is_err()
        );
        let text: String = sqlx::query_scalar(
            "SELECT normalized_markdown FROM chat_messages WHERE id = 'fake-assistant-item'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(text.contains("Streaming response"));
        assert!(text.contains("Output received while stopping"));
    });
}

#[test]
fn simultaneous_window_commands_share_one_serial_driver_operation() {
    tauri::async_runtime::block_on(async {
        let registry = ChatRuntimeRegistry::default();
        let owner = registry
            .owner(ChatThreadId::new("thread-1").unwrap())
            .unwrap();
        let detached = registry
            .owner(ChatThreadId::new("thread-1").unwrap())
            .unwrap();
        let control = Arc::new(FakeDriverControl::default());
        control.set_send_delay(Duration::from_millis(30));
        let sink: Arc<dyn ProviderEventSink> =
            Arc::new(RecordingEventSink::new(Arc::clone(&control)));
        owner
            .start_session(
                fake_driver(Arc::clone(&control)),
                start_request(),
                sink,
                operation_context("start", Duration::from_secs(1)),
            )
            .await
            .unwrap();

        let first = tauri::async_runtime::spawn(async move {
            owner
                .send_turn(
                    send_request("window-one"),
                    turn_reservation("thread-1", "window-one"),
                    operation_context("window-one", Duration::from_secs(1)),
                )
                .await
        });
        let second = tauri::async_runtime::spawn(async move {
            detached
                .send_turn(
                    send_request("window-two"),
                    turn_reservation("thread-1", "window-two"),
                    operation_context("window-two", Duration::from_secs(1)),
                )
                .await
        });
        let results = [first.await.unwrap(), second.await.unwrap()];
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(results.iter().filter(|result| result.is_err()).count(), 1);
        assert_eq!(control.maximum_sends_in_flight(), 1);
    });
}

#[test]
fn ready_session_stops_lazily_and_hung_shutdown_stays_bounded() {
    tauri::async_runtime::block_on(async {
        let idle_registry =
            ChatRuntimeRegistry::with_idle_timeout_for_test(Duration::from_millis(20));
        let idle_owner = idle_registry
            .owner(ChatThreadId::new("thread-1").unwrap())
            .unwrap();
        let idle_control = Arc::new(FakeDriverControl::default());
        let sink: Arc<dyn ProviderEventSink> =
            Arc::new(RecordingEventSink::new(Arc::clone(&idle_control)));
        idle_owner
            .start_session(
                fake_driver(idle_control),
                start_request(),
                sink,
                operation_context("start-idle", Duration::from_secs(1)),
            )
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(60)).await;
        assert_eq!(
            idle_owner.snapshot().unwrap().session_state,
            ProviderSessionState::Stopped
        );

        let registry = ChatRuntimeRegistry::default();
        let mutations = ChatWorkspaceMutationRegistry::default();
        let owner = registry
            .owner(ChatThreadId::new("thread-1").unwrap())
            .unwrap();
        let hung = Arc::new(FakeDriverControl::default());
        hung.hang_on_stop();
        let sink: Arc<dyn ProviderEventSink> = Arc::new(RecordingEventSink::new(Arc::clone(&hung)));
        owner
            .start_session(
                fake_driver(hung),
                start_request(),
                sink,
                operation_context("start-hung", Duration::from_secs(1)),
            )
            .await
            .unwrap();
        let started = Instant::now();
        assert!(
            registry
                .shutdown_and_wait(Duration::from_millis(40), &mutations)
                .await
                .is_err()
        );
        assert!(started.elapsed() < Duration::from_millis(250));
        assert!(!owner.snapshot().unwrap().accepting_commands);
    });
}
