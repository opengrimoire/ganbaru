use super::*;

const SEEDED_AT: &str = "2026-10-20T12:00:00.000Z";
const EDITED_AT: &str = "2026-10-20T12:05:00.000Z";

fn assignment() -> ChatWorkAssignmentId {
    ChatWorkAssignmentId::new("assignment:folders").unwrap()
}

fn input(item_id: &str) -> ChatAssignmentInputId {
    ChatAssignmentInputId::new(format!("input:{item_id}")).unwrap()
}

fn edit(markdown: &str) -> ChatAssignmentInputEditRequest {
    ChatAssignmentInputEditRequest {
        normalized_markdown: markdown.to_string(),
        rich_content: VersionedJson {
            schema_version: 1,
            value: serde_json::json!({ "type": "doc", "content": [] }),
        },
    }
}

fn now() -> UtcTimestamp {
    UtcTimestamp::new(EDITED_AT).unwrap()
}

/// Seeds the reply `item_id` by `author`, routed to the fixture assignment at `ordinal`.
async fn seed_reply(
    pool: &SqlitePool,
    item_id: &str,
    ordinal: i64,
    routing_kind: &str,
    author: &str,
    text: &str,
) {
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "INSERT INTO chat_conversation_items
            (id, conversation_id, reply_thread_id, item_kind, order_key, created_at)
         SELECT '{item_id}', conversation_id, id, 'message', 'a{ordinal}', '{SEEDED_AT}'
         FROM chat_reply_threads WHERE id = 'reply:folders';
         INSERT INTO chat_communication_messages (item_id, author_participant_id, created_at)
         VALUES ('{item_id}', '{author}', '{SEEDED_AT}');
         INSERT INTO chat_communication_message_revisions
            (id, message_item_id, normalized_markdown, created_at)
         VALUES ('revision:{item_id}', '{item_id}', '{text}', '{SEEDED_AT}');
         UPDATE chat_communication_messages
         SET current_revision_id = 'revision:{item_id}' WHERE item_id = '{item_id}';
         INSERT INTO chat_work_assignment_inputs
            (id, assignment_id, message_item_id, ordinal, routing_kind, created_at)
         VALUES ('input:{item_id}', 'assignment:folders', '{item_id}', {ordinal},
                 '{routing_kind}', '{SEEDED_AT}');"
    )))
    .execute(pool)
    .await
    .unwrap();
}

async fn seed_local_reply(pool: &SqlitePool, item_id: &str, ordinal: i64, text: &str) {
    seed_reply(
        pool,
        item_id,
        ordinal,
        "queued_continuation",
        LOCAL_PARTICIPANT_ID,
        text,
    )
    .await;
}

async fn pool() -> SqlitePool {
    crate::chat::revocation::tests::pool_with_folder_runs().await
}

async fn deliver(pool: &SqlitePool, item_id: &str) {
    sqlx::query(
        "UPDATE chat_work_assignment_inputs
         SET delivery_state = 'delivered', delivered_at = ?,
             delivered_revision_id = 'revision:' || message_item_id
         WHERE message_item_id = ?",
    )
    .bind(SEEDED_AT)
    .bind(item_id)
    .execute(pool)
    .await
    .unwrap();
}

fn conflict_reason(error: &ChatError) -> Option<&str> {
    error
        .details
        .as_deref()
        .and_then(|details| details.get("reason"))
        .and_then(serde_json::Value::as_str)
}

#[tokio::test]
async fn assignment_inputs_list_excludes_triggers_and_orders_by_ordinal() {
    let pool = pool().await;
    seed_reply(
        &pool,
        "item:trigger",
        1,
        "trigger",
        LOCAL_PARTICIPANT_ID,
        "Start",
    )
    .await;
    seed_local_reply(&pool, "item:third", 4, "Then update the docs").await;
    seed_reply(
        &pool,
        "item:second",
        2,
        "steer",
        LOCAL_PARTICIPANT_ID,
        "Prefer the smaller fix",
    )
    .await;
    seed_local_reply(&pool, "item:long", 5, &"é".repeat(INPUT_PREVIEW_CHARS + 1)).await;
    seed_local_reply(&pool, "item:exact", 6, &"a".repeat(INPUT_PREVIEW_CHARS)).await;
    deliver(&pool, "item:second").await;

    let inputs = list_assignment_inputs(&pool, &assignment()).await.unwrap();

    let order = inputs
        .iter()
        .map(|input| input.message_item_id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        order,
        ["item:second", "item:third", "item:long", "item:exact"]
    );
    let second = &inputs[0];
    assert_eq!(second.routing_kind, "steer");
    assert_eq!(second.delivery_state, "delivered");
    assert_eq!(
        second.delivered_revision_id, second.current_revision_id,
        "an input delivered unedited names its current revision"
    );
    assert_eq!(second.preview, "Prefer the smaller fix");
    assert!(!second.preview_truncated);
    assert_eq!(
        second
            .author_participant_id
            .as_ref()
            .map(ChatParticipantId::as_str),
        Some(LOCAL_PARTICIPANT_ID)
    );
    assert_eq!(inputs[1].delivery_state, "pending");
    assert_eq!(inputs[1].delivered_revision_id, None);
    assert_eq!(inputs[2].preview.chars().count(), INPUT_PREVIEW_CHARS);
    assert!(inputs[2].preview_truncated, "a cut preview says so");
    assert!(
        !inputs[3].preview_truncated,
        "a message of exactly the preview length is whole"
    );
}

