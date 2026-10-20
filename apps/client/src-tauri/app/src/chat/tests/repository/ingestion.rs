use super::*;

#[test]
fn ingestion_batches_adjacent_deltas_and_notifies_only_after_commit() {
    tauri::async_runtime::block_on(async {
        let pool = pool_with_thread().await;
        let emitter = Arc::new(RecordingEmitter::default());
        let mut ingestor = ChatEventIngestor::new(pool.clone(), emitter.clone());
        ingestor
            .ingest(content_event("event-batch-1", "Hello "))
            .await
            .unwrap();
        ingestor
            .ingest(content_event("event-batch-2", "world"))
            .await
            .unwrap();
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM chat_events")
                .fetch_one(&pool)
                .await
                .unwrap(),
            0
        );
        assert!(emitter.0.lock().unwrap().is_empty());
        ingestor.flush().await.unwrap();
        let text: String = sqlx::query_scalar(
            "SELECT normalized_markdown FROM chat_messages WHERE id = 'assistant-message-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(text, "Hello world");
        let notifications = emitter.0.lock().unwrap();
        assert_eq!(notifications.len(), 1);
        assert_eq!(notifications[0].sequence, 1);
    });
}

#[test]
fn ingestion_splits_delta_batches_at_the_memory_bound() {
    tauri::async_runtime::block_on(async {
        let pool = pool_with_thread().await;
        let emitter = Arc::new(RecordingEmitter::default());
        let mut ingestor = ChatEventIngestor::new(pool.clone(), emitter.clone());
        let first = "a".repeat(40 * 1024);
        let second = "b".repeat(40 * 1024);
        ingestor
            .ingest(content_event("event-bounded-1", &first))
            .await
            .unwrap();
        ingestor
            .ingest(content_event("event-bounded-2", &second))
            .await
            .unwrap();
        ingestor.flush().await.unwrap();
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM chat_events")
                .fetch_one(&pool)
                .await
                .unwrap(),
            2
        );
        let text: String = sqlx::query_scalar(
            "SELECT normalized_markdown FROM chat_messages WHERE id = 'assistant-message-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(text, format!("{first}{second}"));
        let preview: String =
            sqlx::query_scalar("SELECT latest_preview FROM chat_threads WHERE id = 'thread-1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(preview, "b".repeat(2000));
        assert_eq!(emitter.0.lock().unwrap().len(), 2);
    });
}

#[test]
fn ingestion_bounds_command_output_with_a_visible_notice() {
    tauri::async_runtime::block_on(async {
        let pool = pool_with_thread().await;
        let emitter = Arc::new(RecordingEmitter::default());
        let mut ingestor = ChatEventIngestor::new(pool.clone(), emitter);
        let mut first = content_event("event-output-1", &"a".repeat(2 * 1024 * 1024 - 10));
        let mut second = content_event("event-output-2", &"b".repeat(100));
        let mut discarded = content_event("event-output-3", "must not be retained");
        for request in [&mut first, &mut second, &mut discarded] {
            let CanonicalEvent::ContentDelta(delta) = &mut request.runtime.event else {
                unreachable!()
            };
            delta.item_id = "command-1".to_string();
            delta.stream_kind = ContentStreamKind::CommandOutput;
        }
        ingestor.ingest(first).await.unwrap();
        ingestor.ingest(second).await.unwrap();
        ingestor.ingest(discarded).await.unwrap();
        ingestor.flush().await.unwrap();
        let payloads: Vec<String> = sqlx::query_scalar(
            "SELECT payload_data FROM chat_events WHERE event_type = 'content_delta' ORDER BY sequence",
        ).fetch_all(&pool).await.unwrap();
        let retained = payloads
            .into_iter()
            .map(|payload| {
                serde_json::from_str::<serde_json::Value>(&payload).unwrap()["payload"]["delta"]
                    .as_str()
                    .unwrap()
                    .to_string()
            })
            .collect::<String>();
        assert!(retained.contains("[Output truncated at the 2 MiB Chat artifact limit]"));
        assert!(!retained.contains("must not be retained"));
        assert!(retained.len() <= 2 * 1024 * 1024 + 64);
    });
}

#[test]
fn ingestion_retains_a_pending_batch_after_append_failure() {
    tauri::async_runtime::block_on(async {
        let pool = pool_with_thread().await;
        let emitter = Arc::new(RecordingEmitter::default());
        let mut ingestor = ChatEventIngestor::new(pool.clone(), emitter.clone());
        ingestor
            .ingest(content_event("event-retry", "retained"))
            .await
            .unwrap();
        sqlx::query(
            "CREATE TRIGGER reject_chat_event
             BEFORE INSERT ON chat_events
             BEGIN SELECT RAISE(ABORT, 'fixture rejection'); END",
        )
        .execute(&pool)
        .await
        .unwrap();
        assert!(ingestor.flush().await.is_err());
        assert!(emitter.0.lock().unwrap().is_empty());
        sqlx::query("DROP TRIGGER reject_chat_event")
            .execute(&pool)
            .await
            .unwrap();
        ingestor.flush().await.unwrap();
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM chat_events")
                .fetch_one(&pool)
                .await
                .unwrap(),
            1
        );
        assert_eq!(emitter.0.lock().unwrap().len(), 1);
    });
}

