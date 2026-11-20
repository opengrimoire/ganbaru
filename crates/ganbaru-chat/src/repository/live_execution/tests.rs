use super::*;

const THREAD: &str = "thread:live";
const TURN: &str = "turn:live";
const ASSIGNMENT: &str = "assignment:live";
const DEVICE: &str = "device-desk";
const NOW: &str = "2026-11-19T09:00:00.000Z";

fn plain_measure(text: &str) -> usize {
    text.len()
}

fn budget(payload_bytes: usize) -> LiveExecutionBudget {
    LiveExecutionBudget {
        payload_bytes,
        measure: plain_measure,
    }
}

async fn migrated_pool() -> SqlitePool {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    ganbaru_db::test_support::apply_migrated_template(&pool)
        .await
        .unwrap();
    sqlx::raw_sql("PRAGMA foreign_keys=OFF")
        .execute(&pool)
        .await
        .unwrap();
    pool
}

/// A pool with the live thread, an earlier turn and the run's turn, and the assignment executed
/// by `executing_device` in `state` with one run in `run_state`.
async fn seeded_pool(executing_device: Option<&str>, state: &str, run_state: &str) -> SqlitePool {
    let pool = migrated_pool().await;
    // The thread needs no project, working folder, or execution environment rows.
    sqlx::raw_sql(
        "DROP TRIGGER chat_threads_assign_current_environment;
         DROP TRIGGER chat_threads_execution_target_insert;
         DROP TRIGGER chat_threads_project_matches_working_folder_insert;",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO chat_threads
            (id, project_id, working_folder_id, title, provider_family_id, provider_instance_id,
             continuation_group_id, safety_mode, interaction_mode, state,
             last_activity_at, created_at, updated_at)
         VALUES (?, 'project-live', 'folder-live', 'Live', 'codex', 'codex-personal',
                 'continuation-live', 'ask_for_approval', 'build', 'idle', ?, ?, ?)",
    )
    .bind(THREAD)
    .bind(NOW)
    .bind(NOW)
    .bind(NOW)
    .execute(&pool)
    .await
    .unwrap();
    for (turn, ordinal) in [("turn:earlier", 0), (TURN, 1)] {
        sqlx::query(
            "INSERT INTO chat_turns
                (id, thread_id, ordinal, state, safety_mode, interaction_mode,
                 model_selection_data, created_at, updated_at)
             VALUES (?, ?, ?, 'active', 'ask_for_approval', 'build',
                     '{\"modelId\":\"gpt-live\",\"options\":[]}', ?, ?)",
        )
        .bind(turn)
        .bind(THREAD)
        .bind(ordinal)
        .bind(NOW)
        .bind(NOW)
        .execute(&pool)
        .await
        .unwrap();
    }
    set_applying(&pool, true).await;
    sqlx::query(
        "INSERT INTO chat_work_assignments
            (id, reply_thread_id, agent_id, triggering_message_item_id, executing_device_id,
             state, model_selection_schema_version, model_selection_data, created_at, updated_at)
         VALUES (?, 'reply:live', 'participant:live', 'item:live', ?, ?, 1, '{}', ?, ?)",
    )
    .bind(ASSIGNMENT)
    .bind(executing_device)
    .bind(state)
    .bind(NOW)
    .bind(NOW)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO chat_agent_runs
            (id, assignment_id, project_id, authorization_revision_id,
             authorization_scope_digest, provider_turn_id, provider_thread_id, state,
             run_ordinal, created_at, updated_at)
         VALUES ('run:live', ?, 'project-live', 'authorization:live', ?, ?, ?, ?, 1, ?, ?)",
    )
    .bind(ASSIGNMENT)
    .bind("a".repeat(64))
    .bind(TURN)
    .bind(THREAD)
    .bind(run_state)
    .bind(NOW)
    .bind(NOW)
    .execute(&pool)
    .await
    .unwrap();
    set_applying(&pool, false).await;
    pool
}

async fn insert_message(pool: &SqlitePool, id: &str, turn: &str, role: &str, markdown: &str) {
    sqlx::query(
        "INSERT INTO chat_messages
            (id, thread_id, turn_id, sequence_anchor, role, normalized_markdown, streaming_state,
             provider_item_id, content_metadata_data, created_at, updated_at)
         VALUES (?, ?, ?, 1, ?, ?, 'streaming', 'provider-item-secret',
                 '{\"phase\":\"commentary\",\"attachments\":[{\"path\":\"/home/secret\"}]}', ?, ?)",
    )
    .bind(id)
    .bind(THREAD)
    .bind(turn)
    .bind(role)
    .bind(markdown)
    .bind(NOW)
    .bind(NOW)
    .execute(pool)
    .await
    .unwrap();
}

