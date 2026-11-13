//! Chat coordination across replicas: work assignments, their inputs and semantic updates, and
//! scheduled messages with their owned references arrive whole, a crash while sealing or
//! applying them leaves nothing partial, rerouting an input updates its row in place, a no-op
//! write captures nothing, concurrent creates of one derived assignment merge, a settle on any
//! device outranks progress the executing device reports concurrently, a review travels
//! whole with the completion it judges while a concurrent cancel outranks it, a delivery of an
//! input outranks a concurrent cancel of it, concurrent creates with model overrides keep
//! one whole override, a concurrent narrowing of a parent grant cancels a child delegated
//! against the old authority, and a cancel of a repeating scheduled message ends its series
//! over any progress the executing device records concurrently.

use super::*;
use crate::manifest::vault::chat::{
    ASSIGNMENT_INPUT_KEY_PREFIX, INPUTS_TABLE, RECURRENCE_KEY_PREFIX, SCHEDULED_TABLE,
    SEMANTIC_UPDATES_TABLE,
};
use crate::tests::replica::pair;

pub(super) const ASSIGNMENT: &str = "assignment:review";
const FOLLOW_UP: &str = "assignment:follow-up";
pub(super) const MENTION: &str = "item:mention";
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
/// thread under [`ROOT`], executed by [`EXECUTOR`].
fn assignment_sql(id: &str, previous: Option<&str>) -> String {
    assignment_on_sql(id, previous, EXECUTOR)
}

/// SQL creating the assignment `id` of [`TEAMMATE`] triggered by [`MENTION`] in the reply
/// thread under [`ROOT`], executed by `executor`.
fn assignment_on_sql(id: &str, previous: Option<&str>, executor: &str) -> String {
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
        executor = quoted(executor),
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
pub(super) fn coordination_sql() -> String {
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
            Some(format!(
                "{EXECUTOR} queued {} trigger plan",
                input_id(MENTION)
            ))
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
            reader
                .count("SELECT count(*) FROM temp.input_deletes")
                .await,
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
        assert_eq!(
            assignment_state(&mut replicas[2]).await,
            "ready_for_review 0"
        );

        replicas[0].exec(&state_sql("working", &at(2, 30))).await;
        replicas[2].exec(&state_sql("cancelled", &at(2, 0))).await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(assignment_state(replica).await, "cancelled 1");
        }
    });
}

