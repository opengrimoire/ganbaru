//! Chat identity and agent configuration across replicas: participants and agents arrive with
//! their provider family and policy revisions, a crash while sealing or applying them leaves
//! nothing partial, and concurrent edits of an agent's current revision settle on one value
//! every replica agrees on. The vault agent settings singleton is published by seeding, stays
//! one row everywhere, and is never deleted; project agent settings leave with their project.
//! Concurrent agent access edits at vault, project, and channel scope keep the narrower level
//! whatever order the replicas exchange in, with an inherited level counting as the widest,
//! while instructions and a channel's model override are last writer wins, the override whole
//! or absent. Conversations, channels, and memberships follow: a concurrent removal wins over a
//! concurrent re-add, and a later re-add that saw the removal wins over it. The timeline
//! follows: messages, revisions, reply threads, and read cursors arrive whole, concurrent posts
//! that take one order key read in one order everywhere, read cursors only move forward, and the
//! Chat hook is handed applied messages and revisions to refresh search. Attachment references
//! arrive by value with their revision and name only non-empty files. Channel notes follow their
//! channel, and a deletion of one wins over a concurrent edit. Standing push approvals follow
//! their channel and folder, and a revocation wins over a concurrent approval. Work assignments
//! carry the model and instructions they were requested with. Device local Chat rows stay on
//! their device.

use std::sync::{Arc, Mutex};

use ganbaru_sync_contracts::chat_push_approvals::PUSH_APPROVAL_KEY_PREFIX;
use ganbaru_sync_contracts::{
    Cause, Change, ChangeAction, Content, Envelope, RowKey, TableId, derived_key,
};
use sqlx::SqliteConnection;

use super::authorization::{forge, view};
use super::block_on;
use super::fixture::{
    FixtureHook, LISTS_TABLE, OVERRIDES_TABLE, OverrideHook, TEST_MANIFEST, test_adapters,
};
use super::replica::{Replica, converge, device_id, forge_change, prepared, quoted};
use super::sim::Rng;
use crate::domains::chat::LOCAL_OWNER_PARTICIPANT_KEY;
use crate::error::failpoints;
use crate::hooks::{ChangeSink, HookFuture, MaterializeHook};
use crate::manifest::TableSpec;
use crate::manifest::vault::chat::{
    AGENTS_TABLE, APP_SETTINGS_TABLE, CHANNEL_MEMBERSHIP_KEY_PREFIX, CHANNELS_TABLE,
    CONVERSATION_CURSOR_KEY_PREFIX, CONVERSATIONS_TABLE, MEMBERSHIP_KEY_PREFIX, MESSAGES_TABLE,
    PARTICIPANTS_TABLE, POLICY_REVISIONS_TABLE, REPLY_CURSOR_KEY_PREFIX, REPLY_CURSORS_TABLE,
    REPLY_THREAD_KEY_PREFIX, REPLY_THREADS_TABLE, REVISIONS_TABLE,
};
use crate::manifest::vault::projects::PROJECTS_TABLE;
use crate::provenance::row_creating_devices;
use crate::{Engine, HoldReason, MAX_UNSEEN_PRESENTATIONS, StoreOutcome, SyncError, SyncResult};

const SALT: u64 = 73;
const AGENT: &str = "participant:reviewer";
const CREATED: &str = "2026-10-24T09:00:00.000Z";

/// The model selection of the agent's first policy revision.
const FIRST_MODEL: &str = r#"{"modelId":"first"}"#;

/// Seeds of the concurrent current revision scenario.
const POLICY_SEEDS: u64 = 8;

/// Edit rounds of one current revision scenario.
const POLICY_ROUNDS: usize = 4;

/// How far a replica's clock moves ahead so its concurrent edit carries the later clock.
const LATER_MS: u64 = 60_000;

/// Replicas that each seeded their existing rows at vault open, as the app does, with operations
/// exchanged, so the seeded local owner, vault agent settings, and routine projects are published
/// everywhere. Built once per count and copied, since converging the seeded rows dominates every
/// Chat test.
async fn replicas(count: usize) -> Vec<Replica> {
    prepared(&format!("chat seeded {count}"), async || {
        let mut replicas = Vec::with_capacity(count);
        for index in 0..count {
            let mut replica = Replica::new(index, SALT).await;
            let now = replica.now_ms;
            replica
                .engine
                .seed_captures(&mut replica.conn, now)
                .await
                .unwrap();
            replicas.push(replica);
        }
        converge(&mut replicas).await;
        replicas
    })
    .await
}

/// A timestamp `minute` minutes after [`CREATED`], at `second` seconds.
fn at(minute: usize, second: usize) -> String {
    format!(
        "2026-10-24T{:02}:{:02}:{:02}.000Z",
        9 + minute / 60,
        minute % 60,
        second
    )
}

/// SQL creating [`AGENT`] with an avatar and its provider family.
fn agent_sql() -> String {
    format!(
        "INSERT INTO chat_participants
            (id, participant_kind, display_name, avatar_data, created_at, updated_at)
         VALUES ({agent}, 'ai_agent', 'Reviewer', {avatar}, {created}, {created});
         INSERT INTO chat_agents (participant_id, provider_family_id, created_at, updated_at)
         VALUES ({agent}, 'codex', {created}, {created});",
        agent = quoted(AGENT),
        avatar = quoted(r#"{"kind":"emoji","emoji":"R"}"#),
        created = quoted(CREATED),
    )
}

/// SQL adding policy revision `id` of [`AGENT`] with `model` and making it current.
fn policy_sql(id: &str, model: &str, created_at: &str) -> String {
    format!(
        "INSERT INTO chat_agent_policy_revisions (id, agent_id, model_selection_data, created_at)
         VALUES ({id}, {agent}, {model}, {created_at});
         UPDATE chat_agents
         SET current_policy_revision_id = {id}, updated_at = {created_at}
         WHERE participant_id = {agent};",
        id = quoted(id),
        agent = quoted(AGENT),
        model = quoted(model),
        created_at = quoted(created_at),
    )
}

/// The model selection policy revision `id` holds in the scenarios that write it.
fn model_of(id: &str) -> String {
    format!(r#"{{"modelId":"{id}"}}"#)
}

/// The identity rows every scenario starts from: the agent with its first policy.
fn identity_sql() -> String {
    format!(
        "{}{}",
        agent_sql(),
        policy_sql("policy-0", FIRST_MODEL, CREATED)
    )
}

async fn assert_converged(replicas: &mut [Replica]) {
    let first = replicas[0].domain_dump().await;
    for replica in replicas.iter_mut().skip(1) {
        assert_eq!(
            replica.domain_dump().await,
            first,
            "replica {} diverged",
            replica.index
        );
    }
}

/// The current policy revision of [`AGENT`] and the model selection of the revision it names,
/// as `id model`.
async fn current_policy(replica: &mut Replica) -> String {
    replica
        .scalar_text(
            "SELECT policy.id || ' ' || policy.model_selection_data FROM chat_agents agent
             JOIN chat_agent_policy_revisions policy
               ON policy.id = agent.current_policy_revision_id
             WHERE agent.participant_id = 'participant:reviewer'",
        )
        .await
        .expect("the agent names a policy that arrived")
}

/// Captured replicated Chat rows as `table key`, in table and key order.
async fn chat_captures(replica: &mut Replica) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT table_id || ' ' || row_key FROM sync_capture
         WHERE table_id BETWEEN ? AND ? ORDER BY table_id, row_key",
    )
    .bind(i64::from(PARTICIPANTS_TABLE.0))
    .bind(i64::from(CHANNELS_TABLE.0))
    .fetch_all(&mut replica.conn)
    .await
    .unwrap()
}