async fn insert_activity(pool: &SqlitePool, id: &str, kind: &str, detail: Option<&str>) {
    sqlx::query(
        "INSERT INTO chat_activities
            (id, thread_id, turn_id, sequence_anchor, item_kind, status, title, detail,
             provider_item_id, safe_metadata_data, source_event_type, created_at, updated_at)
         VALUES (?, ?, ?, 2, ?, 'completed', 'Run tests', ?, 'provider-item-secret',
                 '{\"toolName\":\"shell\",\"exitCode\":1,\"durationMs\":42.5,
                   \"cwd\":\"/home/secret/project\",\"changes\":[{\"path\":\"a.rs\"}],
                   \"toolInput\":{\"token\":\"secret\"},\"unlisted\":\"secret\"}',
                 'item_completed', ?, ?)",
    )
    .bind(id)
    .bind(THREAD)
    .bind(TURN)
    .bind(kind)
    .bind(detail)
    .bind(NOW)
    .bind(NOW)
    .execute(pool)
    .await
    .unwrap();
}

fn query(after_change: Option<u64>) -> LiveExecutionQuery<'static> {
    LiveExecutionQuery {
        assignment_id: ASSIGNMENT,
        own_device_id: DEVICE,
        after_change,
    }
}

async fn read(pool: &SqlitePool, after: Option<u64>, bytes: usize) -> ChatLiveExecutionRead {
    match read_live_execution(pool, query(after), budget(bytes))
        .await
        .unwrap()
    {
        LiveExecutionOutcome::Read(read) => *read,
        LiveExecutionOutcome::NotHere => panic!("the assignment runs here"),
    }
}

fn ids(read: &ChatLiveExecutionRead) -> Vec<&str> {
    read.items
        .iter()
        .map(|item| item.activity_id.as_str())
        .collect()
}

#[tokio::test]
async fn an_assignment_another_device_executes_is_not_served() {
    for device in [Some("device-other"), None] {
        let pool = seeded_pool(device, "working", "working").await;
        let outcome = read_live_execution(&pool, query(None), budget(600_000))
            .await
            .unwrap();
        assert_eq!(outcome, LiveExecutionOutcome::NotHere);
    }
    let pool = seeded_pool(Some(DEVICE), "working", "working").await;
    let unknown = LiveExecutionQuery {
        assignment_id: "assignment:unknown",
        ..query(None)
    };
    let outcome = read_live_execution(&pool, unknown, budget(600_000))
        .await
        .unwrap();
    assert_eq!(outcome, LiveExecutionOutcome::NotHere);
}

#[tokio::test]
async fn a_read_carries_only_the_run_turn_and_withholds_user_rows() {
    let pool = seeded_pool(Some(DEVICE), "working", "working").await;
    insert_message(&pool, "message:earlier", "turn:earlier", "assistant", "old").await;
    insert_message(&pool, "message:user", TURN, "user", "private prompt").await;
    insert_message(&pool, "message:reply", TURN, "assistant", "Working on it").await;
    insert_activity(&pool, "activity:user", "user_message", Some("private")).await;
    insert_activity(&pool, "activity:command", "command_execution", Some("ok")).await;

    let read = read(&pool, None, 600_000).await;

    assert_eq!(ids(&read), ["message:reply", "activity:command"]);
    assert_eq!(read.turn_id.as_ref().map(ChatTurnId::as_str), Some(TURN));
    assert_eq!(
        read.run_id
            .as_ref()
            .map(ganbaru_chat_contracts::models::ChatAgentRunId::as_str),
        Some("run:live")
    );
    assert_eq!(
        read.thread_id.as_ref().map(ChatThreadId::as_str),
        Some(THREAD)
    );
    assert_eq!(
        read.provider_family_id
            .as_ref()
            .map(ProviderFamilyId::as_str),
        Some("codex")
    );
    assert_eq!(
        read.model_id.as_ref().map(ModelId::as_str),
        Some("gpt-live")
    );
    assert_eq!(read.turns.len(), 1);
    assert!(read.turns[0].changed_files.is_empty());
    assert!(!read.settled);
    assert!(!read.has_more);
}