/// Two devices Start the same mention concurrently, each creating the assignment its derived id
/// names with itself as the executing device: the creates merge into one row with one input,
/// and every device agrees on which device executes it.
#[test]
fn chat_concurrent_starts_of_one_mention_merge_into_one_assignment() {
    block_on(async {
        let mut replicas = conversation_replicas(3).await;
        replicas[0]
            .exec(&format!(
                "{}{}{}",
                message_sql(ROOT, None, "c001", "Root"),
                thread_sql(),
                message_sql(MENTION, Some(&thread()), "c001", "@Reviewer please check"),
            ))
            .await;
        converge(&mut replicas).await;

        for (index, executor) in [(0, "device-0"), (1, "device-1")] {
            replicas[index]
                .exec(&format!(
                    "{}{}",
                    assignment_on_sql(ASSIGNMENT, None, executor),
                    input_sql(MENTION, ASSIGNMENT, 1, "trigger"),
                ))
                .await;
        }
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;

        let mut executors = Vec::new();
        for replica in &mut replicas {
            assert_eq!(
                replica
                    .count(
                        "SELECT (SELECT count(*) FROM chat_work_assignments)
                              + (SELECT count(*) FROM chat_work_assignment_inputs)",
                    )
                    .await,
                2,
                "replica {} holds one assignment and one input",
                replica.index
            );
            executors.push(
                replica
                    .scalar_text(
                        "SELECT executing_device_id || ' ' || input.assignment_id
                         FROM chat_work_assignments assignment
                         JOIN chat_work_assignment_inputs input
                           ON input.assignment_id = assignment.id",
                    )
                    .await
                    .expect("the assignment arrived"),
            );
        }
        assert!(
            executors[0] == format!("device-0 {ASSIGNMENT}")
                || executors[0] == format!("device-1 {ASSIGNMENT}")
        );
        assert!(
            executors.iter().all(|executor| *executor == executors[0]),
            "every device agrees on the executing device: {executors:?}"
        );
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

/// Reviewer of the Accept in the review scenarios.
const ACCEPTING_REVIEWER: &str = "participant:accepting";

/// Reviewer of the follow-up completion in the review scenarios.
const REQUESTING_REVIEWER: &str = "participant:requesting";

/// Most pairwise pulls between the concurrent reviews and the final convergence.
const REVIEW_PULLS: usize = 6;

/// SQL completing [`ASSIGNMENT`] with the review `outcome` by `reviewer` at `at`.
fn review_sql(outcome: &str, reviewer: &str, at: &str) -> String {
    format!(
        "UPDATE chat_work_assignments
         SET state = 'completed', settled_at = {at}, review_outcome = {outcome},
             reviewed_at = {at}, reviewed_by_participant_id = {reviewer},
             revision = revision + 1, updated_at = {at}
         WHERE id = {assignment};",
        at = quoted(at),
        outcome = quoted(outcome),
        reviewer = quoted(reviewer),
        assignment = quoted(ASSIGNMENT),
    )
}

/// [`ASSIGNMENT`]'s state, review outcome, and reviewer, `-` for no review, and whether the
/// review has a time.
async fn assignment_review(replica: &mut Replica) -> String {
    replica
        .scalar_text(
            "SELECT state || ' ' || coalesce(review_outcome, '-') || ' '
                || coalesce(reviewed_by_participant_id, '-') || ' ' || (reviewed_at IS NOT NULL)
             FROM chat_work_assignments WHERE id = 'assignment:review'",
        )
        .await
        .expect("the assignment arrived")
}

/// Brings the replicas to one ready-for-review [`ASSIGNMENT`].
async fn reviewed_replicas() -> Vec<Replica> {
    let mut replicas = conversation_replicas(3).await;
    replicas[0].exec(&coordination_sql()).await;
    replicas[0]
        .exec(&state_sql("ready_for_review", &at(1, 0)))
        .await;
    converge(&mut replicas).await;
    replicas
}

/// Pulls between random pairs of replicas, each source sealing first, so the concurrent writes
/// arrive in a seed-dependent order before the final convergence.
async fn pull_at_random(replicas: &mut [Replica], rng: &mut Rng) {
    for _ in 0..rng.below(REVIEW_PULLS + 1) {
        let target = rng.below(replicas.len());
        let source = (target + 1 + rng.below(replicas.len() - 1)) % replicas.len();
        replicas[source].seal_rotating().await.unwrap();
        let (to, from) = pair(replicas, target, source);
        to.pull(from).await;
    }
}

/// One device accepts reviewed work while another cancels it; whatever the exchange order,
/// every device ends cancelled with no review, since a cancel is the more restrictive ending.
#[test]
fn chat_simulated_accept_against_cancel_keeps_the_cancel() {
    block_on(async {
        for seed in 0..PROGRESS_SEEDS {
            let mut rng = Rng(seed ^ 0xacce);
            let mut replicas = reviewed_replicas().await;
            replicas[1]
                .exec(&review_sql(
                    "accepted",
                    ACCEPTING_REVIEWER,
                    &at(2, rng.below(60)),
                ))
                .await;
            replicas[2]
                .exec(&state_sql("cancelled", &at(2, rng.below(60))))
                .await;
            pull_at_random(&mut replicas, &mut rng).await;
            converge(&mut replicas).await;
            assert_converged(&mut replicas).await;
            for replica in &mut replicas {
                assert_eq!(
                    assignment_review(replica).await,
                    "cancelled - - 0",
                    "seed {seed} replica {}",
                    replica.index
                );
            }
        }
    });
}

/// One device accepts reviewed work while another completes it by requesting changes and
/// creating the follow-up; every device ends completed with one whole review, its outcome
/// beside its own reviewer, and the follow-up exists either way.
#[test]
fn chat_simulated_accept_against_follow_up_completion_converges_completed() {
    block_on(async {
        for seed in 0..PROGRESS_SEEDS {
            let mut rng = Rng(seed ^ 0xf011);
            let mut replicas = reviewed_replicas().await;
            replicas[1]
                .exec(&review_sql(
                    "accepted",
                    ACCEPTING_REVIEWER,
                    &at(2, rng.below(60)),
                ))
                .await;
            replicas[2]
                .exec(&format!(
                    "{}{}",
                    review_sql(
                        "changes_requested",
                        REQUESTING_REVIEWER,
                        &at(2, rng.below(60))
                    ),
                    assignment_sql(FOLLOW_UP, Some(ASSIGNMENT)),
                ))
                .await;
            pull_at_random(&mut replicas, &mut rng).await;
            converge(&mut replicas).await;
            assert_converged(&mut replicas).await;
            let winner = assignment_review(&mut replicas[0]).await;
            assert!(
                [
                    format!("completed accepted {ACCEPTING_REVIEWER} 1"),
                    format!("completed changes_requested {REQUESTING_REVIEWER} 1"),
                ]
                .contains(&winner),
                "seed {seed}: {winner}"
            );
            for replica in &mut replicas {
                assert_eq!(assignment_review(replica).await, winner, "seed {seed}");
                let follow_up = replica
                    .scalar_text(
                        "SELECT previous_assignment_id FROM chat_work_assignments
                         WHERE id = 'assignment:follow-up'",
                    )
                    .await;
                assert_eq!(follow_up.as_deref(), Some(ASSIGNMENT), "seed {seed}");
            }
        }
    });
}

/// The child [`ASSIGNMENT`] delegates in the delegation scenario.
const CHILD: &str = "assignment:delegated";

/// Seeds of the delegation against a narrowed parent grant scenario.
const DELEGATION_SEEDS: u64 = 8;

/// State reason of a child cancelled because its parent's authority narrowed.
const PARENT_AUTHORITY_REVOKED: &str = "parent_authority_revoked";

/// SQL delegating [`CHILD`] from [`ASSIGNMENT`] with an `edit` ceiling on [`FOLDER`].
fn delegated_sql() -> String {
    format!(
        "INSERT INTO chat_work_assignments
            (id, reply_thread_id, teammate_id, triggering_message_item_id, executing_device_id,
             state, parent_assignment_id, delegation_depth, delegation_objective,
             delegation_expected_result, delegation_folder_ceiling, created_at, updated_at)
         VALUES ({child}, {thread}, {teammate}, {mention}, {executor}, 'queued', {parent}, 1,
                 'Read the failing tests', 'The causes', {ceiling}, {created}, {created});",
        child = quoted(CHILD),
        thread = quoted(&thread()),
        teammate = quoted(TEAMMATE),
        mention = quoted(MENTION),
        executor = quoted(EXECUTOR),
        parent = quoted(ASSIGNMENT),
        ceiling = quoted(&format!(
            r#"[{{"workingFolderId":"{FOLDER}","capability":"edit"}}]"#
        )),
        created = quoted(CREATED),
    )
}

/// SQL moving [`CHILD`] to `state` with `reason`, settling it when the state is final.
fn child_state_sql(state: &str, reason: Option<&str>, updated_at: &str) -> String {
    let settled = matches!(state, "completed" | "failed" | "cancelled");
    format!(
        "UPDATE chat_work_assignments
         SET state = {state}, state_reason = {reason}, settled_at = {settled_at},
             revision = revision + 1, updated_at = {updated_at}
         WHERE id = {child};",
        state = quoted(state),
        reason = optional(reason),
        settled_at = if settled {
            quoted(updated_at)
        } else {
            "NULL".to_owned()
        },
        updated_at = quoted(updated_at),
        child = quoted(CHILD),
    )
}

/// Whether `replica` holds [`CHILD`] and the narrowed `read` grant, so it would refuse to run
/// the child.
async fn sees_narrowed_parent(replica: &mut Replica) -> bool {
    replica
        .scalar_text(
            "SELECT grant_row.capability FROM chat_teammate_working_folder_grants grant_row
             WHERE EXISTS (SELECT 1 FROM chat_work_assignments WHERE id = 'assignment:delegated')",
        )
        .await
        .is_some_and(|capability| capability == "read")
}

/// [`CHILD`]'s state, reason, and delegation fields.
async fn child_summary(replica: &mut Replica) -> String {
    replica
        .scalar_text(
            "SELECT state || ' ' || coalesce(state_reason, '-') || ' ' || parent_assignment_id
                    || ' ' || delegation_depth || ' ' || delegation_folder_ceiling
             FROM chat_work_assignments WHERE id = 'assignment:delegated'",
        )
        .await
        .expect("the delegated assignment arrived")
}

/// One device delegates a child under the parent's `edit` grant while another narrows that
/// grant to `read`. The executing device reports progress until it sees the narrowing, and any
/// device that holds both rows cancels the child with `parent_authority_revoked`; whatever the
/// exchange order, every device ends with the narrowed grant and the cancelled child, so a
/// concurrent narrowing always wins over a child created against the old authority.
#[test]
fn chat_simulated_delegation_against_a_narrowed_parent_grant_cancels_the_child() {
    block_on(async {
        for seed in 0..DELEGATION_SEEDS {
            let mut rng = Rng(seed ^ 0xde1e);
            let mut replicas = conversation_replicas(3).await;
            replicas[0].exec(&coordination_sql()).await;
            converge(&mut replicas).await;

            replicas[0].exec(&delegated_sql()).await;
            replicas[1].exec(&grant_sql("read", None, None)).await;
            for round in 0..PROGRESS_ROUNDS {
                pull_at_random(&mut replicas, &mut rng).await;
                let updated_at = at(round + 1, rng.below(60));
                if sees_narrowed_parent(&mut replicas[0]).await {
                    if !child_settled(&mut replicas[0]).await {
                        replicas[0]
                            .exec(&child_state_sql(
                                "cancelled",
                                Some(PARENT_AUTHORITY_REVOKED),
                                &updated_at,
                            ))
                            .await;
                    }
                } else if !child_settled(&mut replicas[0]).await {
                    replicas[0]
                        .exec(&child_state_sql("working", None, &updated_at))
                        .await;
                }
                let detector = 1 + rng.below(replicas.len() - 1);
                if sees_narrowed_parent(&mut replicas[detector]).await
                    && !child_settled(&mut replicas[detector]).await
                {
                    replicas[detector]
                        .exec(&child_state_sql(
                            "cancelled",
                            Some(PARENT_AUTHORITY_REVOKED),
                            &updated_at,
                        ))
                        .await;
                }
            }
            converge(&mut replicas).await;
            if !child_settled(&mut replicas[2]).await {
                replicas[2]
                    .exec(&child_state_sql(
                        "cancelled",
                        Some(PARENT_AUTHORITY_REVOKED),
                        &at(PROGRESS_ROUNDS + 1, 0),
                    ))
                    .await;
                converge(&mut replicas).await;
            }
            assert_converged(&mut replicas).await;
            let expected = format!(
                "cancelled {PARENT_AUTHORITY_REVOKED} {ASSIGNMENT} 1 \
                 [{{\"workingFolderId\":\"{FOLDER}\",\"capability\":\"edit\"}}]"
            );
            for replica in &mut replicas {
                assert_eq!(
                    child_summary(replica).await,
                    expected,
                    "seed {seed} replica {}",
                    replica.index
                );
                assert_eq!(
                    replica
                        .scalar_text("SELECT capability FROM chat_teammate_working_folder_grants")
                        .await
                        .as_deref(),
                    Some("read"),
                    "seed {seed} replica {}",
                    replica.index
                );
            }
        }
    });
}

/// Reply queued on [`ASSIGNMENT`] in the delivery scenarios.
const QUEUED_REPLY: &str = "item:queued-reply";

/// SQL delivering [`QUEUED_REPLY`] at `at`, naming the revision the run received.
fn delivery_sql(at: &str) -> String {
    format!(
        "UPDATE chat_work_assignment_inputs
         SET delivery_state = 'delivered', delivered_at = {at},
             delivered_revision_id = {revision}
         WHERE id = {input} AND delivery_state = 'pending';",
        at = quoted(at),
        revision = quoted(&format!("{QUEUED_REPLY}:0")),
        input = quoted(&input_id(QUEUED_REPLY)),
    )
}

/// SQL cancelling [`QUEUED_REPLY`] while it is pending.
fn input_cancel_sql() -> String {
    format!(
        "UPDATE chat_work_assignment_inputs SET delivery_state = 'ignored'
         WHERE id = {input} AND delivery_state = 'pending';",
        input = quoted(&input_id(QUEUED_REPLY)),
    )
}

/// [`QUEUED_REPLY`]'s delivery state, delivery time, and delivered revision.
async fn queued_reply_delivery(replica: &mut Replica) -> String {
    replica
        .scalar_text(
            "SELECT delivery_state || ' ' || coalesce(delivered_at, '-') || ' '
                    || coalesce(delivered_revision_id, '-')
             FROM chat_work_assignment_inputs WHERE message_item_id = 'item:queued-reply'",
        )
        .await
        .expect("the input arrived")
}

/// The executing device delivers a queued reply while another device cancels it; whatever the
/// exchange order, every device shows it delivered with the revision the run received, since
/// the run already read it.
#[test]
fn chat_simulated_delivery_against_input_cancel_keeps_the_delivery() {
    block_on(async {
        for seed in 0..PROGRESS_SEEDS {
            let mut rng = Rng(seed ^ 0xde11);
            let mut replicas = conversation_replicas(3).await;
            replicas[0]
                .exec(&format!(
                    "{}{}{}",
                    coordination_sql(),
                    message_sql(
                        QUEUED_REPLY,
                        Some(&thread()),
                        "c003",
                        "Then update the docs"
                    ),
                    input_sql(QUEUED_REPLY, ASSIGNMENT, 2, "queued_continuation"),
                ))
                .await;
            converge(&mut replicas).await;
            let delivered_at = at(2, rng.below(60));
            replicas[0].exec(&delivery_sql(&delivered_at)).await;
            replicas[1 + rng.below(2)].exec(&input_cancel_sql()).await;
            pull_at_random(&mut replicas, &mut rng).await;
            converge(&mut replicas).await;
            assert_converged(&mut replicas).await;
            for replica in &mut replicas {
                assert_eq!(
                    queued_reply_delivery(replica).await,
                    format!("delivered {delivered_at} {QUEUED_REPLY}:0"),
                    "seed {seed} replica {}",
                    replica.index
                );
            }
        }
    });
}

/// Whether [`CHILD`] is settled on `replica`; false while it has not arrived.
async fn child_settled(replica: &mut Replica) -> bool {
    replica
        .scalar_text(
            "SELECT state FROM chat_work_assignments
             WHERE id = 'assignment:delegated' AND settled_at IS NOT NULL",
        )
        .await
        .is_some()
}

/// SQL creating [`ASSIGNMENT`] on `executor` with the model override `model`.
fn overridden_assignment_sql(executor: &str, model: &str) -> String {
    format!(
        "INSERT INTO chat_work_assignments
            (id, reply_thread_id, teammate_id, triggering_message_item_id, executing_device_id,
             override_model_selection_schema_version, override_model_selection_data, state,
             created_at, updated_at)
         VALUES ({id}, {thread}, {teammate}, {mention}, {executor}, 1, {data}, 'queued',
                 {created}, {created});",
        id = quoted(ASSIGNMENT),
        thread = quoted(&thread()),
        teammate = quoted(TEAMMATE),
        mention = quoted(MENTION),
        executor = quoted(executor),
        data = quoted(&format!(
            "{{\"providerManagedModel\":false,\"modelId\":\"{model}\",\"modelOptions\":[]}}"
        )),
        created = quoted(CREATED),
    )
}

/// Two devices Start one mention concurrently with different model overrides; the created group
/// is fixed with the request, so every device keeps one whole override from one of the creates,
/// beside that create's executing device.
#[test]
fn chat_concurrent_overridden_starts_keep_one_whole_override() {
    block_on(async {
        let mut replicas = conversation_replicas(3).await;
        replicas[0]
            .exec(&format!(
                "{}{}{}",
                message_sql(ROOT, None, "c001", "Root"),
                thread_sql(),
                message_sql(MENTION, Some(&thread()), "c001", "@Reviewer please check"),
            ))
            .await;
        converge(&mut replicas).await;

        for (index, executor, model) in [(0, "device-0", "model-a"), (1, "device-1", "model-b")] {
            replicas[index]
                .exec(&overridden_assignment_sql(executor, model))
                .await;
        }
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;

        let mut created = Vec::new();
        for replica in &mut replicas {
            created.push(
                replica
                    .scalar_text(
                        "SELECT executing_device_id || ' '
                                || override_model_selection_schema_version || ' '
                                || json_extract(override_model_selection_data, '$.modelId')
                         FROM chat_work_assignments",
                    )
                    .await
                    .expect("the assignment arrived"),
            );
        }
        assert!(
            ["device-0 1 model-a", "device-1 1 model-b"].contains(&created[0].as_str()),
            "the override stays with its own create: {}",
            created[0]
        );
        assert!(
            created.iter().all(|row| *row == created[0]),
            "every device agrees on the override: {created:?}"
        );
    });
}

/// Seeds of the cancel against recurrence progress scenario.
const RECURRENCE_SEEDS: u64 = 8;

/// Rounds of one cancel against recurrence progress scenario.
const RECURRENCE_ROUNDS: usize = 5;

/// SQL repeating [`SCHEDULED`] every two hours, deferred through quiet periods.
fn recurrence_sql() -> String {
    format!(
        "INSERT INTO chat_scheduled_message_recurrences
            (id, scheduled_message_id, rule_kind, interval_minutes, time_zone, quiet_policy,
             next_occurrence_at, fired_count, created_at, updated_at)
         VALUES ({id}, {scheduled}, 'interval', 120, 'America/Mexico_City', 'defer', {due}, 0,
                 {created}, {created});",
        id = quoted(&key(RECURRENCE_KEY_PREFIX, &[SCHEDULED])),
        scheduled = quoted(SCHEDULED),
        due = quoted(&at(30, 0)),
        created = quoted(CREATED),
    )
}

/// SQL recording that [`SCHEDULED`] posted `fired` occurrences, the next one due at `next`.
fn advance_sql(fired: usize, next: &str, updated_at: &str) -> String {
    format!(
        "UPDATE chat_scheduled_message_recurrences
         SET next_occurrence_at = {next}, fired_count = {fired}, updated_at = {updated_at}
         WHERE scheduled_message_id = {scheduled};",
        next = quoted(next),
        updated_at = quoted(updated_at),
        scheduled = quoted(SCHEDULED),
    )
}

/// [`SCHEDULED`]'s recurrence as `rule fired_count`, or `None` once it is gone.
async fn recurrence_progress(replica: &mut Replica) -> Option<String> {
    replica
        .scalar_text(
            "SELECT recurrence.rule_kind || ' ' || recurrence.fired_count
             FROM chat_scheduled_message_recurrences recurrence
             JOIN chat_scheduled_messages message
               ON message.id = recurrence.scheduled_message_id",
        )
        .await
}

#[test]
fn chat_scheduled_recurrences_round_trip_and_leave_with_their_message() {
    block_on(async {
        let mut replicas = conversation_replicas(2).await;
        replicas[0]
            .exec(&format!("{}{}", scheduled_sql(), recurrence_sql()))
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        assert_eq!(
            replicas[1]
                .scalar_text(
                    "SELECT id || ' ' || interval_minutes || ' ' || time_zone || ' '
                            || quiet_policy || ' ' || next_occurrence_at
                     FROM chat_scheduled_message_recurrences",
                )
                .await,
            Some(format!(
                "{} 120 America/Mexico_City defer {}",
                key(RECURRENCE_KEY_PREFIX, &[SCHEDULED]),
                at(30, 0)
            ))
        );

        replicas[0]
            .exec(&advance_sql(1, &at(150, 0), &at(31, 0)))
            .await;
        converge(&mut replicas).await;
        assert_eq!(
            recurrence_progress(&mut replicas[1]).await.as_deref(),
            Some("interval 1")
        );

        replicas[1]
            .exec("DELETE FROM chat_scheduled_messages WHERE id = 'scheduled:reminder'")
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        assert_eq!(
            replicas[0]
                .count("SELECT count(*) FROM chat_scheduled_message_recurrences")
                .await,
            0,
            "a cancel removes the series with its message"
        );
    });
}

/// The executing device advances a series while the other devices cancel it at random; a
/// cancel anywhere ends the series on every device whatever the exchange order, and without
/// one every device shows the executing device's last advance.
#[test]
fn chat_simulated_cancel_against_recurrence_progress_keeps_the_cancel() {
    block_on(async {
        for seed in 0..RECURRENCE_SEEDS {
            let mut rng = Rng(seed ^ 0x5e71);
            let mut replicas = conversation_replicas(3).await;
            replicas[0]
                .exec(&format!("{}{}", scheduled_sql(), recurrence_sql()))
                .await;
            converge(&mut replicas).await;
            let mut cancelled = false;
            let mut fired = 0;
            for round in 0..RECURRENCE_ROUNDS {
                let executor_has_series = recurrence_progress(&mut replicas[0]).await.is_some();
                if executor_has_series && rng.chance(80) {
                    fired += 1;
                    replicas[0]
                        .exec(&advance_sql(
                            fired,
                            &at(30 + 120 * fired, 0),
                            &at(round + 1, rng.below(60)),
                        ))
                        .await;
                }
                for replica in replicas.iter_mut().skip(1) {
                    if recurrence_progress(replica).await.is_some() && rng.chance(15) {
                        replica
                            .exec(
                                "DELETE FROM chat_scheduled_messages
                                 WHERE id = 'scheduled:reminder'",
                            )
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
            let expected = (!cancelled).then(|| format!("interval {fired}"));
            for replica in &mut replicas {
                assert_eq!(
                    recurrence_progress(replica).await,
                    expected,
                    "seed {seed} replica {}",
                    replica.index
                );
                if cancelled {
                    assert_eq!(
                        replica
                            .count("SELECT count(*) FROM chat_scheduled_message_recurrences")
                            .await,
                        0,
                        "seed {seed} replica {}",
                        replica.index
                    );
                }
            }
        }
    });
}