#[test]
fn chat_identity_rows_round_trip_with_every_replicated_value() {
    block_on(async {
        let mut replicas = replicas(2).await;
        replicas[0].exec(&identity_sql()).await;
        replicas[0]
            .exec(&policy_sql("policy-1", &model_of("policy-1"), &at(1, 0)))
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;

        let reader = &mut replicas[1];
        assert_eq!(
            reader
                .scalar_text(
                    "SELECT display_name || ' ' || avatar_data || ' ' || provider_family_id
                     FROM chat_participants participant
                     JOIN chat_agents agent ON agent.participant_id = participant.id
                     WHERE participant.id = 'participant:reviewer'",
                )
                .await
                .as_deref(),
            Some(r#"Reviewer {"kind":"emoji","emoji":"R"} codex"#)
        );
        assert_eq!(
            current_policy(reader).await,
            format!("policy-1 {}", model_of("policy-1"))
        );
        // Display numbers follow `(created_at, id)` order on every replica.
        assert_eq!(
            reader
                .scalar_text(
                    "SELECT group_concat(id || '=' || revision, ' ')
                     FROM (SELECT id, revision FROM chat_agent_policy_revisions_numbered
                           ORDER BY revision)",
                )
                .await
                .as_deref(),
            Some("policy-0=1 policy-1=2")
        );
        assert_eq!(
            reader
                .scalar_text(
                    "SELECT id || '=' || revision || ' ' || interaction_mode
                     FROM chat_agent_effective_policies",
                )
                .await
                .as_deref(),
            Some("policy-1=2 build")
        );
    });
}

/// Arms `name`, checks that `call` fails there leaving `replica` exactly as it was, then runs
/// the call again disarmed.
pub(super) async fn after_rollback<T: std::fmt::Debug>(
    replica: &mut Replica,
    name: &'static str,
    mut call: impl AsyncFnMut(&mut Replica) -> SyncResult<T>,
) -> T {
    let before = replica.snapshot().await;
    failpoints::arm(name);
    let result = call(replica).await;
    failpoints::clear();
    match result {
        Err(SyncError::Failpoint(hit)) => assert_eq!(hit, name),
        other => panic!("{name} did not fail: {other:?}"),
    }
    assert!(replica.snapshot().await == before, "{name} left changes");
    replica.check_invariants().await;
    call(replica).await.unwrap()
}

#[test]
fn chat_a_crash_while_sealing_or_applying_identity_rows_leaves_nothing_partial() {
    block_on(async {
        let mut replicas = replicas(2).await;
        replicas[0]
            .exec(&format!("{}{}", identity_sql(), conversation_sql()))
            .await;
        let captured = chat_captures(&mut replicas[0]).await;
        // Three identity rows, then the conversation, two memberships, the AI channel
        // membership, and the channel.
        assert_eq!(captured.len(), 8, "{captured:?}");
        for name in ["seal.reserve", "seal.before_commit"] {
            let report = after_rollback(&mut replicas[0], name, async |r: &mut Replica| {
                r.seal().await
            })
            .await;
            assert_eq!(report.sealed_ops, 1, "{name}: the rows seal together");
            assert!(chat_captures(&mut replicas[0]).await.is_empty());
            if name == "seal.reserve" {
                // A new current policy gives the next failpoint rows to seal.
                replicas[0]
                    .exec(&policy_sql("policy-1", &model_of("policy-1"), &at(1, 0)))
                    .await;
            }
        }

        let (author, rest) = replicas.split_at_mut(1);
        let reader = &mut rest[0];
        let known = reader.stored_vector().await;
        let ops = author[0].ops_since(&known).await;
        reader.receive(&ops).await;
        assert_eq!(
            reader
                .count(
                    "SELECT count(*) FROM chat_agents WHERE participant_id = 'participant:reviewer'"
                )
                .await,
            0
        );
        after_rollback(reader, "apply.before_commit", async |r: &mut Replica| {
            r.apply_all().await
        })
        .await;
        assert_eq!(
            current_policy(reader).await,
            format!("policy-1 {}", model_of("policy-1"))
        );
        assert_eq!(channel_membership_state(reader).await, "active");
        assert_converged(&mut replicas).await;
    });
}

/// Two devices point the agent at different revisions concurrently: the pointer is a plain
/// register, so the edit with the later clock wins everywhere, in either order, and a later edit
/// that saw it replaces it.
#[test]
fn chat_concurrent_current_revisions_converge_on_the_later_edit() {
    block_on(async {
        for later in [0_usize, 1] {
            let mut replicas = replicas(2).await;
            replicas[0].exec(&identity_sql()).await;
            converge(&mut replicas).await;

            for (index, replica) in replicas.iter_mut().enumerate() {
                let id = format!("policy-from-{index}");
                replica
                    .exec(&policy_sql(&id, &model_of(&id), &at(1, 0)))
                    .await;
            }
            replicas[later].now_ms += LATER_MS;
            converge(&mut replicas).await;
            assert_converged(&mut replicas).await;
            let winner = format!("policy-from-{later}");
            for replica in &mut replicas {
                assert_eq!(
                    current_policy(replica).await,
                    format!("{winner} {}", model_of(&winner)),
                    "later edit from replica {later}"
                );
                // Both revisions arrived and keep their display numbers.
                assert_eq!(
                    replica
                        .count("SELECT count(*) FROM chat_agent_policy_revisions")
                        .await,
                    3
                );
            }

            // A later edit that is not concurrent replaces the winner.
            let other = 1 - later;
            replicas[other]
                .exec(&policy_sql(
                    "policy-next",
                    &model_of("policy-next"),
                    &at(2, 0),
                ))
                .await;
            converge(&mut replicas).await;
            assert_converged(&mut replicas).await;
            assert_eq!(
                current_policy(&mut replicas[later]).await,
                format!("policy-next {}", model_of("policy-next"))
            );
        }
    });
}

/// The agent's on, off, and hidden state, as `archived hidden` with each `-` when null.
async fn lifecycle(replica: &mut Replica) -> String {
    replica
        .scalar_text(&format!(
            "SELECT coalesce(archived_at, '-') || ' ' || coalesce(hidden_at, '-')
             FROM chat_participants WHERE id = {}",
            quoted(AGENT)
        ))
        .await
        .expect("the agent arrived")
}

/// One device archives an agent that is off, hiding it, while another turns it on. The off,
/// hidden, and on states share one register, so the later edit wins whole and no replica reads
/// a hidden agent that is on.
#[test]
fn chat_a_concurrent_archive_and_turn_on_settle_as_one_state() {
    block_on(async {
        for later in [0_usize, 1] {
            let mut replicas = replicas(2).await;
            replicas[0]
                .exec(&format!(
                    "{}UPDATE chat_participants SET archived_at = {created} WHERE id = {agent};",
                    identity_sql(),
                    created = quoted(CREATED),
                    agent = quoted(AGENT),
                ))
                .await;
            converge(&mut replicas).await;

            replicas[0]
                .exec(&format!(
                    "UPDATE chat_participants SET hidden_at = {} WHERE id = {};",
                    quoted(&at(1, 0)),
                    quoted(AGENT)
                ))
                .await;
            replicas[1]
                .exec(&format!(
                    "UPDATE chat_participants SET archived_at = NULL, hidden_at = NULL
                     WHERE id = {};",
                    quoted(AGENT)
                ))
                .await;
            replicas[later].now_ms += LATER_MS;
            converge(&mut replicas).await;
            assert_converged(&mut replicas).await;
            let expected = if later == 0 {
                format!("{CREATED} {}", at(1, 0))
            } else {
                "- -".to_string()
            };
            for replica in &mut replicas {
                assert_eq!(lifecycle(replica).await, expected, "later edit {later}");
            }
        }
    });
}

/// The random key of an agent a person named, which sync lets a person delete.
const NAMED: &str = "agent:0123456789abcdef0123456789abcdef";

/// SQL creating [`NAMED`] with its first policy revision, a member of [`CONVERSATION`] with AI
/// channel access there. It needs the rows of [`conversation_replicas`].
fn named_agent_sql() -> String {
    format!(
        "INSERT INTO chat_participants
            (id, participant_kind, display_name, created_at, updated_at)
         VALUES ({named}, 'ai_agent', 'Named', {created}, {created});
         INSERT INTO chat_agents (participant_id, provider_family_id, created_at, updated_at)
         VALUES ({named}, 'codex', {created}, {created});
         INSERT INTO chat_agent_policy_revisions (id, agent_id, model_selection_data, created_at)
         VALUES ('policy-named', {named}, {model}, {created});
         UPDATE chat_agents SET current_policy_revision_id = 'policy-named'
         WHERE participant_id = {named};
         INSERT INTO chat_conversation_memberships
            (conversation_id, participant_id, sync_key, membership_role, created_at, updated_at)
         VALUES ({conversation}, {named}, {member_key}, 'member', {created}, {created});",
        named = quoted(NAMED),
        model = quoted(FIRST_MODEL),
        created = quoted(CREATED),
        conversation = quoted(CONVERSATION),
        member_key = quoted(&key(MEMBERSHIP_KEY_PREFIX, &[CONVERSATION, NAMED])),
    )
}

/// SQL granting [`NAMED`] AI channel access in [`CONVERSATION`], a message it posts there that
/// mentions it, a work assignment of it the message triggers, and a scheduled message mentioning
/// it: what a device that has not seen the agent's deletion writes once it starts using the agent.
fn named_agent_use_sql() -> String {
    format!(
        "INSERT INTO chat_agent_channel_memberships
            (conversation_id, agent_id, sync_key, created_at, updated_at)
         VALUES ({conversation}, {named}, {access_key}, {created}, {created});
         {root}
         {thread}
         INSERT INTO chat_conversation_items
            (id, conversation_id, reply_thread_id, item_kind, order_key, created_at)
         VALUES ('item:named', {conversation}, {reply_thread}, 'message', 'c002', {created});
         INSERT INTO chat_communication_messages
            (item_id, author_participant_id, created_at, author_label_snapshot)
         VALUES ('item:named', {named}, {created}, 'Named');
         INSERT INTO chat_communication_message_revisions
            (id, message_item_id, normalized_markdown, created_at)
         VALUES ('item:named:0', 'item:named', 'Posted by the agent', {created});
         UPDATE chat_communication_messages SET current_revision_id = 'item:named:0'
         WHERE item_id = 'item:named';
         INSERT INTO chat_message_references
            (id, message_revision_id, reference_kind, label_snapshot, plain_text_projection,
             start_offset, end_offset, ordinal, created_at)
         VALUES ('reference:named', 'item:named:0', 'participant', '@Named', '@Named', 0, 6, 0,
                 {created});
         INSERT INTO chat_participant_reference_targets (reference_id, participant_id)
         VALUES ('reference:named', {named});
         INSERT INTO chat_scheduled_messages
            (id, client_command_id, channel_id, executing_device_id, request_data, scheduled_for,
             created_at, updated_at)
         VALUES ('scheduled:named', 'command:named', {channel}, 'device-1',
                 '{{\"markdown\":\"@Named check\"}}', {created}, {created}, {created});
         INSERT INTO chat_scheduled_message_references
            (scheduled_message_id, ordinal, reference_kind, label_snapshot, plain_text_projection,
             start_offset, end_offset, participant_id, participant_kind)
         VALUES ('scheduled:named', 0, 'participant', '@Named', '@Named', 0, 6, {named},
                 'ai_agent');
         INSERT INTO chat_work_assignments
            (id, reply_thread_id, agent_id, triggering_message_item_id, executing_device_id,
             model_selection_schema_version, model_selection_data, instructions, state,
             created_at, updated_at)
         VALUES ('assignment:named', {reply_thread}, {named}, {root_item}, 'device-1', 1,
                 '{{\"providerManagedModel\":false,\"modelId\":\"first\",\"modelOptions\":[]}}',
                 '', 'queued', {created}, {created});",
        conversation = quoted(CONVERSATION),
        named = quoted(NAMED),
        access_key = quoted(&key(CHANNEL_MEMBERSHIP_KEY_PREFIX, &[CONVERSATION, NAMED])),
        created = quoted(CREATED),
        root = message_sql(ROOT, None, "c001", "Root"),
        thread = thread_sql(),
        reply_thread = quoted(&thread()),
        root_item = quoted(ROOT),
        channel = quoted(CHANNEL),
    )
}

/// The rows of [`NAMED`] and of what hangs from it on `replica`, as `participant agent policy
/// memberships access assignments authored`, each a count.
async fn named_agent_rows(replica: &mut Replica) -> String {
    replica
        .scalar_text(&format!(
            "SELECT (SELECT count(*) FROM chat_participants WHERE id = {named})
                 || ' ' || (SELECT count(*) FROM chat_agents WHERE participant_id = {named})
                 || ' ' || (SELECT count(*) FROM chat_agent_policy_revisions
                            WHERE agent_id = {named})
                 || ' ' || (SELECT count(*) FROM chat_conversation_memberships
                            WHERE participant_id = {named})
                 || ' ' || (SELECT count(*) FROM chat_agent_channel_memberships
                            WHERE agent_id = {named})
                 || ' ' || (SELECT count(*) FROM chat_work_assignments WHERE agent_id = {named})
                 || ' ' || (SELECT count(*) FROM chat_communication_messages
                            WHERE author_participant_id = {named})",
            named = quoted(NAMED),
        ))
        .await
        .expect("an aggregate always has a row")
}

/// Deleting an agent a person named removes its participant, agent row, policy revisions, and
/// memberships on every replica.
#[test]
fn chat_a_named_agent_deletion_removes_it_everywhere() {
    block_on(async {
        let mut replicas = conversation_replicas(2).await;
        replicas[0].exec(&named_agent_sql()).await;
        converge(&mut replicas).await;
        assert_eq!(named_agent_rows(&mut replicas[1]).await, "1 1 1 1 0 0 0");

        replicas[1]
            .exec(&format!(
                "DELETE FROM chat_participants WHERE id = {}",
                quoted(NAMED)
            ))
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(named_agent_rows(replica).await, "0 0 0 0 0 0 0");
        }
    });
}