#[tokio::test]
async fn edit_assignment_input_replaces_the_text_the_run_will_receive() {
    let pool = pool().await;
    seed_local_reply(&pool, "item:queued", 2, "Then update the docs").await;
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "INSERT INTO chat_communication_attachment_references
            (message_revision_id, ordinal, attachment_id, working_folder_id, kind,
             original_display_name, mime_type, byte_size, sha256)
         VALUES ('revision:item:queued', 0, 'attachment:log', 'workspace-1', 'text_snippet',
                 'log.txt', 'text/plain', 0, '{}')",
        "0".repeat(64)
    )))
    .execute(&pool)
    .await
    .unwrap();

    let read = edit_assignment_input(
        &pool,
        &input("item:queued"),
        &edit("  Then update the changelog  "),
        &now(),
    )
    .await
    .unwrap();

    assert_eq!(read.preview, "Then update the changelog");
    assert_eq!(read.delivery_state, "pending");
    let current = read.current_revision_id.unwrap();
    assert_ne!(current.as_str(), "revision:item:queued");
    let edited_at: Option<String> = sqlx::query_scalar(
        "SELECT edited_at FROM chat_communication_messages WHERE item_id = 'item:queued'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(edited_at.as_deref(), Some(EDITED_AT));
    let attachments: Vec<String> = sqlx::query_scalar(
        "SELECT attachment_id FROM chat_communication_attachment_references
         WHERE message_revision_id = ? ORDER BY ordinal",
    )
    .bind(current.as_str())
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        attachments,
        ["attachment:log"],
        "the edit keeps the attachments"
    );
}

#[tokio::test]
async fn edit_assignment_input_refuses_after_delivery() {
    let pool = pool().await;
    seed_local_reply(&pool, "item:delivered", 2, "Prefer the smaller fix").await;
    seed_local_reply(&pool, "item:cancelled", 3, "Then update the docs").await;
    deliver(&pool, "item:delivered").await;
    cancel_assignment_input(&pool, &input("item:cancelled"))
        .await
        .unwrap();

    for item_id in ["item:delivered", "item:cancelled"] {
        let error = edit_assignment_input(&pool, &input(item_id), &edit("Changed"), &now())
            .await
            .unwrap_err();
        assert_eq!(error.code, ChatErrorCode::Conflict, "{item_id}");
        assert_eq!(conflict_reason(&error), Some(INPUT_ALREADY_DELIVERED));
    }
    let revisions: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM chat_communication_message_revisions
         WHERE message_item_id IN ('item:delivered', 'item:cancelled')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(revisions, 2, "a refused edit writes no revision");
}