#[test]
fn ingestion_rejects_diagnostics_that_are_not_redacted() {
    tauri::async_runtime::block_on(async {
        let pool = pool_with_thread().await;
        let emitter = Arc::new(RecordingEmitter::default());
        let mut ingestor = ChatEventIngestor::new(pool, emitter);
        let mut request = content_event("event-secret", "ignored");
        request.runtime.redacted_diagnostic = Some(VersionedJson {
            schema_version: 1,
            value: serde_json::json!({ "authorization": "Bearer private" }),
        });
        request.diagnostic_expires_at = Some(UtcTimestamp::new("2026-07-21T12:00:00Z").unwrap());
        assert!(ingestor.ingest(request).await.is_err());

        let mut request = content_event("event-home-path", "ignored");
        request.runtime.redacted_diagnostic = Some(VersionedJson {
            schema_version: 1,
            value: serde_json::json!({ "detail": "c:\\users\\person\\secret.txt" }),
        });
        request.diagnostic_expires_at = Some(UtcTimestamp::new("2026-07-21T12:00:00Z").unwrap());
        assert!(ingestor.ingest(request).await.is_err());
    });
}

#[test]
fn ingestion_caps_streamed_message_text_with_a_notice_and_warning() {
    tauri::async_runtime::block_on(async {
        let pool = pool_with_thread().await;
        let emitter = Arc::new(RecordingEmitter::default());
        let mut ingestor = ChatEventIngestor::new(pool.clone(), emitter);
        let chunk = "a".repeat(3 * 1024 * 1024);
        for index in 0..3 {
            ingestor
                .ingest(content_event(&format!("event-message-{index}"), &chunk))
                .await
                .unwrap();
        }
        ingestor
            .ingest(content_event(
                "event-message-dropped",
                "must not be retained",
            ))
            .await
            .unwrap();
        ingestor.flush().await.unwrap();
        let text: String = sqlx::query_scalar(
            "SELECT normalized_markdown FROM chat_messages WHERE id = 'assistant-message-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(text.ends_with("[Text truncated at the 8 MiB Chat message limit]\n"));
        assert!(!text.contains("must not be retained"));
        assert!(text.len() <= 8 * 1024 * 1024 + 64);
        assert_eq!(text_limit_warnings(&pool).await, 1);
    });
}

#[test]
fn ingestion_caps_streamed_text_across_the_items_of_one_turn() {
    tauri::async_runtime::block_on(async {
        let pool = pool_with_thread().await;
        let emitter = Arc::new(RecordingEmitter::default());
        let mut ingestor = ChatEventIngestor::new(pool.clone(), emitter);
        let chunk = "b".repeat(3 * 1024 * 1024 + 512 * 1024);
        for item in 0..6 {
            for part in 0..2 {
                let mut request = content_event(&format!("event-turn-{item}-{part}"), &chunk);
                let CanonicalEvent::ContentDelta(delta) = &mut request.runtime.event else {
                    unreachable!()
                };
                delta.item_id = format!("assistant-message-{item}");
                ingestor.ingest(request).await.unwrap();
            }
        }
        ingestor.flush().await.unwrap();
        let texts: Vec<String> = sqlx::query_scalar(
            "SELECT normalized_markdown FROM chat_messages ORDER BY sequence_anchor",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        let retained = texts.iter().map(String::len).sum::<usize>();
        assert_eq!(texts.len(), 5, "items after the turn cap are not stored");
        assert!(texts[4].ends_with("[Text truncated at the 32 MiB Chat turn limit]\n"));
        assert!(retained <= 32 * 1024 * 1024 + 64);
        assert_eq!(text_limit_warnings(&pool).await, 1);
    });
}

async fn text_limit_warnings(pool: &sqlx::SqlitePool) -> i64 {
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM chat_events
         WHERE event_type = 'runtime_warning' AND payload_data LIKE '%streamed_text_limit%'",
    )
    .fetch_one(pool)
    .await
    .unwrap()
}