/// One device deletes a named agent while another, unaware, grants it channel access, posts a
/// message as it, and starts a work assignment of it. Each replica receives the other's writes
/// after its own, which covers both arrival orders. The deletion holds everywhere: the access, the
/// assignment, and the scheduled mention leave with the agent without a recovery offer, the
/// scheduled message stays, and the message stays readable without its author, its mention
/// naming the absent agent.
#[test]
fn chat_a_named_agent_deletion_against_concurrent_use_holds_and_keeps_its_messages() {
    block_on(async {
        let mut replicas = conversation_replicas(2).await;
        replicas[0].exec(&named_agent_sql()).await;
        converge(&mut replicas).await;

        replicas[0]
            .exec(&format!(
                "DELETE FROM chat_participants WHERE id = {}",
                quoted(NAMED)
            ))
            .await;
        replicas[1].exec(&named_agent_use_sql()).await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;

        for replica in &mut replicas {
            assert_eq!(
                named_agent_rows(replica).await,
                "0 0 0 0 0 0 0",
                "replica {}",
                replica.index
            );
            assert_eq!(
                replica
                    .scalar_text(
                        "SELECT coalesce(author_participant_id, '-') || ' ' || author_label_snapshot
                         FROM chat_communication_messages WHERE item_id = 'item:named'",
                    )
                    .await
                    .as_deref(),
                Some("- Named"),
                "replica {}",
                replica.index
            );
            assert_eq!(
                replica
                    .scalar_text(
                        "SELECT revision.normalized_markdown
                         FROM chat_communication_messages message
                         JOIN chat_communication_message_revisions revision
                           ON revision.id = message.current_revision_id
                         WHERE message.item_id = 'item:named'",
                    )
                    .await
                    .as_deref(),
                Some("Posted by the agent"),
                "replica {}",
                replica.index
            );
            assert_eq!(
                replica
                    .scalar_text(
                        "SELECT (SELECT participant_id FROM chat_participant_reference_targets
                                 WHERE reference_id = 'reference:named')
                             || ' ' || (SELECT count(*) FROM chat_scheduled_messages
                                        WHERE id = 'scheduled:named')
                             || ' ' || (SELECT count(*) FROM chat_scheduled_message_references
                                        WHERE scheduled_message_id = 'scheduled:named')",
                    )
                    .await,
                Some(format!("{NAMED} 1 0")),
                "replica {}",
                replica.index
            );
            let entries = replica
                .engine
                .recovery_entries(&mut replica.conn, &replica.ctx)
                .await
                .unwrap();
            assert!(entries.is_empty(), "replica {}: {entries:?}", replica.index);
        }
    });
}

/// A tombstone that would delete the local person, a built-in agent, or an agent's row or policy
/// apart from its participant's deletion is invalid by its bytes alone, so the rows stay on the
/// receiver. A policy revision tombstone that names a named agent as its cause but belongs to
/// another agent passes those checks, and the receiver refuses to delete it while its agent exists.
#[test]
fn chat_only_a_named_agent_deletion_is_accepted() {
    block_on(async {
        let tombstone = |table, row: &str, cause: Option<(TableId, &str)>| Change {
            table,
            row: RowKey::new(row).unwrap(),
            action: ChangeAction::Tombstone,
            groups: Vec::new(),
            replaced_by: None,
            cause: cause.map(|(table, row)| Cause {
                table,
                row: RowKey::new(row).unwrap(),
            }),
        };
        for change in [
            tombstone(PARTICIPANTS_TABLE, OWNER, None),
            tombstone(PARTICIPANTS_TABLE, AGENT, None),
            tombstone(AGENTS_TABLE, AGENT, Some((PARTICIPANTS_TABLE, AGENT))),
            tombstone(AGENTS_TABLE, NAMED, None),
            tombstone(AGENTS_TABLE, NAMED, Some((PARTICIPANTS_TABLE, OWNER))),
            tombstone(POLICY_REVISIONS_TABLE, "policy-named", None),
            tombstone(
                POLICY_REVISIONS_TABLE,
                "policy-0",
                Some((PARTICIPANTS_TABLE, AGENT)),
            ),
        ] {
            let mut replicas = conversation_replicas(2).await;
            replicas[0].exec(&named_agent_sql()).await;
            converge(&mut replicas).await;
            let description = format!("{change:?}");
            let forged = forge_change(&mut replicas[0], change).await;
            assert_eq!(
                replicas[1].receive(&[forged]).await,
                vec![StoreOutcome::Stored {
                    held: Some(HoldReason::Invalid)
                }],
                "{description}"
            );
            replicas[1].apply_all().await.unwrap();
            assert_eq!(
                named_agent_rows(&mut replicas[1]).await,
                "1 1 1 1 0 0 0",
                "{description}"
            );
            assert_eq!(
                replicas[1]
                    .scalar_text(&format!(
                        "SELECT count(*) || '' FROM chat_participants WHERE id IN ({}, {})",
                        quoted(OWNER),
                        quoted(AGENT)
                    ))
                    .await
                    .as_deref(),
                Some("2"),
                "{description}"
            );
        }
        let mut replicas = conversation_replicas(2).await;
        replicas[0].exec(&named_agent_sql()).await;
        converge(&mut replicas).await;
        let forged = forge_change(
            &mut replicas[0],
            tombstone(
                POLICY_REVISIONS_TABLE,
                "policy-0",
                Some((PARTICIPANTS_TABLE, NAMED)),
            ),
        )
        .await;
        replicas[1].receive(&[forged]).await;
        replicas[1].apply_all().await.unwrap();
        assert_eq!(
            replicas[1]
                .scalar_text(&format!(
                    "SELECT agent.current_policy_revision_id
                     FROM chat_agents agent
                     JOIN chat_agent_policy_revisions policy
                       ON policy.id = agent.current_policy_revision_id
                     WHERE agent.participant_id = {}",
                    quoted(AGENT)
                ))
                .await
                .as_deref(),
            Some("policy-0")
        );
        assert_eq!(named_agent_rows(&mut replicas[1]).await, "1 1 1 1 0 0 0");
    });
}

#[test]
fn chat_simulated_concurrent_current_revision_edits_agree_on_one_of_them() {
    block_on(async {
        for seed in 0..POLICY_SEEDS {
            let mut rng = Rng(seed);
            let mut replicas = replicas(3).await;
            replicas[0].exec(&identity_sql()).await;
            converge(&mut replicas).await;
            let mut expected = vec!["policy-0".to_owned()];
            for round in 0..POLICY_ROUNDS {
                let mut written = Vec::new();
                for (index, replica) in replicas.iter_mut().enumerate() {
                    if !rng.chance(70) {
                        continue;
                    }
                    let id = format!("policy-{round}-{index}");
                    replica
                        .exec(&policy_sql(
                            &id,
                            &model_of(&id),
                            &at(round + 1, rng.below(60)),
                        ))
                        .await;
                    written.push(id);
                    if rng.chance(50) {
                        replica.seal().await.unwrap();
                    }
                }
                converge(&mut replicas).await;
                assert_converged(&mut replicas).await;
                if !written.is_empty() {
                    expected = written;
                }
                let current = current_policy(&mut replicas[0]).await;
                let (id, model) = current.split_once(' ').unwrap();
                assert!(
                    expected.iter().any(|candidate| candidate == id),
                    "seed {seed} round {round}: {id} is none of {expected:?}"
                );
                assert_eq!(model, model_of(id), "seed {seed} round {round}");
            }
        }
    });
}

/// Audience state is device local: the conversation insert creates it on the author, and the
/// applier stands the trigger down, so the reader's state comes from the Chat hook the app
/// registers.
#[test]
fn chat_device_local_rows_stay_on_their_device() {
    block_on(async {
        let mut replicas = replicas(2).await;
        replicas[0]
            .exec(&format!("{}{}", identity_sql(), conversation_sql()))
            .await;
        let audience = "SELECT count(*) FROM chat_conversation_audience_state
                        WHERE conversation_id = 'conversation:design'";
        converge(&mut replicas).await;
        assert_eq!(replicas[0].count(audience).await, 1);
        assert_eq!(replicas[1].count(audience).await, 0);
        assert_eq!(
            replicas[1]
                .count("SELECT count(*) FROM chat_conversations WHERE id = 'conversation:design'")
                .await,
            1
        );
    });
}

/// The seeded vault agent settings row is captured by seeding, so every replica publishes it
/// and they agree on one row with the default access and no instructions.
#[test]
fn chat_seeding_publishes_the_vault_agent_settings_and_replicas_agree_on_them() {
    block_on(async {
        let mut fresh = Replica::new(0, SALT).await;
        let now = fresh.now_ms;
        let seeded = fresh
            .engine
            .seed_captures(&mut fresh.conn, now)
            .await
            .unwrap();
        assert!(seeded.contains(&"chat_agent_app_settings"), "{seeded:?}");
        assert_eq!(
            fresh
                .count(&format!(
                    "SELECT count(*) FROM sync_capture
                     WHERE table_id = {} AND row_key = 'vault' AND created = 1",
                    APP_SETTINGS_TABLE.0
                ))
                .await,
            1
        );

        let mut replicas = replicas(3).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(
                app_settings(replica).await,
                "1 ask ",
                "replica {}",
                replica.index
            );
        }

        replicas[2]
            .exec(&instructions_sql(
                Scope::App,
                "Prefer small changes",
                &at(1, 0),
            ))
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(app_settings(replica).await, "1 ask Prefer small changes");
        }
    });
}

/// The vault agent settings as `rows access instructions`.
async fn app_settings(replica: &mut Replica) -> String {
    replica
        .scalar_text(
            "SELECT count(*) || ' ' || max(agent_access) || ' ' || max(agent_instructions)
             FROM chat_agent_app_settings",
        )
        .await
        .expect("an aggregate always has a row")
}

/// A forged tombstone of the vault agent settings is invalid by its bytes alone, so the row
/// stays on the receiver.
#[test]
fn chat_vault_agent_settings_are_never_deleted() {
    block_on(async {
        let mut replicas = replicas(2).await;
        let tombstone = Change {
            table: APP_SETTINGS_TABLE,
            row: RowKey::new("vault").unwrap(),
            action: ChangeAction::Tombstone,
            groups: Vec::new(),
            replaced_by: None,
            cause: None,
        };
        let forged = forge_change(&mut replicas[0], tombstone).await;
        assert_eq!(
            replicas[1].receive(&[forged]).await,
            vec![StoreOutcome::Stored {
                held: Some(HoldReason::Invalid)
            }]
        );
        replicas[1].apply_all().await.unwrap();
        assert_eq!(app_settings(&mut replicas[1]).await, "1 ask ");
    });
}