#[tokio::test]
async fn rows_carry_only_allowlisted_fields() {
    let pool = seeded_pool(Some(DEVICE), "working", "working").await;
    insert_message(&pool, "message:reply", TURN, "assistant", "Done").await;
    insert_activity(&pool, "activity:command", "command_execution", Some("ok")).await;

    let read = read(&pool, None, 600_000).await;
    let encoded = serde_json::to_string(&read).unwrap();

    assert!(!encoded.contains("secret"), "{encoded}");
    let message = read.items[0].data.value.as_object().unwrap();
    let mut keys = message.keys().map(String::as_str).collect::<Vec<_>>();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "createdAt",
            "markdown",
            "markdownTruncated",
            "metadata",
            "role",
            "streamingState",
            "updatedAt"
        ]
    );
    assert_eq!(message["metadata"], json!({ "phase": "commentary" }));
    let activity = &read.items[1].data.value;
    assert_eq!(
        activity["metadata"],
        json!({ "toolName": "shell", "exitCode": 1, "durationMs": 42 })
    );
    assert!(activity.get("providerItemId").is_none());
    assert!(activity.get("detail").is_none());
    assert_eq!(activity["detailPreview"], json!("ok"));
}

#[tokio::test]
async fn long_text_is_shortened_to_its_tail_or_preview() {
    let pool = seeded_pool(Some(DEVICE), "working", "working").await;
    let markdown = format!("{}end", "é".repeat(LIVE_MESSAGE_TAIL_BYTES));
    insert_message(&pool, "message:long", TURN, "assistant", &markdown).await;
    let output = format!("start{}", "x".repeat(ACTIVITY_DETAIL_PREVIEW_BYTES * 3));
    insert_activity(
        &pool,
        "activity:command",
        "command_execution",
        Some(&output),
    )
    .await;
    insert_activity(&pool, "activity:search", "web_search", Some(&output)).await;

    let read = read(&pool, None, 600_000).await;

    let message = &read.items[0].data.value;
    let tail = message["markdown"].as_str().unwrap();
    assert!(tail.len() <= LIVE_MESSAGE_TAIL_BYTES);
    assert!(tail.ends_with("end"));
    assert_eq!(message["markdownTruncated"], json!(true));
    let command = &read.items[1].data.value;
    let preview = command["detailPreview"].as_str().unwrap();
    assert_eq!(preview.len(), ACTIVITY_DETAIL_PREVIEW_BYTES);
    assert!(!preview.starts_with("start"));
    assert_eq!(command["detailBytes"], json!(output.len()));
    assert_eq!(command["detailTruncated"], json!(true));
    let search = read.items[2].data.value["detailPreview"].as_str().unwrap();
    assert!(search.starts_with("start"));
}

#[tokio::test]
async fn a_small_budget_pages_by_change_and_resumes_from_the_cursor() {
    let pool = seeded_pool(Some(DEVICE), "working", "working").await;
    for index in 0..5 {
        insert_message(
            &pool,
            &format!("message:{index}"),
            TURN,
            "assistant",
            &"m".repeat(2_000),
        )
        .await;
    }

    let mut cursor = None;
    let mut seen = Vec::new();
    for _ in 0..10 {
        let page = read(&pool, cursor, 5_000).await;
        assert!(serde_json::to_string(&page).unwrap().len() <= 5_000 || page.items.len() == 1);
        seen.extend(ids(&page).into_iter().map(str::to_string));
        cursor = Some(page.change_cursor);
        if !page.has_more {
            break;
        }
        assert!(!page.items.is_empty());
    }

    assert_eq!(
        seen,
        (0..5)
            .map(|index| format!("message:{index}"))
            .collect::<Vec<_>>()
    );
    let again = read(&pool, cursor, 5_000).await;
    assert!(again.items.is_empty());
    assert!(!again.has_more);
}

#[tokio::test]
async fn the_budget_counts_the_transport_encoding() {
    fn doubled(text: &str) -> usize {
        text.len() * 2
    }
    let pool = seeded_pool(Some(DEVICE), "working", "working").await;
    for index in 0..4 {
        insert_message(
            &pool,
            &format!("message:{index}"),
            TURN,
            "assistant",
            &"\"".repeat(1_000),
        )
        .await;
    }
    let plain = read(&pool, None, 12_000).await;
    let encoded = match read_live_execution(
        &pool,
        query(None),
        LiveExecutionBudget {
            payload_bytes: 12_000,
            measure: doubled,
        },
    )
    .await
    .unwrap()
    {
        LiveExecutionOutcome::Read(read) => *read,
        LiveExecutionOutcome::NotHere => panic!("the assignment runs here"),
    };
    assert!(encoded.items.len() < plain.items.len());
    assert!(encoded.has_more);
}