#[tokio::test]
async fn edit_assignment_input_refuses_other_authors() {
    let pool = pool().await;
    let agent: String = sqlx::query_scalar(
        "SELECT agent_id FROM chat_work_assignments WHERE id = 'assignment:folders'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    seed_reply(
        &pool,
        "item:agent",
        2,
        "queued_continuation",
        &agent,
        "I will look",
    )
    .await;

    let error = edit_assignment_input(&pool, &input("item:agent"), &edit("Changed"), &now())
        .await
        .unwrap_err();

    assert_eq!(error.code, ChatErrorCode::Permission);
    let current: String = sqlx::query_scalar(
        "SELECT current_revision_id FROM chat_communication_messages
         WHERE item_id = 'item:agent'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(current, "revision:item:agent");
}

#[tokio::test]
async fn edit_assignment_input_refuses_empty_text_and_the_trigger() {
    let pool = pool().await;
    seed_local_reply(&pool, "item:queued", 2, "Then update the docs").await;
    seed_reply(
        &pool,
        "item:trigger",
        1,
        "trigger",
        LOCAL_PARTICIPANT_ID,
        "Start",
    )
    .await;

    let empty = edit_assignment_input(&pool, &input("item:queued"), &edit("   "), &now())
        .await
        .unwrap_err();
    assert_eq!(empty.code, ChatErrorCode::Validation);
    let trigger = edit_assignment_input(&pool, &input("item:trigger"), &edit("Changed"), &now())
        .await
        .unwrap_err();
    assert_eq!(trigger.code, ChatErrorCode::NotFound);
}

#[tokio::test]
async fn cancel_assignment_input_sets_ignored_once() {
    let pool = pool().await;
    seed_local_reply(&pool, "item:queued", 2, "Then update the docs").await;
    seed_local_reply(&pool, "item:delivered", 3, "Prefer the smaller fix").await;
    deliver(&pool, "item:delivered").await;

    let cancelled = cancel_assignment_input(&pool, &input("item:queued"))
        .await
        .unwrap();
    assert_eq!(cancelled.delivery_state, "ignored");

    let again = cancel_assignment_input(&pool, &input("item:queued"))
        .await
        .unwrap_err();
    assert_eq!(again.code, ChatErrorCode::Conflict);
    assert_eq!(conflict_reason(&again), Some(INPUT_ALREADY_DELIVERED));
    let late = cancel_assignment_input(&pool, &input("item:delivered"))
        .await
        .unwrap_err();
    assert_eq!(conflict_reason(&late), Some(INPUT_ALREADY_DELIVERED));
    let missing = cancel_assignment_input(&pool, &input("item:missing"))
        .await
        .unwrap_err();
    assert_eq!(missing.code, ChatErrorCode::NotFound);
    let states: Vec<String> = sqlx::query_scalar(
        "SELECT delivery_state FROM chat_work_assignment_inputs
         WHERE assignment_id = 'assignment:folders' AND routing_kind <> 'trigger'
         ORDER BY ordinal",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(states, ["ignored", "delivered"]);
}

#[tokio::test]
async fn a_follow_up_that_started_work_is_listed_but_never_edited_or_cancelled() {
    let pool = pool().await;
    seed_reply(
        &pool,
        "item:follow-up",
        2,
        "follow_up",
        LOCAL_PARTICIPANT_ID,
        "Also check the logs",
    )
    .await;

    let listed = list_assignment_inputs(&pool, &assignment()).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].routing_kind, "follow_up");

    let edited = edit_assignment_input(&pool, &input("item:follow-up"), &edit("Changed"), &now())
        .await
        .unwrap_err();
    assert_eq!(edited.code, ChatErrorCode::Conflict);
    assert_eq!(conflict_reason(&edited), Some(INPUT_STARTS_WORK));
    let cancelled = cancel_assignment_input(&pool, &input("item:follow-up"))
        .await
        .unwrap_err();
    assert_eq!(cancelled.code, ChatErrorCode::Conflict);
    assert_eq!(conflict_reason(&cancelled), Some(INPUT_STARTS_WORK));

    let (state, revisions): (String, i64) = sqlx::query_as(
        "SELECT input.delivery_state,
                (SELECT count(*) FROM chat_communication_message_revisions
                 WHERE message_item_id = input.message_item_id)
         FROM chat_work_assignment_inputs input WHERE input.id = 'input:item:follow-up'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        (state.as_str(), revisions),
        ("pending", 1),
        "the frozen text and its delivery stay as the package holds them"
    );

    deliver(&pool, "item:follow-up").await;
    let late = cancel_assignment_input(&pool, &input("item:follow-up"))
        .await
        .unwrap_err();
    assert_eq!(conflict_reason(&late), Some(INPUT_ALREADY_DELIVERED));
}