const OWNER: &str = "participant:local-owner";
const PROJECT: &str = "project-routine-learning";
const FOLDER: &str = "working-folder-routine-learning";
const CONVERSATION: &str = "conversation:design";
pub(super) const CHANNEL: &str = "channel:design";

/// The push URL the approval of [`folder_approval_sql`] names.
const APPROVED_PUSH_URL: &str = "git@example.com:team/app.git";

/// The destination branch the approval of [`folder_approval_sql`] names.
const APPROVED_BRANCH: &str = "main";

/// Seeds of the removal against re-add and narrowing against widening scenario.
const ACCESS_SEEDS: u64 = 8;

/// Edit rounds of one removal against re-add and narrowing against widening scenario.
const ACCESS_ROUNDS: usize = 4;

/// Membership roles, least restrictive first, as the adapter ranks them.
const ROLES: [&str; 2] = ["owner", "member"];

/// Agent access levels, widest first, as the adapter ranks them; `None` inherits the enclosing
/// scope's level and counts as the widest.
const ACCESS_LEVELS: [Option<&str>; 5] =
    [None, Some("own"), Some("ask"), Some("read"), Some("talk")];

/// The derived key `prefix` names over `parts`.
fn key(prefix: &str, parts: &[&str]) -> String {
    derived_key(prefix, parts).unwrap().into_string()
}

/// `text` quoted, or `NULL`.
fn optional(text: Option<&str>) -> String {
    text.map_or_else(|| "NULL".to_owned(), quoted)
}

/// SQL creating [`CONVERSATION`] with its channel, the owner and [`AGENT`] as members, and
/// [`AGENT`]'s AI channel membership. It needs the rows of [`identity_sql`].
fn conversation_sql() -> String {
    conversation_on(PROJECT)
}

/// [`identity_sql`], a conversation of `project`, and the owner's standing approval of
/// [`AGENT`]'s pushes from `folder` in it.
pub(super) fn folder_approval_sql(project: &str, folder: &str) -> String {
    format!(
        "{}{}
         INSERT INTO chat_push_approvals
            (conversation_id, agent_id, working_folder_id, push_url, destination_branch, sync_key,
             approved_by_participant_id, approved_at)
         VALUES ({conversation}, {agent}, {folder}, {url}, {branch}, {key}, {owner}, {created});",
        identity_sql(),
        conversation_on(project),
        conversation = quoted(CONVERSATION),
        agent = quoted(AGENT),
        folder = quoted(folder),
        url = quoted(APPROVED_PUSH_URL),
        branch = quoted(APPROVED_BRANCH),
        key = quoted(&key(
            PUSH_APPROVAL_KEY_PREFIX,
            &[
                CONVERSATION,
                AGENT,
                folder,
                APPROVED_PUSH_URL,
                APPROVED_BRANCH
            ]
        )),
        owner = quoted(OWNER),
        created = quoted(CREATED),
    )
}

/// [`conversation_sql`] on `project`.
fn conversation_on(project: &str) -> String {
    format!(
        "INSERT INTO chat_conversations
            (id, project_id, conversation_kind, last_activity_at, created_at, updated_at)
         VALUES ({conversation}, {project}, 'channel', {created}, {created}, {created});
         INSERT INTO chat_channels (id, project_id, conversation_id, name, created_at, updated_at)
         VALUES ({channel}, {project}, {conversation}, 'design', {created}, {created});
         INSERT INTO chat_conversation_memberships
            (conversation_id, participant_id, sync_key, membership_role, created_at, updated_at)
         VALUES ({conversation}, {owner}, {owner_key}, 'owner', {created}, {created}),
                ({conversation}, {agent}, {agent_key}, 'member', {created}, {created});
         INSERT INTO chat_agent_channel_memberships
            (conversation_id, agent_id, sync_key, created_at, updated_at)
         VALUES ({conversation}, {agent}, {access_key}, {created}, {created});",
        conversation = quoted(CONVERSATION),
        channel = quoted(CHANNEL),
        project = quoted(project),
        owner = quoted(OWNER),
        agent = quoted(AGENT),
        created = quoted(CREATED),
        owner_key = quoted(&key(MEMBERSHIP_KEY_PREFIX, &[CONVERSATION, OWNER])),
        agent_key = quoted(&key(MEMBERSHIP_KEY_PREFIX, &[CONVERSATION, AGENT])),
        access_key = quoted(&key(CHANNEL_MEMBERSHIP_KEY_PREFIX, &[CONVERSATION, AGENT])),
    )
}

/// SQL adding [`PROJECT`]'s agent settings, inheriting the vault's access.
fn project_settings_sql() -> String {
    format!(
        "INSERT INTO chat_project_agent_settings (project_id, created_at, updated_at)
         VALUES ({project}, {created}, {created});",
        project = quoted(PROJECT),
        created = quoted(CREATED),
    )
}

/// A scope that holds agent access and instructions.
#[derive(Clone, Copy, Debug)]
enum Scope {
    /// The vault agent settings singleton.
    App,
    /// [`PROJECT`]'s agent settings.
    Project,
    /// [`CHANNEL`].
    Channel,
}

impl Scope {
    const ALL: [Self; 3] = [Self::App, Self::Project, Self::Channel];

    /// The table and row condition of the scope's row.
    fn row(self) -> (&'static str, &'static str) {
        match self {
            Self::App => ("chat_agent_app_settings", "id = 'vault'"),
            Self::Project => (
                "chat_project_agent_settings",
                "project_id = 'project-routine-learning'",
            ),
            Self::Channel => ("chat_channels", "id = 'channel:design'"),
        }
    }
}

/// SQL setting `scope`'s agent access to `level`, or to inherit when `None`.
fn access_sql(scope: Scope, level: Option<&str>, updated_at: &str) -> String {
    let (table, row) = scope.row();
    format!(
        "UPDATE {table} SET agent_access = {level}, updated_at = {updated_at},
             revision = revision + 1
         WHERE {row};",
        level = optional(level),
        updated_at = quoted(updated_at),
    )
}

/// SQL setting `scope`'s agent instructions to `text`.
fn instructions_sql(scope: Scope, text: &str, updated_at: &str) -> String {
    let (table, row) = scope.row();
    format!(
        "UPDATE {table} SET agent_instructions = {text}, updated_at = {updated_at},
             revision = revision + 1
         WHERE {row};",
        text = quoted(text),
        updated_at = quoted(updated_at),
    )
}

/// The query reading `scope`'s agent access, or `inherit`.
fn access_value(scope: Scope) -> String {
    let (table, row) = scope.row();
    format!("SELECT coalesce(agent_access, 'inherit') FROM {table} WHERE {row}")
}

/// `scope`'s agent access, or `inherit`.
async fn access_level(replica: &mut Replica, scope: Scope) -> String {
    replica
        .scalar_text(&access_value(scope))
        .await
        .unwrap_or_else(|| panic!("the {scope:?} row arrived"))
}

/// `scope`'s agent instructions.
async fn instructions(replica: &mut Replica, scope: Scope) -> String {
    let (table, row) = scope.row();
    replica
        .scalar_text(&format!(
            "SELECT agent_instructions FROM {table} WHERE {row}"
        ))
        .await
        .unwrap_or_else(|| panic!("the {scope:?} row arrived"))
}

/// Replicas sharing the conversation rows and [`PROJECT`]'s agent settings, with every scope's
/// access set to `ask`.
async fn scoped_replicas(count: usize) -> Vec<Replica> {
    let mut replicas = conversation_replicas(count).await;
    replicas[0]
        .exec(&format!(
            "{}{}{}",
            project_settings_sql(),
            access_sql(Scope::Project, Some("ask"), CREATED),
            access_sql(Scope::Channel, Some("ask"), CREATED),
        ))
        .await;
    converge(&mut replicas).await;
    replicas
}

/// SQL setting [`AGENT`]'s conversation membership to `role`, removed at `removed_at` when
/// given.
fn membership_sql(role: &str, removed_at: Option<&str>, updated_at: &str) -> String {
    format!(
        "UPDATE chat_conversation_memberships
         SET membership_role = {role}, removed_at = {removed_at}, updated_at = {updated_at},
             revision = revision + 1
         WHERE participant_id = {agent};",
        role = quoted(role),
        removed_at = optional(removed_at),
        updated_at = quoted(updated_at),
        agent = quoted(AGENT),
    )
}

/// SQL marking [`AGENT`]'s AI channel membership removed at `removed_at`, or present again when
/// `None`.
fn channel_membership_sql(removed_at: Option<&str>, updated_at: &str) -> String {
    format!(
        "UPDATE chat_agent_channel_memberships
         SET removed_at = {removed_at}, updated_at = {updated_at}
         WHERE agent_id = {agent};",
        removed_at = optional(removed_at),
        updated_at = quoted(updated_at),
        agent = quoted(AGENT),
    )
}

/// A channel model override as its schema version and selection.
type ChannelModel<'a> = (i64, &'a str);

/// SQL setting the model [`CHANNEL`] starts [`AGENT`] on to `model` as its schema version and
/// selection, or to the agent's default when `None`.
fn channel_model_sql(model: Option<ChannelModel<'_>>, updated_at: &str) -> String {
    let (version, data) = model.map_or_else(
        || ("NULL".to_owned(), "NULL".to_owned()),
        |(version, data)| (version.to_string(), quoted(data)),
    );
    format!(
        "UPDATE chat_agent_channel_memberships
         SET model_selection_schema_version = {version}, model_selection_data = {data},
             updated_at = {updated_at}
         WHERE agent_id = {agent};",
        updated_at = quoted(updated_at),
        agent = quoted(AGENT),
    )
}

/// [`AGENT`]'s conversation membership as `role active|removed`.
async fn membership_state(replica: &mut Replica) -> String {
    replica
        .scalar_text(
            "SELECT membership_role || ' '
                    || CASE WHEN removed_at IS NULL THEN 'active' ELSE 'removed' END
             FROM chat_conversation_memberships
             WHERE participant_id = 'participant:reviewer'",
        )
        .await
        .expect("the membership arrived")
}