#[tokio::test]
async fn removals_and_reset_follow_the_thread_clock() {
    let pool = seeded_pool(Some(DEVICE), "working", "working").await;
    insert_message(&pool, "message:gone", TURN, "assistant", "draft").await;
    let first = read(&pool, None, 600_000).await;
    sqlx::query("DELETE FROM chat_messages WHERE id = 'message:gone'")
        .execute(&pool)
        .await
        .unwrap();

    let after = read(&pool, Some(first.change_cursor), 600_000).await;
    assert_eq!(
        after
            .removed_item_ids
            .iter()
            .map(ChatActivityId::as_str)
            .collect::<Vec<_>>(),
        ["message:gone"]
    );
    let ahead = read(&pool, Some(after.change_cursor + 10), 600_000).await;
    assert!(ahead.reset_required);
    assert!(ahead.items.is_empty());
}

#[tokio::test]
async fn settled_follows_the_assignment_and_the_run() {
    let cases = [
        ("working", "working", false),
        ("waiting_for_approval", "waiting", false),
        ("working", "completed", true),
        ("ready_for_review", "working", true),
        ("cancelled", "cancelled", true),
    ];
    for (state, run_state, settled) in cases {
        let pool = seeded_pool(Some(DEVICE), state, run_state).await;
        assert_eq!(
            read(&pool, None, 600_000).await.settled,
            settled,
            "{state} {run_state}"
        );
    }
}

#[tokio::test]
async fn a_queued_assignment_without_a_run_reads_empty() {
    let pool = seeded_pool(Some(DEVICE), "queued", "working").await;
    sqlx::query("DELETE FROM chat_agent_runs")
        .execute(&pool)
        .await
        .unwrap();
    let read = read(&pool, None, 600_000).await;
    assert!(read.turn_id.is_none());
    assert!(read.run_id.is_none());
    assert!(read.items.is_empty());
    assert!(!read.settled);
}

async fn set_applying(pool: &SqlitePool, applying: bool) {
    sqlx::query("UPDATE sync_apply_state SET applying = ? WHERE singleton = 1")
        .bind(i64::from(applying))
        .execute(pool)
        .await
        .unwrap();
}

#[tokio::test]
async fn web_activities_carry_their_query_and_bounded_result_links() {
    let pool = seeded_pool(Some(DEVICE), "working", "working").await;
    let long_title = "t".repeat(400);
    let mut links = vec![
        json!({ "url": "https://example.com/a", "title": long_title, "snippet": "secret" }),
        json!({ "url": "javascript:secret" }),
        json!({ "url": format!("https://example.com/{}", "x".repeat(3000)) }),
    ];
    links.extend((0..30).map(|index| json!({ "url": format!("https://example.com/{index}") })));
    let metadata = json!({
        "toolName": "WebSearch",
        "toolInput": { "query": "riemann", "token": "secret" },
        "webResult": { "links": links, "summary": "secret" },
    });
    sqlx::query(
        "INSERT INTO chat_activities
            (id, thread_id, turn_id, sequence_anchor, item_kind, status, title, detail,
             provider_item_id, safe_metadata_data, source_event_type, created_at, updated_at)
         VALUES ('activity:search', ?, ?, 2, 'web_search', 'completed', 'riemann', NULL,
                 'provider-item', ?, 'item_completed', ?, ?)",
    )
    .bind(THREAD)
    .bind(TURN)
    .bind(metadata.to_string())
    .bind(NOW)
    .bind(NOW)
    .execute(&pool)
    .await
    .unwrap();

    let read = read(&pool, None, 600_000).await;
    let encoded = serde_json::to_string(&read).unwrap();

    assert!(!encoded.contains("secret"), "{encoded}");
    let metadata = &read.items[0].data.value["metadata"];
    assert_eq!(metadata["toolName"], json!("WebSearch"));
    assert_eq!(metadata["toolInput"], json!({ "query": "riemann" }));
    let links = metadata["webResult"]["links"].as_array().unwrap();
    assert_eq!(links.len(), 24);
    assert_eq!(links[0]["url"], json!("https://example.com/a"));
    assert_eq!(links[0]["title"].as_str().unwrap().len(), 256);
    assert_eq!(links[1], json!({ "url": "https://example.com/0" }));
}
