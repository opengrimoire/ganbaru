//! Chat coordination across replicas: work assignments, their inputs and semantic updates, and
//! scheduled messages with their owned references arrive whole, a crash while sealing or
//! applying them leaves nothing partial, rerouting an input updates its row in place, a no-op
//! write captures nothing, and a settle on any device outranks progress the executing device
//! reports concurrently.

use super::*;
use crate::manifest::vault::chat::{
    ASSIGNMENT_INPUT_KEY_PREFIX, INPUTS_TABLE, SCHEDULED_TABLE, SEMANTIC_UPDATES_TABLE,
};

const ASSIGNMENT: &str = "assignment:review";
const FOLLOW_UP: &str = "assignment:follow-up";
const MENTION: &str = "item:mention";
const UPDATE: &str = "item:update";
const SCHEDULED: &str = "scheduled:reminder";
const EXECUTOR: &str = "device-0";

/// Seeds of the cancel against progress scenario.
const PROGRESS_SEEDS: u64 = 8;

/// Rounds of one cancel against progress scenario.
const PROGRESS_ROUNDS: usize = 5;

/// States the executing device reports while it works.
const ACTIVE_STATES: [&str; 4] = [
    "working",
    "waiting_for_answer",
    "waiting_for_approval",
    "ready_for_review",
];

/// The derived id of the input `message` routes.
fn input_id(message: &str) -> String {
    key(ASSIGNMENT_INPUT_KEY_PREFIX, &[message])
}

/// SQL creating the assignment `id` of [`TEAMMATE`] triggered by [`MENTION`] in the reply
/// thread under [`ROOT`], executed by [`EXECUTOR`], with its trigger input.
fn assignment_sql(id: &str, previous: Option<&str>) -> String {
    format!(
        "INSERT INTO chat_work_assignments
            (id, reply_thread_id, teammate_id, triggering_message_item_id, executing_device_id,
             previous_assignment_id, state, created_at, updated_at)
         VALUES ({id}, {thread}, {teammate}, {mention}, {executor}, {previous}, 'queued',
                 {created}, {created});",
        id = quoted(id),
        thread = quoted(&thread()),
        teammate = quoted(TEAMMATE),
        mention = quoted(MENTION),
        executor = quoted(EXECUTOR),
        previous = optional(previous),
        created = quoted(CREATED),
    )
}

/// SQL routing `message` to `assignment` as input `ordinal` of `kind`.
fn input_sql(message: &str, assignment: &str, ordinal: i64, kind: &str) -> String {
    format!(
        "INSERT INTO chat_work_assignment_inputs
            (id, assignment_id, message_item_id, ordinal, routing_kind, created_at)
         VALUES ({id}, {assignment}, {message}, {ordinal}, {kind}, {created});",
        id = quoted(&input_id(message)),
        assignment = quoted(assignment),
        message = quoted(message),
        kind = quoted(kind),
        created = quoted(CREATED),
    )
}

/// SQL posting the thread, the mention, the assignment with its trigger input, and a plan
/// update.
fn coordination_sql() -> String {
    format!(
        "{}{}{}{}{}
         INSERT INTO chat_conversation_items
            (id, conversation_id, reply_thread_id, item_kind, order_key, created_at)
         VALUES ({update}, {conversation}, {thread}, 'work_update', 'c002', {created});
         INSERT INTO chat_work_semantic_updates
            (item_id, assignment_id, update_kind, payload_data, created_at)
         VALUES ({update}, {assignment}, 'plan', '{{\"steps\":[\"Read\"]}}', {created});",
        message_sql(ROOT, None, "c001", "Root"),
        thread_sql(),
        message_sql(MENTION, Some(&thread()), "c001", "@Reviewer please check"),
        assignment_sql(ASSIGNMENT, None),
        input_sql(MENTION, ASSIGNMENT, 1, "trigger"),
        update = quoted(UPDATE),
        conversation = quoted(CONVERSATION),
        thread = quoted(&thread()),
        assignment = quoted(ASSIGNMENT),
        created = quoted(CREATED),
    )
}

/// SQL scheduling [`SCHEDULED`] in [`CHANNEL`] from [`EXECUTOR`] with one folder mention and
/// one attachment.
fn scheduled_sql() -> String {
    format!(
        "INSERT INTO chat_scheduled_messages
            (id, client_command_id, channel_id, executing_device_id, request_data, scheduled_for,
             created_at, updated_at)
         VALUES ({scheduled}, 'command:reminder', {channel}, {executor},
                 '{{\"markdown\":\"Check the folder\"}}', {due}, {created}, {created});
         INSERT INTO chat_scheduled_message_references
            (scheduled_message_id, ordinal, reference_kind, label_snapshot, plain_text_projection,
             start_offset, end_offset, working_folder_id)
         VALUES ({scheduled}, 0, 'working_folder', 'Routine', 'Routine', 10, 17, {folder});
         INSERT INTO chat_scheduled_message_attachment_references
            (scheduled_message_id, ordinal, attachment_id, working_folder_id, kind,
             original_display_name, mime_type, byte_size, sha256)
         VALUES ({scheduled}, 0, 'attachment:image', {folder}, 'image', 'diagram.png',
                 'image/png', 4096, {image});",
        scheduled = quoted(SCHEDULED),
        channel = quoted(CHANNEL),
        executor = quoted(EXECUTOR),
        due = quoted(&at(30, 0)),
        created = quoted(CREATED),
        folder = quoted(FOLDER),
        image = quoted(IMAGE_DIGEST),
    )
}