/// [`AGENT`]'s AI channel membership as `active` or `removed`.
async fn channel_membership_state(replica: &mut Replica) -> String {
    replica
        .scalar_text(
            "SELECT CASE WHEN removed_at IS NULL THEN 'active' ELSE 'removed' END
             FROM chat_agent_channel_memberships
             WHERE agent_id = 'participant:reviewer'",
        )
        .await
        .expect("the AI channel membership arrived")
}

/// The model [`CHANNEL`] starts [`AGENT`] on as `version data`, or `default`.
async fn channel_model(replica: &mut Replica) -> String {
    replica
        .scalar_text(
            "SELECT coalesce(model_selection_schema_version || ' ' || model_selection_data,
                             'default')
             FROM chat_agent_channel_memberships
             WHERE agent_id = 'participant:reviewer'",
        )
        .await
        .expect("the AI channel membership arrived")
}

#[test]
fn chat_conversation_rows_round_trip_with_their_derived_keys() {
    block_on(async {
        let mut replicas = replicas(2).await;
        replicas[0]
            .exec(&format!("{}{}", identity_sql(), conversation_sql()))
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;

        let reader = &mut replicas[1];
        assert_eq!(
            reader
                .scalar_text(
                    "SELECT conversation.conversation_kind || ' ' || channel.name || ' '
                            || channel.is_default || ' '
                            || coalesce(channel.agent_access, 'inherit') || ' ['
                            || channel.agent_instructions || ']'
                     FROM chat_conversations conversation
                     JOIN chat_channels channel ON channel.conversation_id = conversation.id
                     WHERE conversation.id = 'conversation:design'",
                )
                .await
                .as_deref(),
            Some("channel design 0 inherit []")
        );
        assert_eq!(membership_state(reader).await, "member active");
        assert_eq!(channel_membership_state(reader).await, "active");
        assert_eq!(channel_model(reader).await, "default");
        assert_eq!(
            reader
                .scalar_text(
                    "SELECT sync_key FROM chat_agent_channel_memberships
                     WHERE agent_id = 'participant:reviewer'",
                )
                .await,
            Some(key(CHANNEL_MEMBERSHIP_KEY_PREFIX, &[CONVERSATION, AGENT]))
        );
        assert_eq!(
            reader
                .count(
                    "SELECT count(*) FROM chat_conversation_memberships
                     WHERE conversation_id = 'conversation:design'",
                )
                .await,
            2
        );
    });
}

#[test]
fn chat_concurrent_removal_and_re_add_keep_the_removal() {
    block_on(async {
        let mut replicas = scoped_replicas(2).await;

        // Replica 1 removes the agent and narrows the channel first; replica 0 widens
        // everything later.
        let removed = at(1, 0);
        replicas[1]
            .exec(&format!(
                "{}{}{}",
                membership_sql("member", Some(&removed), &removed),
                channel_membership_sql(Some(&removed), &removed),
                access_sql(Scope::Channel, Some("talk"), &removed),
            ))
            .await;
        let widened = at(2, 0);
        replicas[0]
            .exec(&format!(
                "{}{}",
                membership_sql("owner", None, &widened),
                access_sql(Scope::Channel, Some("own"), &widened),
            ))
            .await;
        replicas[0].now_ms += LATER_MS;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(membership_state(replica).await, "member removed");
            assert_eq!(channel_membership_state(replica).await, "removed");
            assert_eq!(access_level(replica, Scope::Channel).await, "talk");
        }
    });
}

/// Pairs of concurrent access edits, the narrower first, each written over `ask`. Inheriting is
/// only offered where a scope has an enclosing one.
fn access_pairs(scope: Scope) -> Vec<(&'static str, Option<&'static str>)> {
    let mut pairs = vec![("read", Some("own")), ("talk", Some("read"))];
    if !matches!(scope, Scope::App) {
        pairs.push(("read", None));
    }
    pairs
}

/// One device narrows access while another widens it with the later clock, at vault, project,
/// and channel scope at once: the narrower level wins at every scope on every replica, whichever
/// device carries the later clock.
#[test]
fn chat_concurrent_access_edits_keep_the_narrower_level_at_every_scope() {
    block_on(async {
        for pair in 0..3 {
            for later in [0_usize, 1] {
                let mut replicas = scoped_replicas(2).await;
                let narrower = 1 - later;
                let mut expected = Vec::new();
                for scope in Scope::ALL {
                    let Some(&(narrow, wide)) = access_pairs(scope).get(pair) else {
                        continue;
                    };
                    replicas[narrower]
                        .exec(&access_sql(scope, Some(narrow), &at(1, 0)))
                        .await;
                    replicas[later]
                        .exec(&access_sql(scope, wide, &at(2, 0)))
                        .await;
                    expected.push((scope, narrow));
                }
                replicas[later].now_ms += LATER_MS;
                converge(&mut replicas).await;
                assert_converged(&mut replicas).await;
                for replica in &mut replicas {
                    for &(scope, narrow) in &expected {
                        assert_eq!(
                            access_level(replica, scope).await,
                            narrow,
                            "{scope:?} pair {pair}, later edit from replica {later}"
                        );
                    }
                }

                // A widening that saw the narrowing replaces it.
                replicas[narrower]
                    .exec(&access_sql(Scope::Channel, Some("own"), &at(3, 0)))
                    .await;
                converge(&mut replicas).await;
                assert_converged(&mut replicas).await;
                assert_eq!(
                    access_level(&mut replicas[later], Scope::Channel).await,
                    "own"
                );
            }
        }
    });
}

/// Concurrent instruction edits at every scope keep the edit with the later clock, whichever
/// device carries it.
#[test]
fn chat_concurrent_instruction_edits_keep_the_last_writer_at_every_scope() {
    block_on(async {
        for later in [0_usize, 1] {
            let mut replicas = scoped_replicas(2).await;
            for scope in Scope::ALL {
                for (index, replica) in replicas.iter_mut().enumerate() {
                    replica
                        .exec(&instructions_sql(
                            scope,
                            &format!("{scope:?} from {index}"),
                            &at(1, 0),
                        ))
                        .await;
                }
            }
            replicas[later].now_ms += LATER_MS;
            converge(&mut replicas).await;
            assert_converged(&mut replicas).await;
            for replica in &mut replicas {
                for scope in Scope::ALL {
                    assert_eq!(
                        instructions(replica, scope).await,
                        format!("{scope:?} from {later}"),
                        "later edit from replica {later}"
                    );
                }
            }
        }
    });
}

/// A channel's model override travels as one value: concurrent overrides, or an override against
/// a clear, settle on the later edit whole everywhere, never a version from one and a selection
/// from the other.
#[test]
fn chat_concurrent_channel_model_overrides_converge_whole_or_absent() {
    block_on(async {
        let first = r#"{"modelId":"first"}"#;
        let second = r#"{"modelId":"second"}"#;
        let cases: [(Option<ChannelModel>, Option<ChannelModel>); 2] = [
            (Some((1, first)), Some((2, second))),
            (Some((1, first)), None),
        ];
        for (base, other) in cases {
            for later in [0_usize, 1] {
                let mut replicas = conversation_replicas(2).await;
                replicas[0]
                    .exec(&channel_model_sql(
                        Some((3, r#"{"modelId":"base"}"#)),
                        &at(0, 30),
                    ))
                    .await;
                converge(&mut replicas).await;
                replicas[1 - later]
                    .exec(&channel_model_sql(base, &at(1, 0)))
                    .await;
                replicas[later]
                    .exec(&channel_model_sql(other, &at(1, 0)))
                    .await;
                replicas[later].now_ms += LATER_MS;
                converge(&mut replicas).await;
                assert_converged(&mut replicas).await;
                let expected = other.map_or_else(
                    || "default".to_owned(),
                    |(version, data)| format!("{version} {data}"),
                );
                for replica in &mut replicas {
                    assert_eq!(
                        channel_model(replica).await,
                        expected,
                        "later edit from replica {later}"
                    );
                }
            }
        }
    });
}

#[test]
fn chat_a_re_add_after_a_seen_removal_converges_to_active() {
    block_on(async {
        let mut replicas = replicas(2).await;
        replicas[0]
            .exec(&format!("{}{}", identity_sql(), conversation_sql()))
            .await;
        converge(&mut replicas).await;

        let removed = at(1, 0);
        replicas[0]
            .exec(&format!(
                "{}{}",
                membership_sql("member", Some(&removed), &removed),
                channel_membership_sql(Some(&removed), &removed),
            ))
            .await;
        converge(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(channel_membership_state(replica).await, "removed");
        }

        // Replica 1 saw the removal, so its re-add is causally later and replaces it.
        let added = at(2, 0);
        replicas[1]
            .exec(&format!(
                "{}{}",
                membership_sql("member", None, &added),
                channel_membership_sql(None, &added),
            ))
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(membership_state(replica).await, "member active");
            assert_eq!(channel_membership_state(replica).await, "active");
        }
    });
}

/// Deleting a project deletes its agent settings on every replica, also against a concurrent
/// edit of them.
#[test]
fn chat_project_agent_settings_leave_with_their_project() {
    block_on(async {
        let mut replicas = scoped_replicas(2).await;
        for replica in &mut replicas {
            assert_eq!(access_level(replica, Scope::Project).await, "ask");
        }
        replicas[0]
            .exec(&instructions_sql(Scope::Project, "Offline edit", &at(1, 0)))
            .await;
        replicas[1]
            .exec(&format!(
                "DELETE FROM projects WHERE id = {}",
                quoted(PROJECT)
            ))
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(
                replica
                    .count("SELECT count(*) FROM chat_project_agent_settings")
                    .await,
                0,
                "replica {}",
                replica.index
            );
            // The vault settings stay.
            assert_eq!(app_settings(replica).await, "1 ask ");
        }
    });
}

/// Raw replicated values of [`AGENT`]'s membership that [`membership_sql`] writes.
const MEMBERSHIP_VALUE: &str = "SELECT membership_role || '|' || coalesce(removed_at, '')
     FROM chat_conversation_memberships WHERE participant_id = 'participant:reviewer'";

/// Raw replicated presence of [`AGENT`]'s AI channel membership that
/// [`channel_membership_sql`] writes.
const PRESENCE_VALUE: &str = "SELECT coalesce(removed_at, '')
     FROM chat_agent_channel_memberships WHERE agent_id = 'participant:reviewer'";

/// Runs `sql` on `replica` and says whether it changed the value `value` reads. A write that
/// leaves a group's value as it was captures nothing, so it is no concurrent edit.
async fn edits(replica: &mut Replica, value: &str, sql: &str) -> bool {
    let before = replica.scalar_text(value).await;
    replica.exec(sql).await;
    replica.scalar_text(value).await != before
}