#[tokio::test]
async fn hold_and_resume_change_only_pending_waiting_replies() {
    let pool = pool().await;
    seed_local_reply(&pool, "item:queued", 2, "Then update the docs").await;
    seed_local_reply(&pool, "item:delivered", 3, "Prefer the smaller fix").await;
    deliver(&pool, "item:delivered").await;
    seed_reply(
        &pool,
        "item:follow-up",
        4,
        "follow_up",
        LOCAL_PARTICIPANT_ID,
        "Also check the logs",
    )
    .await;

    let held = set_assignment_input_held(&pool, &input("item:queued"), true)
        .await
        .unwrap();
    assert!(held.held);
    let listed = list_assignment_inputs(&pool, &assignment()).await.unwrap();
    assert!(listed[0].held, "the hold is stored and listed");
    let resumed = set_assignment_input_held(&pool, &input("item:queued"), false)
        .await
        .unwrap();
    assert!(!resumed.held);

    let late = set_assignment_input_held(&pool, &input("item:delivered"), true)
        .await
        .unwrap_err();
    assert_eq!(conflict_reason(&late), Some(INPUT_ALREADY_DELIVERED));
    let follow_up = set_assignment_input_held(&pool, &input("item:follow-up"), true)
        .await
        .unwrap_err();
    assert_eq!(conflict_reason(&follow_up), Some(INPUT_STARTS_WORK));
    let missing = set_assignment_input_held(&pool, &input("item:missing"), true)
        .await
        .unwrap_err();
    assert_eq!(missing.code, ChatErrorCode::NotFound);
}

#[tokio::test]
async fn steering_a_queued_reply_routes_it_to_the_current_turn_and_resumes_it() {
    let pool = pool().await;
    seed_local_reply(&pool, "item:queued", 2, "Then update the docs").await;
    set_assignment_input_held(&pool, &input("item:queued"), true)
        .await
        .unwrap();

    let steered = steer_assignment_input(&pool, &input("item:queued"))
        .await
        .unwrap();
    assert_eq!(
        (steered.routing_kind.as_str(), steered.held),
        ("steer", false)
    );

    cancel_assignment_input(&pool, &input("item:queued"))
        .await
        .unwrap();
    let settled = steer_assignment_input(&pool, &input("item:queued"))
        .await
        .unwrap_err();
    assert_eq!(conflict_reason(&settled), Some(INPUT_ALREADY_DELIVERED));
}

#[tokio::test]
async fn reorder_keeps_the_slots_of_waiting_replies_and_refuses_a_stale_set() {
    let pool = pool().await;
    seed_local_reply(&pool, "item:first", 2, "First").await;
    seed_local_reply(&pool, "item:delivered", 3, "Delivered").await;
    deliver(&pool, "item:delivered").await;
    seed_local_reply(&pool, "item:second", 4, "Second").await;
    seed_local_reply(&pool, "item:third", 6, "Third").await;

    let reordered = reorder_assignment_inputs(
        &pool,
        &assignment(),
        &[
            input("item:third"),
            input("item:first"),
            input("item:second"),
        ],
    )
    .await
    .unwrap();
    let order = reordered
        .iter()
        .map(|read| (read.message_item_id.as_str(), read.ordinal))
        .collect::<Vec<_>>();
    assert_eq!(
        order,
        [
            ("item:third", 2),
            ("item:delivered", 3),
            ("item:first", 4),
            ("item:second", 6)
        ],
        "the delivered reply keeps its place between the reordered ones"
    );

    let stale = reorder_assignment_inputs(
        &pool,
        &assignment(),
        &[input("item:first"), input("item:second")],
    )
    .await
    .unwrap_err();
    assert_eq!(stale.code, ChatErrorCode::StaleRevision);
    let with_settled = reorder_assignment_inputs(
        &pool,
        &assignment(),
        &[
            input("item:first"),
            input("item:second"),
            input("item:third"),
            input("item:delivered"),
        ],
    )
    .await
    .unwrap_err();
    assert_eq!(with_settled.code, ChatErrorCode::StaleRevision);
}

#[test]
fn reorder_ordinals_stay_strictly_increasing() {
    assert_eq!(reorder_ordinals(&[1, 3, 4]).unwrap(), [1, 3, 4]);
    assert_eq!(reorder_ordinals(&[2, 2, 3]).unwrap(), [2, 3, 4]);
    assert_eq!(reorder_ordinals(&[]).unwrap(), Vec::<i64>::new());
    assert_eq!(
        reorder_ordinals(&[i64::MAX, i64::MAX]).unwrap_err().code,
        ChatErrorCode::Validation
    );
}

#[test]
fn preview_cuts_at_a_character_boundary() {
    let text = format!("{}tail", "€".repeat(INPUT_PREVIEW_CHARS));
    let (cut, truncated) = preview(&text);
    assert_eq!(cut, "€".repeat(INPUT_PREVIEW_CHARS));
    assert!(truncated);
    assert_eq!(preview(""), (String::new(), false));
}