/// SQL moving [`ASSIGNMENT`] to `state`, settling it when the state is final.
fn state_sql(state: &str, updated_at: &str) -> String {
    let settled = matches!(state, "completed" | "failed" | "cancelled");
    format!(
        "UPDATE chat_work_assignments
         SET state = {state}, settled_at = {settled_at}, revision = revision + 1,
             updated_at = {updated_at}
         WHERE id = {assignment};",
        state = quoted(state),
        settled_at = if settled {
            quoted(updated_at)
        } else {
            "NULL".to_owned()
        },
        updated_at = quoted(updated_at),
        assignment = quoted(ASSIGNMENT),
    )
}

/// [`ASSIGNMENT`]'s state and whether it is settled.
async fn assignment_state(replica: &mut Replica) -> String {
    replica
        .scalar_text(
            "SELECT state || ' ' || (settled_at IS NOT NULL) FROM chat_work_assignments
             WHERE id = 'assignment:review'",
        )
        .await
        .expect("the assignment arrived")
}

/// Captured coordination rows as `table_id row_key`.
async fn coordination_captures(replica: &mut Replica) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT table_id || ' ' || row_key FROM sync_capture
         WHERE table_id BETWEEN ? AND ? ORDER BY table_id, row_key",
    )
    .bind(i64::from(SCHEDULED_TABLE.0))
    .bind(i64::from(SEMANTIC_UPDATES_TABLE.0))
    .fetch_all(&mut replica.conn)
    .await
    .unwrap()
}

#[test]
fn chat_coordination_rows_round_trip_with_their_derived_keys() {
    block_on(async {
        let mut replicas = conversation_replicas(2).await;
        replicas[0]
            .exec(&format!("{}{}", coordination_sql(), scheduled_sql()))
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;

        let reader = &mut replicas[1];
        assert_eq!(
            reader
                .scalar_text(
                    "SELECT assignment.executing_device_id || ' ' || assignment.state || ' '
                            || input.id || ' ' || input.routing_kind || ' '
                            || update_row.update_kind
                     FROM chat_work_assignments assignment
                     JOIN chat_work_assignment_inputs input ON input.assignment_id = assignment.id
                     JOIN chat_work_semantic_updates update_row
                       ON update_row.assignment_id = assignment.id",
                )
                .await,
            Some(format!("{EXECUTOR} queued {} trigger plan", input_id(MENTION)))
        );
        assert_eq!(
            reader
                .scalar_text(
                    "SELECT message.executing_device_id || ' ' || message.request_data || ' '
                            || reference.working_folder_id || ' ' || attachment.sha256
                     FROM chat_scheduled_messages message
                     JOIN chat_scheduled_message_references reference
                       ON reference.scheduled_message_id = message.id
                     JOIN chat_scheduled_message_attachment_references attachment
                       ON attachment.scheduled_message_id = message.id",
                )
                .await,
            Some(format!(
                "{EXECUTOR} {{\"markdown\":\"Check the folder\"}} {FOLDER} {IMAGE_DIGEST}"
            ))
        );
        assert_eq!(
            reader
                .count("SELECT count(*) FROM chat_scheduled_message_dispatch")
                .await,
            0,
            "dispatch claims stay on the scheduling device"
        );
    });
}

#[test]
fn chat_a_crash_while_sealing_or_applying_coordination_rows_leaves_nothing_partial() {
    block_on(async {
        let mut replicas = conversation_replicas(2).await;
        replicas[0]
            .exec(&format!("{}{}", coordination_sql(), scheduled_sql()))
            .await;
        // The scheduled message, the assignment, its input, and the semantic update.
        assert_eq!(coordination_captures(&mut replicas[0]).await.len(), 4);
        let report = after_rollback(
            &mut replicas[0],
            "seal.before_commit",
            async |r: &mut Replica| r.seal().await,
        )
        .await;
        assert_eq!(report.sealed_ops, 1, "the rows seal together");
        assert!(coordination_captures(&mut replicas[0]).await.is_empty());

        let (author, rest) = replicas.split_at_mut(1);
        let reader = &mut rest[0];
        let known = reader.stored_vector().await;
        let ops = author[0].ops_since(&known).await;
        reader.receive(&ops).await;
        after_rollback(reader, "apply.before_commit", async |r: &mut Replica| {
            r.apply_all().await
        })
        .await;
        assert_eq!(
            reader
                .count(
                    "SELECT (SELECT count(*) FROM chat_work_assignments)
                          + (SELECT count(*) FROM chat_work_assignment_inputs)
                          + (SELECT count(*) FROM chat_work_semantic_updates)
                          + (SELECT count(*) FROM chat_scheduled_messages)
                          + (SELECT count(*) FROM chat_scheduled_message_references)
                          + (SELECT count(*) FROM chat_scheduled_message_attachment_references)",
                )
                .await,
            6
        );
        assert_converged(&mut replicas).await;
    });
}