/// The edits of one access round: whether a replica removed each membership, and otherwise the
/// strictest value a replica wrote.
#[derive(Default)]
struct AccessRound {
    membership_removed: bool,
    role: Option<usize>,
    channel_removed: bool,
    channel_re_added: bool,
    channel_access: Option<usize>,
    app_access: Option<usize>,
}

#[test]
fn chat_simulated_removal_against_re_add_and_narrowing_against_widening_keep_the_narrower() {
    block_on(async {
        for seed in 0..ACCESS_SEEDS {
            let mut rng = Rng(seed ^ 0x5eed);
            let mut replicas = replicas(3).await;
            replicas[0]
                .exec(&format!("{}{}", identity_sql(), conversation_sql()))
                .await;
            converge(&mut replicas).await;
            let mut expected_role = "member active".to_owned();
            let mut expected_presence = "active";
            let mut expected_channel = "inherit";
            let mut expected_app = "ask";
            let channel_value = access_value(Scope::Channel);
            let app_value = access_value(Scope::App);
            for round in 0..ACCESS_ROUNDS {
                let mut round_edits = AccessRound::default();
                for replica in &mut replicas {
                    let stamp = at(round + 1, rng.below(60));
                    if rng.chance(60) {
                        if rng.chance(40) {
                            let sql = membership_sql("member", Some(&stamp), &stamp);
                            round_edits.membership_removed |=
                                edits(replica, MEMBERSHIP_VALUE, &sql).await;
                        } else {
                            let role = rng.below(ROLES.len());
                            let sql = membership_sql(ROLES[role], None, &stamp);
                            if edits(replica, MEMBERSHIP_VALUE, &sql).await {
                                round_edits.role = round_edits.role.max(Some(role));
                            }
                        }
                    }
                    if rng.chance(60) {
                        let removing = rng.chance(50);
                        let sql =
                            channel_membership_sql(removing.then_some(stamp.as_str()), &stamp);
                        if edits(replica, PRESENCE_VALUE, &sql).await {
                            if removing {
                                round_edits.channel_removed = true;
                            } else {
                                round_edits.channel_re_added = true;
                            }
                        }
                    }
                    if rng.chance(60) {
                        let level = rng.below(ACCESS_LEVELS.len());
                        let sql = access_sql(Scope::Channel, ACCESS_LEVELS[level], &stamp);
                        if edits(replica, &channel_value, &sql).await {
                            round_edits.channel_access =
                                round_edits.channel_access.max(Some(level));
                        }
                    }
                    if rng.chance(40) {
                        // The vault level never inherits.
                        let level = 1 + rng.below(ACCESS_LEVELS.len() - 1);
                        let sql = access_sql(Scope::App, ACCESS_LEVELS[level], &stamp);
                        if edits(replica, &app_value, &sql).await {
                            round_edits.app_access = round_edits.app_access.max(Some(level));
                        }
                    }
                    if rng.chance(50) {
                        replica.seal().await.unwrap();
                    }
                }
                converge(&mut replicas).await;
                assert_converged(&mut replicas).await;

                // Removal outranks every re-add, and the narrower access level outranks every
                // wider one.
                if round_edits.membership_removed {
                    expected_role = "member removed".to_owned();
                } else if let Some(role) = round_edits.role {
                    expected_role = format!("{} active", ROLES[role]);
                }
                if round_edits.channel_removed {
                    expected_presence = "removed";
                } else if round_edits.channel_re_added {
                    expected_presence = "active";
                }
                if let Some(level) = round_edits.channel_access {
                    expected_channel = ACCESS_LEVELS[level].unwrap_or("inherit");
                }
                if let Some(level) = round_edits.app_access {
                    expected_app = ACCESS_LEVELS[level].unwrap_or("inherit");
                }
                for replica in &mut replicas {
                    let context = format!("seed {seed} round {round}");
                    assert_eq!(membership_state(replica).await, expected_role, "{context}");
                    assert_eq!(
                        channel_membership_state(replica).await,
                        expected_presence,
                        "{context}"
                    );
                    assert_eq!(
                        access_level(replica, Scope::Channel).await,
                        expected_channel,
                        "{context}"
                    );
                    assert_eq!(
                        access_level(replica, Scope::App).await,
                        expected_app,
                        "{context}"
                    );
                }
            }
        }
    });
}

/// Records the conversation keys a materialization handed the hook whose rows are gone, as the
/// app's Chat hook reports removed conversations.
#[derive(Default)]
struct RemovedConversations(Arc<Mutex<Vec<String>>>);

impl MaterializeHook for RemovedConversations {
    fn rebuild_all<'a>(
        &'a self,
        _conn: &'a mut SqliteConnection,
        _table: &'static TableSpec,
        _changes: &'a mut dyn ChangeSink,
    ) -> HookFuture<'a> {
        Box::pin(std::future::ready(Ok(())))
    }

    fn after_materialize<'a>(
        &'a self,
        conn: &'a mut SqliteConnection,
        _table: &'static TableSpec,
        changed_keys: &'a [String],
        _changes: &'a mut dyn ChangeSink,
    ) -> HookFuture<'a> {
        Box::pin(async move {
            for key in changed_keys {
                let present: i64 =
                    sqlx::query_scalar("SELECT count(*) FROM chat_conversations WHERE id = ?")
                        .bind(key)
                        .fetch_one(&mut *conn)
                        .await?;
                if present == 0 {
                    self.0.lock().unwrap().push(key.clone());
                }
            }
            Ok(())
        })
    }
}

/// A project deleted on one replica cascades into its conversations on the other, and the
/// receiver's Chat hook is handed those conversation keys, so it can report them removed.
#[test]
fn chat_hook_sees_conversations_an_applied_project_delete_cascades_away() {
    block_on(async {
        let mut replicas = replicas(2).await;
        replicas[0]
            .exec(&format!("{}{}", identity_sql(), conversation_sql()))
            .await;
        converge(&mut replicas).await;

        let removed = RemovedConversations::default();
        let seen = Arc::clone(&removed.0);
        replicas[1].engine = Engine::with(
            &TEST_MANIFEST,
            test_adapters(),
            super::notes::with_notes_hooks(crate::domains::vault_hooks())
                .with(LISTS_TABLE, Box::new(FixtureHook))
                .with(OVERRIDES_TABLE, Box::new(OverrideHook::default()))
                .with(CONVERSATIONS_TABLE, Box::new(removed)),
        )
        .unwrap();
        replicas[0]
            .exec(&format!(
                "DELETE FROM projects WHERE id = {}",
                quoted(PROJECT)
            ))
            .await;
        converge(&mut replicas).await;

        for replica in &mut replicas {
            assert_eq!(
                replica
                    .count(
                        "SELECT count(*) FROM chat_conversations
                         WHERE id = 'conversation:design'"
                    )
                    .await,
                0
            );
        }
        assert_eq!(*seen.lock().unwrap(), vec![CONVERSATION.to_owned()]);
    });
}

/// Root message of the reply thread in the timeline tests.
const ROOT: &str = "item:root";

/// The reply thread under [`ROOT`].
fn thread() -> String {
    key(REPLY_THREAD_KEY_PREFIX, &[ROOT])
}

/// SQL posting message `item` by [`OWNER`] at `order_key` in [`CONVERSATION`], in `thread` when
/// given, with its first revision `{item}:0` holding `markdown` as the current one.
fn message_sql(item: &str, thread: Option<&str>, order_key: &str, markdown: &str) -> String {
    format!(
        "INSERT INTO chat_conversation_items
            (id, conversation_id, reply_thread_id, item_kind, order_key, created_at)
         VALUES ({item}, {conversation}, {thread}, 'message', {order_key}, {created});
         INSERT INTO chat_communication_messages
            (item_id, author_participant_id, created_at, author_label_snapshot)
         VALUES ({item}, {owner}, {created}, 'Owner');
         INSERT INTO chat_communication_message_revisions
            (id, message_item_id, normalized_markdown, created_at)
         VALUES ({revision}, {item}, {markdown}, {created});
         UPDATE chat_communication_messages SET current_revision_id = {revision}
         WHERE item_id = {item};",
        item = quoted(item),
        conversation = quoted(CONVERSATION),
        thread = optional(thread),
        order_key = quoted(order_key),
        owner = quoted(OWNER),
        created = quoted(CREATED),
        revision = quoted(&format!("{item}:0")),
        markdown = quoted(markdown),
    )
}

/// SQL editing `item` to revision `revision` holding `markdown`, created at `created_at`.
fn edit_sql(item: &str, revision: &str, markdown: &str, created_at: &str) -> String {
    format!(
        "INSERT INTO chat_communication_message_revisions
            (id, message_item_id, normalized_markdown, created_at)
         VALUES ({revision}, {item}, {markdown}, {created_at});
         UPDATE chat_communication_messages
         SET current_revision_id = {revision}, edited_at = {created_at}
         WHERE item_id = {item};",
        item = quoted(item),
        revision = quoted(revision),
        markdown = quoted(markdown),
        created_at = quoted(created_at),
    )
}

/// SQL opening the reply thread under [`ROOT`], which must exist.
fn thread_sql() -> String {
    format!(
        "INSERT INTO chat_reply_threads
            (id, conversation_id, root_item_id, last_activity_at, created_at, updated_at)
         VALUES ({thread}, {conversation}, {root}, {created}, {created}, {created});",
        thread = quoted(&thread()),
        conversation = quoted(CONVERSATION),
        root = quoted(ROOT),
        created = quoted(CREATED),
    )
}

/// SQL moving [`OWNER`]'s read cursor of [`CONVERSATION`] to `(order_key, item)` when that is
/// later than where it is, as the app writes it.
fn read_sql(order_key: &str, item: &str, updated_at: &str) -> String {
    format!(
        "INSERT INTO chat_conversation_read_cursors
            (conversation_id, participant_id, sync_key, last_read_order_key, last_read_item_id,
             updated_at)
         VALUES ({conversation}, {owner}, {sync_key}, {order_key}, {item}, {updated_at})
         ON CONFLICT (conversation_id, participant_id) DO UPDATE SET
            last_read_order_key = excluded.last_read_order_key,
            last_read_item_id = excluded.last_read_item_id,
            updated_at = excluded.updated_at
         WHERE (excluded.last_read_order_key, excluded.last_read_item_id)
             > (last_read_order_key, last_read_item_id);",
        conversation = quoted(CONVERSATION),
        owner = quoted(OWNER),
        sync_key = quoted(&key(CONVERSATION_CURSOR_KEY_PREFIX, &[CONVERSATION, OWNER])),
        order_key = quoted(order_key),
        item = quoted(item),
        updated_at = quoted(updated_at),
    )
}

/// The ids of the root timeline of [`CONVERSATION`] in reading order.
async fn root_order(replica: &mut Replica) -> String {
    replica
        .scalar_text(
            "SELECT group_concat(id, ' ') FROM (
                SELECT id FROM chat_conversation_items
                WHERE conversation_id = 'conversation:design' AND reply_thread_id IS NULL
                ORDER BY order_key, id)",
        )
        .await
        .unwrap_or_default()
}

/// [`OWNER`]'s read position in [`CONVERSATION`] as `order_key item`.
async fn read_position(replica: &mut Replica) -> String {
    replica
        .scalar_text(
            "SELECT last_read_order_key || ' ' || last_read_item_id
             FROM chat_conversation_read_cursors
             WHERE participant_id = 'participant:local-owner'",
        )
        .await
        .expect("the cursor arrived")
}

/// Captured timeline rows as `table_id row_key`.
async fn timeline_captures(replica: &mut Replica) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT table_id || ' ' || row_key FROM sync_capture
         WHERE table_id BETWEEN ? AND ? ORDER BY table_id, row_key",
    )
    .bind(i64::from(REPLY_THREADS_TABLE.0))
    .bind(i64::from(REPLY_CURSORS_TABLE.0))
    .fetch_all(&mut replica.conn)
    .await
    .unwrap()
}

/// Replicas sharing the identity and conversation rows.
async fn conversation_replicas(count: usize) -> Vec<Replica> {
    let mut replicas = replicas(count).await;
    replicas[0]
        .exec(&format!("{}{}", identity_sql(), conversation_sql()))
        .await;
    converge(&mut replicas).await;
    replicas
}

#[test]
fn chat_timeline_rows_round_trip_with_their_derived_keys() {
    block_on(async {
        let mut replicas = conversation_replicas(2).await;
        replicas[0]
            .exec(&format!(
                "{}{}{}{}",
                message_sql(ROOT, None, "c001", "Root"),
                thread_sql(),
                message_sql("item:reply", Some(&thread()), "c001", "Reply"),
                edit_sql(ROOT, "item:root:1", "Root edited", &at(1, 0)),
            ))
            .await;
        replicas[0]
            .exec(&format!(
                "{}INSERT INTO chat_reply_thread_read_cursors
                    (reply_thread_id, participant_id, sync_key, last_read_order_key,
                     last_read_item_id, updated_at)
                 VALUES ({thread}, {owner}, {sync_key}, 'c001', 'item:reply', {created});",
                read_sql("c001", ROOT, CREATED),
                thread = quoted(&thread()),
                owner = quoted(OWNER),
                sync_key = quoted(&key(REPLY_CURSOR_KEY_PREFIX, &[&thread(), OWNER])),
                created = quoted(CREATED),
            ))
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;

        let reader = &mut replicas[1];
        assert_eq!(
            reader
                .scalar_text(
                    "SELECT item.order_key || ' ' || revision.normalized_markdown || ' '
                            || revision.revision || ' ' || (message.edited_at IS NOT NULL)
                     FROM chat_conversation_items item
                     JOIN chat_communication_messages message ON message.item_id = item.id
                     JOIN chat_communication_message_revisions_numbered revision
                       ON revision.id = message.current_revision_id
                     WHERE item.id = 'item:root'",
                )
                .await
                .as_deref(),
            Some("c001 Root edited 2 1")
        );
        assert_eq!(
            reader
                .scalar_text(
                    "SELECT thread.root_item_id || ' ' || item.id
                     FROM chat_reply_threads thread
                     JOIN chat_conversation_items item ON item.reply_thread_id = thread.id",
                )
                .await
                .as_deref(),
            Some("item:root item:reply")
        );
        assert_eq!(read_position(reader).await, "c001 item:root");
        assert_eq!(
            reader
                .scalar_text(
                    "SELECT sync_key || ' ' || last_read_order_key || ' ' || last_read_item_id
                     FROM chat_reply_thread_read_cursors",
                )
                .await,
            Some(format!(
                "{} c001 item:reply",
                key(REPLY_CURSOR_KEY_PREFIX, &[&thread(), OWNER])
            ))
        );
    });
}

#[test]
fn chat_a_crash_while_sealing_or_applying_timeline_rows_leaves_nothing_partial() {
    block_on(async {
        let mut replicas = conversation_replicas(2).await;
        replicas[0]
            .exec(&format!(
                "{}{}{}",
                message_sql(ROOT, None, "c001", "Root"),
                thread_sql(),
                message_sql("item:reply", Some(&thread()), "c001", "Reply"),
            ))
            .await;
        // Two items, two messages, two revisions, and the thread.
        assert_eq!(timeline_captures(&mut replicas[0]).await.len(), 7);
        for name in ["seal.reserve", "seal.before_commit"] {
            let report = after_rollback(&mut replicas[0], name, async |r: &mut Replica| {
                r.seal().await
            })
            .await;
            assert_eq!(report.sealed_ops, 1, "{name}: the rows seal together");
            assert!(timeline_captures(&mut replicas[0]).await.is_empty());
            if name == "seal.reserve" {
                replicas[0]
                    .exec(&edit_sql(ROOT, "item:root:1", "Root edited", &at(1, 0)))
                    .await;
            }
        }

        let (author, rest) = replicas.split_at_mut(1);
        let reader = &mut rest[0];
        let known = reader.stored_vector().await;
        let ops = author[0].ops_since(&known).await;
        reader.receive(&ops).await;
        assert_eq!(
            reader
                .count("SELECT count(*) FROM chat_conversation_items")
                .await,
            0
        );
        after_rollback(reader, "apply.before_commit", async |r: &mut Replica| {
            r.apply_all().await
        })
        .await;
        assert_eq!(root_order(reader).await, "item:root");
        assert_converged(&mut replicas).await;
    });
}

#[test]
fn chat_concurrent_posts_with_one_order_key_read_in_one_order_everywhere() {
    block_on(async {
        let mut replicas = conversation_replicas(3).await;
        replicas[0]
            .exec(&message_sql("item:first", None, "c001", "First"))
            .await;
        converge(&mut replicas).await;
        // Each replica appends after the last key it sees, so all three take the same key.
        for (index, item) in ["item:c", "item:a", "item:b"].into_iter().enumerate() {
            replicas[index]
                .exec(&message_sql(item, None, "c002", item))
                .await;
        }
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(root_order(replica).await, "item:first item:a item:b item:c");
        }
    });
}

#[test]
fn chat_read_cursors_only_move_forward_and_unchanged_reads_capture_nothing() {
    block_on(async {
        let mut replicas = conversation_replicas(2).await;
        replicas[0]
            .exec(&format!(
                "{}{}{}",
                message_sql("item:a", None, "c001", "A"),
                message_sql("item:b", None, "c001", "B"),
                read_sql("", "", CREATED),
            ))
            .await;
        converge(&mut replicas).await;

        // A later read on one replica and an earlier one on the other keep the later.
        replicas[0]
            .exec(&read_sql("c001", "item:b", &at(1, 0)))
            .await;
        replicas[1]
            .exec(
                "UPDATE chat_conversation_read_cursors
                 SET last_read_order_key = 'c001', last_read_item_id = 'item:a',
                     updated_at = '2026-10-24T09:02:00.000Z'",
            )
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(read_position(replica).await, "c001 item:b");
        }

        // Reading the same or an earlier item again rewrites nothing.
        let replica = &mut replicas[0];
        replica.exec(&read_sql("c001", "item:b", &at(3, 0))).await;
        replica.exec(&read_sql("c001", "item:a", &at(4, 0))).await;
        assert!(timeline_captures(replica).await.is_empty());
        assert_eq!(
            replica
                .scalar_text("SELECT updated_at FROM chat_conversation_read_cursors")
                .await,
            Some(at(2, 0))
        );
    });
}

/// Records the tables and keys an applied materialization handed the hook.
#[derive(Clone, Default)]
struct RecordingHook(Arc<Mutex<Vec<String>>>);

impl MaterializeHook for RecordingHook {
    fn rebuild_all<'a>(
        &'a self,
        _conn: &'a mut SqliteConnection,
        _table: &'static TableSpec,
        _changes: &'a mut dyn ChangeSink,
    ) -> HookFuture<'a> {
        Box::pin(std::future::ready(Ok(())))
    }

    fn after_materialize<'a>(
        &'a self,
        _conn: &'a mut SqliteConnection,
        table: &'static TableSpec,
        changed_keys: &'a [String],
        _changes: &'a mut dyn ChangeSink,
    ) -> HookFuture<'a> {
        Box::pin(async move {
            let mut seen = self.0.lock().unwrap();
            seen.extend(
                changed_keys
                    .iter()
                    .map(|key| format!("{} {key}", table.name)),
            );
            Ok(())
        })
    }
}

/// Search rows follow local writes through triggers, which stay quiet while applying, so the
/// receiver's Chat hook is handed the applied messages and revisions to rebuild them.
#[test]
fn chat_hook_sees_applied_messages_and_revisions_for_search() {
    block_on(async {
        let mut replicas = conversation_replicas(2).await;
        let hook = RecordingHook::default();
        let seen = Arc::clone(&hook.0);
        replicas[1].engine = Engine::with(
            &TEST_MANIFEST,
            test_adapters(),
            super::notes::with_notes_hooks(crate::domains::vault_hooks())
                .with(LISTS_TABLE, Box::new(FixtureHook))
                .with(OVERRIDES_TABLE, Box::new(OverrideHook::default()))
                .with(MESSAGES_TABLE, Box::new(hook.clone()))
                .with(REVISIONS_TABLE, Box::new(hook)),
        )
        .unwrap();
        replicas[0]
            .exec(&message_sql(ROOT, None, "c001", "Searchable"))
            .await;
        assert_eq!(
            replicas[0]
                .count("SELECT count(*) FROM chat_communication_search_fts")
                .await,
            1
        );
        converge(&mut replicas).await;

        let mut keys = seen.lock().unwrap().clone();
        keys.sort();
        assert_eq!(
            keys,
            vec![
                "chat_communication_message_revisions item:root:0".to_owned(),
                "chat_communication_messages item:root".to_owned(),
            ]
        );
        assert_eq!(
            replicas[1]
                .count("SELECT count(*) FROM chat_communication_search_fts")
                .await,
            0
        );
    });
}