#[test]
fn chat_rerouting_an_input_updates_its_row_and_no_op_writes_capture_nothing() {
    block_on(async {
        let mut replicas = conversation_replicas(2).await;
        replicas[0].exec(&coordination_sql()).await;
        converge(&mut replicas).await;
        replicas[1]
            .exec(
                "CREATE TEMP TABLE input_deletes (id TEXT NOT NULL);
                 CREATE TEMP TRIGGER count_input_deletes
                 AFTER DELETE ON chat_work_assignment_inputs
                 BEGIN INSERT INTO input_deletes (id) VALUES (OLD.id); END;",
            )
            .await;

        replicas[0]
            .exec(&format!(
                "{}{}",
                state_sql("completed", &at(1, 0)),
                assignment_sql(FOLLOW_UP, Some(ASSIGNMENT)),
            ))
            .await;
        replicas[0]
            .exec(&format!(
                "UPDATE chat_work_assignment_inputs
                 SET assignment_id = {follow_up}, ordinal = 1, routing_kind = 'follow_up'
                 WHERE id = {input};",
                follow_up = quoted(FOLLOW_UP),
                input = quoted(&input_id(MENTION)),
            ))
            .await;
        assert!(
            coordination_captures(&mut replicas[0])
                .await
                .contains(&format!("{} {}", INPUTS_TABLE.0, input_id(MENTION))),
            "the reroute captures the input row"
        );
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        let reader = &mut replicas[1];
        assert_eq!(
            reader
                .scalar_text(
                    "SELECT id || ' ' || assignment_id || ' ' || routing_kind
                     FROM chat_work_assignment_inputs",
                )
                .await,
            Some(format!("{} {FOLLOW_UP} follow_up", input_id(MENTION)))
        );
        assert_eq!(
            reader.count("SELECT count(*) FROM temp.input_deletes").await,
            0,
            "the input is updated in place"
        );

        replicas[0]
            .exec(
                "UPDATE chat_work_assignments SET state = state, executing_device_id =
                    executing_device_id;
                 UPDATE chat_work_assignment_inputs SET routing_kind = routing_kind;",
            )
            .await;
        assert!(coordination_captures(&mut replicas[0]).await.is_empty());
    });
}

#[test]
fn chat_a_concurrent_settle_outranks_progress_and_a_review_request_outranks_work() {
    block_on(async {
        let mut replicas = conversation_replicas(3).await;
        replicas[0].exec(&coordination_sql()).await;
        converge(&mut replicas).await;

        replicas[0]
            .exec(&state_sql("ready_for_review", &at(1, 0)))
            .await;
        replicas[1].exec(&state_sql("working", &at(1, 30))).await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        assert_eq!(assignment_state(&mut replicas[2]).await, "ready_for_review 0");

        replicas[0].exec(&state_sql("working", &at(2, 30))).await;
        replicas[2].exec(&state_sql("cancelled", &at(2, 0))).await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(assignment_state(replica).await, "cancelled 1");
        }
    });
}

/// The executing device reports progress while the other devices cancel at random; a cancel
/// anywhere ends the assignment on every device whatever the exchange order, and without one
/// every device shows the executing device's last report.
#[test]
fn chat_simulated_cancel_against_active_progress_keeps_the_cancel() {
    block_on(async {
        for seed in 0..PROGRESS_SEEDS {
            let mut rng = Rng(seed ^ 0xa551);
            let mut replicas = conversation_replicas(3).await;
            replicas[0].exec(&coordination_sql()).await;
            converge(&mut replicas).await;
            let mut cancelled = false;
            let mut last_progress = "queued";
            for round in 0..PROGRESS_ROUNDS {
                let executor_settled = assignment_state(&mut replicas[0]).await.ends_with(" 1");
                if !executor_settled && rng.chance(80) {
                    let state = ACTIVE_STATES[rng.below(ACTIVE_STATES.len())];
                    replicas[0]
                        .exec(&state_sql(state, &at(round + 1, rng.below(60))))
                        .await;
                    last_progress = state;
                }
                for replica in replicas.iter_mut().skip(1) {
                    let settled = assignment_state(replica).await.ends_with(" 1");
                    if !settled && rng.chance(15) {
                        replica
                            .exec(&state_sql("cancelled", &at(round + 1, rng.below(60))))
                            .await;
                        cancelled = true;
                    }
                }
                if rng.chance(50) {
                    converge(&mut replicas).await;
                }
            }
            converge(&mut replicas).await;
            assert_converged(&mut replicas).await;
            let expected = if cancelled {
                "cancelled 1".to_owned()
            } else {
                format!("{last_progress} 0")
            };
            for replica in &mut replicas {
                assert_eq!(
                    assignment_state(replica).await,
                    expected,
                    "seed {seed} replica {}",
                    replica.index
                );
            }
        }
    });
}