/// A post written offline in a project another device deleted cannot come back with the
/// project, so the recovery offer of the project carries its text for the person to copy, while
/// a message the deletion saw stays out of it.
#[test]
fn chat_an_offline_post_in_a_deleted_project_is_readable_in_its_recovery_offer() {
    block_on(async {
        let mut replicas = conversation_replicas(2).await;
        replicas[0]
            .exec(&message_sql("item:seen", None, "c001", "Seen before"))
            .await;
        converge(&mut replicas).await;
        replicas[0]
            .exec(&format!(
                "DELETE FROM projects WHERE id = {}",
                quoted(PROJECT)
            ))
            .await;
        replicas[1]
            .exec(&message_sql(
                "item:offline",
                None,
                "c002",
                "Written offline",
            ))
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            let entries = replica
                .engine
                .recovery_entries(&mut replica.conn, &replica.ctx)
                .await
                .unwrap();
            let entry = entries
                .iter()
                .find(|entry| (entry.table, entry.row_key.as_str()) == (PROJECTS_TABLE, PROJECT))
                .unwrap_or_else(|| panic!("no project offer in {entries:?}"));
            let previews: Vec<&str> = entry
                .unseen
                .iter()
                .map(|presentation| presentation.preview.as_str())
                .collect();
            assert_eq!(previews, vec!["Written offline"], "{entry:?}");
        }
    });
}

/// Unseen previews and omitted count of the recovery offer of [`PROJECT`] on `replica`.
async fn project_offer_unseen(replica: &mut Replica) -> (Vec<String>, usize) {
    let entries = replica
        .engine
        .recovery_entries(&mut replica.conn, &replica.ctx)
        .await
        .unwrap();
    let entry = entries
        .into_iter()
        .find(|entry| (entry.table, entry.row_key.as_str()) == (PROJECTS_TABLE, PROJECT))
        .expect("an offer for the deleted project");
    let previews = entry
        .unseen
        .into_iter()
        .map(|presentation| presentation.preview)
        .collect();
    (previews, entry.unseen_omitted)
}

/// Deletes [`PROJECT`] on the first replica while the second runs `offline`, then converges.
/// The first replica receives the offline writes after its deletion and the second the deletion
/// after its writes, so checking both covers both arrival orders.
async fn delete_project_against(replicas: &mut [Replica], offline: &str) {
    replicas[0]
        .exec(&format!(
            "DELETE FROM projects WHERE id = {}",
            quoted(PROJECT)
        ))
        .await;
    replicas[1].exec(offline).await;
    converge(replicas).await;
    assert_converged(replicas).await;
}

/// An edit from another device leaves the message's creator alone and names the editing device
/// as the creator of the new revision, on every replica.
#[test]
fn chat_an_edit_names_its_device_as_the_revision_creator() {
    block_on(async {
        let mut replicas = conversation_replicas(2).await;
        replicas[0]
            .exec(&message_sql(ROOT, None, "c001", "Root"))
            .await;
        converge(&mut replicas).await;
        replicas[1]
            .exec(&edit_sql(ROOT, "item:root:1", "Root edited", &at(1, 0)))
            .await;
        converge(&mut replicas).await;

        for replica in &mut replicas {
            let manifest = replica.engine.manifest();
            assert_eq!(
                row_creating_devices(&mut replica.conn, manifest, MESSAGES_TABLE, ROOT)
                    .await
                    .unwrap(),
                vec![device_id(0)]
            );
            assert_eq!(
                row_creating_devices(&mut replica.conn, manifest, REVISIONS_TABLE, "item:root:1")
                    .await
                    .unwrap(),
                vec![device_id(1)]
            );
        }
    });
}

/// A post written and then edited offline shows only its newest text in the offer, in both
/// arrival orders.
#[test]
fn chat_a_recovery_offer_shows_only_the_newest_revision_of_an_offline_post() {
    block_on(async {
        let mut replicas = conversation_replicas(2).await;
        let offline = format!(
            "{}{}{}",
            message_sql("item:offline", None, "c001", "First draft"),
            edit_sql(
                "item:offline",
                "item:offline:1",
                "Second draft",
                "2026-10-24T10:52:00.000Z"
            ),
            edit_sql(
                "item:offline",
                "item:offline:2",
                "Final text",
                "2026-10-24T10:53:00.000Z"
            ),
        );
        delete_project_against(&mut replicas, &offline).await;
        for replica in &mut replicas {
            assert_eq!(
                project_offer_unseen(replica).await,
                (vec!["Final text".to_string()], 0)
            );
        }
    });
}

/// A post written and then deleted offline stays out of the offer, while another offline post
/// stays in it, in both arrival orders.
#[test]
fn chat_a_recovery_offer_leaves_out_a_post_deleted_offline() {
    block_on(async {
        let mut replicas = conversation_replicas(2).await;
        let offline = format!(
            "{}{}UPDATE chat_communication_messages SET deleted_at = '2026-10-24T10:55:00.000Z'
             WHERE item_id = 'item:removed';",
            message_sql("item:kept", None, "c001", "Kept post"),
            message_sql("item:removed", None, "c002", "Removed post"),
        );
        delete_project_against(&mut replicas, &offline).await;
        for replica in &mut replicas {
            assert_eq!(
                project_offer_unseen(replica).await,
                (vec!["Kept post".to_string()], 0)
            );
        }
    });
}

/// An offer lists at most [`MAX_UNSEEN_PRESENTATIONS`] posts and counts the rest, so none is
/// dropped without a trace.
#[test]
fn chat_a_recovery_offer_counts_the_unseen_posts_past_its_list() {
    block_on(async {
        let mut replicas = conversation_replicas(2).await;
        let extra = 3;
        let offline: String = (0..MAX_UNSEEN_PRESENTATIONS + extra)
            .map(|index| {
                message_sql(
                    &format!("item:{index:02}"),
                    None,
                    &format!("c{index:03}"),
                    &format!("Post {index}"),
                )
            })
            .collect();
        delete_project_against(&mut replicas, &offline).await;
        for replica in &mut replicas {
            let (previews, omitted) = project_offer_unseen(replica).await;
            assert_eq!(previews.len(), MAX_UNSEEN_PRESENTATIONS);
            assert_eq!(omitted, extra);
        }
    });
}

const IMAGE_DIGEST: &str = "5f2b0c4e8d6a1b3f7e9c2d4a6b8f0e1c3d5a7b9f1e2c4d6a8b0f2e4c6d8a0b1c";
const EMPTY_DIGEST: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

/// SQL attaching an image and an empty text snippet by value to the first revision of
/// [`ROOT`].
fn attachments_sql() -> String {
    format!(
        "INSERT INTO chat_communication_attachment_references
            (message_revision_id, ordinal, attachment_id, working_folder_id, kind,
             original_display_name, mime_type, byte_size, sha256)
         VALUES
            ('item:root:0', 0, 'attachment:image', {folder}, 'image', 'diagram.png',
             'image/png', 4096, {image}),
            ('item:root:0', 1, 'attachment:empty', {folder}, 'text_snippet', 'empty.txt',
             'text/plain', 0, {empty});",
        folder = quoted(FOLDER),
        image = quoted(IMAGE_DIGEST),
        empty = quoted(EMPTY_DIGEST),
    )
}

/// Every attachment reference a replica holds, as `ordinal id kind name type size digest`.
async fn attachment_references(replica: &mut Replica) -> Option<String> {
    replica
        .scalar_text(
            "SELECT group_concat(value, ' | ') FROM (
                SELECT ordinal || ' ' || attachment_id || ' ' || working_folder_id || ' ' || kind
                       || ' ' || original_display_name || ' ' || mime_type || ' ' || byte_size
                       || ' ' || sha256 AS value
                FROM chat_communication_attachment_references ORDER BY message_revision_id, ordinal)",
        )
        .await
}

/// Attachments travel by value with the revision that names them, so the receiver can show
/// them without the sender's attachment rows, and only the non-empty one names a file.
#[test]
fn chat_attachment_references_arrive_by_value_and_name_their_files() {
    block_on(async {
        let mut replicas = conversation_replicas(2).await;
        replicas[0]
            .exec(&format!(
                "{}{}",
                message_sql(ROOT, None, "c001", "See attached"),
                attachments_sql()
            ))
            .await;
        let sent = attachment_references(&mut replicas[0]).await;
        assert!(sent.is_some());
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;

        let reader = &mut replicas[1];
        assert_eq!(attachment_references(reader).await, sent);
        let assets = reader
            .engine
            .referenced_assets(&mut reader.conn)
            .await
            .unwrap();
        let assets: Vec<(&str, &str, &str, u32)> = assets
            .iter()
            .map(|asset| {
                (
                    asset.class().name,
                    asset.sha256(),
                    asset.content_type(),
                    asset.byte_size(),
                )
            })
            .collect();
        assert_eq!(
            assets,
            vec![("chat/attachments", IMAGE_DIGEST, "image/png", 4096)]
        );
    });
}

#[test]
fn local_user_participant_under_another_key_is_invalid() {
    block_on(async {
        let mut replicas = replicas(2).await;
        let mut create = replicas[0]
            .own_ops()
            .await
            .iter()
            .filter_map(|bytes| {
                match Envelope::decode(bytes)
                    .unwrap()
                    .operation()
                    .unwrap()
                    .content
                {
                    Content::Changes(changes) => Some(changes),
                    _ => None,
                }
            })
            .flatten()
            .find(|change| {
                change.table == PARTICIPANTS_TABLE
                    && change.row.as_str() == LOCAL_OWNER_PARTICIPANT_KEY
            })
            .expect("the seeded owner is sealed");
        assert_eq!(create.action, ChangeAction::Create);
        assert_eq!(
            crate::validate::change(&replicas[0].engine, &create),
            Ok(())
        );

        // A writer whose build does not enforce the seeded identity.
        create.row = RowKey::new("participant:other").unwrap();
        let revision = view(&mut replicas[1]).await.revision;
        let forged = forge(&mut replicas[1], Content::Changes(vec![create]), revision).await;

        assert_eq!(
            replicas[0].receive(&[forged]).await,
            vec![StoreOutcome::Stored {
                held: Some(HoldReason::Invalid)
            }]
        );
        replicas[0].apply_all().await.unwrap();
        assert_eq!(
            replicas[0]
                .count("SELECT count(*) FROM chat_participants WHERE id = 'participant:other'")
                .await,
            0
        );
    });
}

#[test]
fn chat_a_crash_while_sealing_or_applying_attachment_references_leaves_nothing_partial() {
    block_on(async {
        let mut replicas = conversation_replicas(2).await;
        replicas[0]
            .exec(&format!(
                "{}{}",
                message_sql(ROOT, None, "c001", "See attached"),
                attachments_sql()
            ))
            .await;
        let report = after_rollback(
            &mut replicas[0],
            "seal.before_commit",
            async |r: &mut Replica| r.seal().await,
        )
        .await;
        assert_eq!(report.sealed_ops, 1, "the rows seal together");

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
                .count("SELECT count(*) FROM chat_communication_attachment_references")
                .await,
            2
        );
        assert_converged(&mut replicas).await;
    });
}

mod agent_usage;
mod assignments;
mod channel_notes;
mod push_approvals;
