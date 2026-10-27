//! Chat identity and policy across replicas: participants, teammates, and access profiles
//! arrive with their revisions, a crash while sealing or applying them leaves nothing partial,
//! and concurrent edits of a teammate's current policy or a profile's current revision keep the
//! more restrictive one whatever order the replicas exchange in. Conversations, channels,
//! memberships, AI channel access, and working folder grants follow them: a concurrent removal or
//! revocation wins over a concurrent widening, and a later re-add that saw the removal wins over
//! it. The timeline follows: messages, revisions, reply threads, and read cursors arrive whole,
//! concurrent posts that take one order key read in one order everywhere, read cursors only move
//! forward, and the Chat hook is handed applied messages and revisions to refresh search. Device
//! local Chat rows stay on their device.

use std::sync::{Arc, Mutex};

use ganbaru_sync_contracts::derived_key;
use sqlx::SqliteConnection;

use super::block_on;
use super::fixture::{
    FixtureHook, LISTS_TABLE, OVERRIDES_TABLE, OverrideHook, TEST_MANIFEST, test_adapters,
};
use super::replica::{Replica, converge, quoted};
use super::sim::Rng;
use crate::error::failpoints;
use crate::hooks::{ChangeSink, HookFuture, MaterializeHook};
use crate::manifest::TableSpec;
use crate::manifest::vault::chat::{
    CHANNEL_MEMBERSHIP_KEY_PREFIX, CHANNELS_TABLE, CONVERSATION_CURSOR_KEY_PREFIX,
    CONVERSATIONS_TABLE, FOLDER_GRANT_KEY_PREFIX, MEMBERSHIP_KEY_PREFIX, MESSAGES_TABLE,
    PARTICIPANTS_TABLE, REPLY_CURSOR_KEY_PREFIX, REPLY_CURSORS_TABLE, REPLY_THREAD_KEY_PREFIX,
    REPLY_THREADS_TABLE, REVISIONS_TABLE,
};
use crate::{Engine, SyncError, SyncResult};

const SALT: u64 = 73;
const TEAMMATE: &str = "participant:reviewer";
const PROFILE: &str = "access-profile:reviewers";
const CREATED: &str = "2026-10-24T09:00:00.000Z";

/// Seeds of the concurrent policy edit scenario.
const POLICY_SEEDS: u64 = 8;

/// Edit rounds of one policy scenario.
const POLICY_ROUNDS: usize = 4;

/// Teammate safety modes, least restrictive first, as the adapter ranks them. Custom defers to the
/// provider's own configuration, which can grant anything, so it ranks lowest.
const SAFETY_MODES: [&str; 4] = [
    "custom",
    "full_access",
    "approve_for_me",
    "ask_for_approval",
];

/// Folder capabilities, least restrictive first, as the adapter ranks them.
const CAPABILITIES: [&str; 5] = ["publish", "execute", "edit", "read", "none"];

/// Replicas that each seeded their existing rows at vault open, as the app does, with operations
/// exchanged, so the seeded local owner and routine projects are published everywhere.
async fn replicas(count: usize) -> Vec<Replica> {
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

/// SQL creating [`TEAMMATE`] with an avatar and a home device.
fn teammate_sql() -> String {
    format!(
        "INSERT INTO chat_participants
            (id, participant_kind, display_name, avatar_data, created_at, updated_at)
         VALUES ({teammate}, 'ai_teammate', 'Reviewer', {avatar}, {created}, {created});
         INSERT INTO chat_teammates
            (participant_id, role, instructions, home_device_id, created_at, updated_at)
         VALUES ({teammate}, 'Reviews changes', 'Be brief', 'device-0', {created}, {created});",
        teammate = quoted(TEAMMATE),
        avatar = quoted(r#"{"kind":"emoji","emoji":"R"}"#),
        created = quoted(CREATED),
    )
}

/// SQL adding policy revision `id` of [`TEAMMATE`] with `safety` and making it current.
fn policy_sql(id: &str, safety: &str, created_at: &str) -> String {
    format!(
        "INSERT INTO chat_teammate_policy_revisions
            (id, teammate_id, provider_instance_id, model_selection_data, safety_mode, created_at)
         VALUES ({id}, {teammate}, 'codex-personal', '{{}}', {safety}, {created_at});
         UPDATE chat_teammates
         SET current_policy_revision_id = {id}, current_policy_safety_mode = {safety},
             updated_at = {created_at}
         WHERE participant_id = {teammate};",
        id = quoted(id),
        teammate = quoted(TEAMMATE),
        safety = quoted(safety),
        created_at = quoted(created_at),
    )
}

/// SQL creating [`PROFILE`] without a revision.
fn profile_sql() -> String {
    format!(
        "INSERT INTO chat_access_profiles (id, display_name, created_at, updated_at)
         VALUES ({profile}, 'Reviewers', {created}, {created});",
        profile = quoted(PROFILE),
        created = quoted(CREATED),
    )
}

/// SQL adding revision `id` of [`PROFILE`] with `capability` and making it current.
fn profile_revision_sql(id: &str, capability: &str, created_at: &str) -> String {
    format!(
        "INSERT INTO chat_access_profile_revisions
            (id, access_profile_id, default_read_history, default_participate,
             default_history_boundary, maximum_folder_capability, created_at)
         VALUES ({id}, {profile}, 1, 1, 'entire', {capability}, {created_at});
         UPDATE chat_access_profiles
         SET current_revision_id = {id}, current_maximum_folder_capability = {capability},
             revision = revision + 1, updated_at = {created_at}
         WHERE id = {profile};",
        id = quoted(id),
        profile = quoted(PROFILE),
        capability = quoted(capability),
        created_at = quoted(created_at),
    )
}

/// The identity rows every scenario starts from: the teammate with its first policy and the
/// profile with its first revision.
fn identity_sql() -> String {
    format!(
        "{}{}{}{}",
        teammate_sql(),
        policy_sql("policy-0", "approve_for_me", CREATED),
        profile_sql(),
        profile_revision_sql("profile-revision-0", "edit", CREATED),
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

/// The current policy safety mode of [`TEAMMATE`] and the safety mode of the revision it names.
async fn current_policy(replica: &mut Replica) -> (String, String) {
    let copied = replica
        .scalar_text(
            "SELECT current_policy_safety_mode FROM chat_teammates
             WHERE participant_id = 'participant:reviewer'",
        )
        .await
        .expect("the teammate names a policy");
    let named = replica
        .scalar_text(
            "SELECT policy.safety_mode FROM chat_teammates teammate
             JOIN chat_teammate_policy_revisions policy
               ON policy.id = teammate.current_policy_revision_id
             WHERE teammate.participant_id = 'participant:reviewer'",
        )
        .await
        .expect("the named policy arrived");
    (copied, named)
}

/// The current folder capability of [`PROFILE`] and the capability of the revision it names.
async fn current_capability(replica: &mut Replica) -> (String, String) {
    let copied = replica
        .scalar_text(
            "SELECT current_maximum_folder_capability FROM chat_access_profiles
             WHERE id = 'access-profile:reviewers'",
        )
        .await
        .expect("the profile names a revision");
    let named = replica
        .scalar_text(
            "SELECT revision.maximum_folder_capability FROM chat_access_profiles profile
             JOIN chat_access_profile_revisions revision
               ON revision.id = profile.current_revision_id
             WHERE profile.id = 'access-profile:reviewers'",
        )
        .await
        .expect("the named revision arrived");
    (copied, named)
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
            .exec(&policy_sql("policy-1", "full_access", &at(1, 0)))
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;

        let reader = &mut replicas[1];
        assert_eq!(
            reader
                .scalar_text(
                    "SELECT display_name || ' ' || avatar_data || ' ' || role || ' '
                            || instructions || ' ' || home_device_id
                     FROM chat_participants participant
                     JOIN chat_teammates teammate ON teammate.participant_id = participant.id
                     WHERE participant.id = 'participant:reviewer'",
                )
                .await
                .as_deref(),
            Some(r#"Reviewer {"kind":"emoji","emoji":"R"} Reviews changes Be brief device-0"#)
        );
        assert_eq!(
            current_policy(reader).await,
            ("full_access".to_owned(), "full_access".to_owned())
        );
        assert_eq!(
            current_capability(reader).await,
            ("edit".to_owned(), "edit".to_owned())
        );
        // Display numbers follow `(created_at, id)` order on every replica.
        assert_eq!(
            reader
                .scalar_text(
                    "SELECT group_concat(id || '=' || revision, ' ')
                     FROM (SELECT id, revision FROM chat_teammate_policy_revisions_numbered
                           ORDER BY revision)",
                )
                .await
                .as_deref(),
            Some("policy-0=1 policy-1=2")
        );
        assert_eq!(
            reader
                .scalar_text("SELECT id || '=' || revision FROM chat_teammate_effective_policies",)
                .await
                .as_deref(),
            Some("policy-1=2")
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
        // Five identity rows, then the conversation, two memberships, the AI channel access,
        // the grant, and the channel.
        assert_eq!(captured.len(), 11, "{captured:?}");
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
                    .exec(&policy_sql("policy-1", "custom", &at(1, 0)))
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
                .count("SELECT count(*) FROM chat_teammates WHERE participant_id = 'participant:reviewer'")
                .await,
            0
        );
        after_rollback(reader, "apply.before_commit", async |r: &mut Replica| {
            r.apply_all().await
        })
        .await;
        assert_eq!(
            current_policy(reader).await,
            ("custom".to_owned(), "custom".to_owned())
        );
        assert_eq!(grant_state(reader).await, "edit inherit active");
        assert_converged(&mut replicas).await;
    });
}

#[test]
fn chat_concurrent_current_revisions_keep_the_more_restrictive_one() {
    block_on(async {
        let mut replicas = replicas(2).await;
        replicas[0].exec(&identity_sql()).await;
        converge(&mut replicas).await;

        // Replica 0 relaxes both while replica 1 tightens both; the later edit is the relaxing
        // one, so restrictiveness, not time, decides.
        replicas[1]
            .exec(&format!(
                "{}{}",
                policy_sql("policy-strict", "ask_for_approval", &at(1, 0)),
                profile_revision_sql("profile-revision-strict", "read", &at(1, 0)),
            ))
            .await;
        replicas[0]
            .exec(&format!(
                "{}{}",
                policy_sql("policy-open", "full_access", &at(2, 0)),
                profile_revision_sql("profile-revision-open", "publish", &at(2, 0)),
            ))
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(
                current_policy(replica).await,
                ("ask_for_approval".to_owned(), "ask_for_approval".to_owned())
            );
            assert_eq!(
                current_capability(replica).await,
                ("read".to_owned(), "read".to_owned())
            );
            // Both revisions arrived and keep their display numbers.
            assert_eq!(
                replica
                    .count("SELECT count(*) FROM chat_teammate_policy_revisions")
                    .await,
                3
            );
        }

        // A later edit that is not concurrent replaces the stricter pointer.
        replicas[0]
            .exec(&policy_sql("policy-relaxed", "approve_for_me", &at(3, 0)))
            .await;
        converge(&mut replicas).await;
        assert_eq!(
            current_policy(&mut replicas[1]).await,
            ("approve_for_me".to_owned(), "approve_for_me".to_owned())
        );
    });
}

#[test]
fn chat_concurrent_policy_edits_of_different_safety_converge_to_the_most_restrictive() {
    block_on(async {
        for seed in 0..POLICY_SEEDS {
            let mut rng = Rng(seed);
            let mut replicas = replicas(3).await;
            replicas[0].exec(&identity_sql()).await;
            converge(&mut replicas).await;
            for round in 0..POLICY_ROUNDS {
                let mut strictest_safety = None;
                let mut strictest_capability = None;
                for (index, replica) in replicas.iter_mut().enumerate() {
                    if !rng.chance(70) {
                        continue;
                    }
                    let created_at = at(round + 1, rng.below(60));
                    let id = format!("{round}-{index}");
                    if rng.chance(60) {
                        let rank = rng.below(SAFETY_MODES.len());
                        replica
                            .exec(&policy_sql(
                                &format!("policy-{id}"),
                                SAFETY_MODES[rank],
                                &created_at,
                            ))
                            .await;
                        strictest_safety = strictest_safety.max(Some(rank));
                    } else {
                        let rank = rng.below(CAPABILITIES.len());
                        replica
                            .exec(&profile_revision_sql(
                                &format!("profile-revision-{id}"),
                                CAPABILITIES[rank],
                                &created_at,
                            ))
                            .await;
                        strictest_capability = strictest_capability.max(Some(rank));
                    }
                    if rng.chance(50) {
                        replica.seal().await.unwrap();
                    }
                }
                converge(&mut replicas).await;
                assert_converged(&mut replicas).await;
                for replica in &mut replicas {
                    let (copied, named) = current_policy(replica).await;
                    assert_eq!(copied, named, "seed {seed} round {round}");
                    if let Some(rank) = strictest_safety {
                        assert_eq!(copied, SAFETY_MODES[rank], "seed {seed} round {round}");
                    }
                    let (copied, named) = current_capability(replica).await;
                    assert_eq!(copied, named, "seed {seed} round {round}");
                    if let Some(rank) = strictest_capability {
                        assert_eq!(copied, CAPABILITIES[rank], "seed {seed} round {round}");
                    }
                }
            }
        }
    });
}

#[test]
fn chat_device_local_rows_stay_on_their_device() {
    block_on(async {
        let mut replicas = replicas(2).await;
        replicas[0].exec(&identity_sql()).await;
        let before = replicas[1]
            .count("SELECT count(*) FROM chat_teammate_access_state")
            .await;
        converge(&mut replicas).await;
        // The command trigger created access state on the author only; the applier stands it
        // down, so the reader's state comes from the Chat hook the app registers.
        assert_eq!(
            replicas[0]
                .count("SELECT count(*) FROM chat_teammate_access_state")
                .await,
            1
        );
        assert_eq!(
            replicas[1]
                .count("SELECT count(*) FROM chat_teammate_access_state")
                .await,
            before
        );
    });
}

const OWNER: &str = "participant:local-owner";
const PROJECT: &str = "project-routine-learning";
const FOLDER: &str = "working-folder-routine-learning";
const CONVERSATION: &str = "conversation:design";
const CHANNEL: &str = "channel:design";

/// Seeds of the revoke against widen scenario.
const ACCESS_SEEDS: u64 = 8;

/// Edit rounds of one revoke against widen scenario.
const ACCESS_ROUNDS: usize = 4;

/// Membership roles, least restrictive first, as the adapter ranks them.
const ROLES: [&str; 2] = ["owner", "member"];

/// Runtime approval overrides, least restrictive first, as the adapter ranks them; `None`
/// inherits the teammate's policy, which an override can only narrow.
const APPROVALS: [Option<&str>; 5] = [
    None,
    Some("provider_custom"),
    Some("unattended"),
    Some("auto_approve"),
    Some("ask"),
];

/// The derived key `prefix` names over `parts`.
fn key(prefix: &str, parts: &[&str]) -> String {
    derived_key(prefix, parts).unwrap().into_string()
}

/// `text` quoted, or `NULL`.
fn optional(text: Option<&str>) -> String {
    text.map_or_else(|| "NULL".to_owned(), quoted)
}

/// SQL creating [`CONVERSATION`] with its channel, the owner and [`TEAMMATE`] as members,
/// [`TEAMMATE`]'s AI channel access through [`PROFILE`], and its default `edit` grant on
/// [`FOLDER`]. It needs the rows of [`identity_sql`].
fn conversation_sql() -> String {
    format!(
        "INSERT INTO chat_conversations
            (id, project_id, conversation_kind, last_activity_at, created_at, updated_at)
         VALUES ({conversation}, {project}, 'channel', {created}, {created}, {created});
         INSERT INTO chat_channels (id, project_id, conversation_id, name, created_at, updated_at)
         VALUES ({channel}, {project}, {conversation}, 'design', {created}, {created});
         INSERT INTO chat_conversation_memberships
            (conversation_id, participant_id, sync_key, membership_role, created_at, updated_at)
         VALUES ({conversation}, {owner}, {owner_key}, 'owner', {created}, {created}),
                ({conversation}, {teammate}, {teammate_key}, 'member', {created}, {created});
         INSERT INTO chat_teammate_channel_memberships
            (conversation_id, teammate_id, sync_key, access_profile_id, read_history,
             participate, created_at, updated_at)
         VALUES ({conversation}, {teammate}, {access_key}, {profile}, 1, 1, {created}, {created});
         INSERT INTO chat_teammate_working_folder_grants
            (conversation_id, teammate_id, working_folder_id, sync_key, project_id, capability,
             is_default, created_at)
         VALUES ({conversation}, {teammate}, {folder}, {grant_key}, {project}, 'edit', 1,
                 {created});",
        conversation = quoted(CONVERSATION),
        channel = quoted(CHANNEL),
        project = quoted(PROJECT),
        folder = quoted(FOLDER),
        owner = quoted(OWNER),
        teammate = quoted(TEAMMATE),
        profile = quoted(PROFILE),
        created = quoted(CREATED),
        owner_key = quoted(&key(MEMBERSHIP_KEY_PREFIX, &[CONVERSATION, OWNER])),
        teammate_key = quoted(&key(MEMBERSHIP_KEY_PREFIX, &[CONVERSATION, TEAMMATE])),
        access_key = quoted(&key(
            CHANNEL_MEMBERSHIP_KEY_PREFIX,
            &[CONVERSATION, TEAMMATE]
        )),
        grant_key = quoted(&key(
            FOLDER_GRANT_KEY_PREFIX,
            &[CONVERSATION, TEAMMATE, FOLDER]
        )),
    )
}

/// SQL setting [`TEAMMATE`]'s grant to `capability` with `approval`, revoked at `revoked_at`
/// when given.
fn grant_sql(capability: &str, approval: Option<&str>, revoked_at: Option<&str>) -> String {
    format!(
        "UPDATE chat_teammate_working_folder_grants
         SET capability = {capability}, runtime_approval_policy = {approval},
             revoked_at = {revoked_at}, revision = revision + 1
         WHERE teammate_id = {teammate};",
        capability = quoted(capability),
        approval = optional(approval),
        revoked_at = optional(revoked_at),
        teammate = quoted(TEAMMATE),
    )
}

/// SQL setting [`TEAMMATE`]'s conversation membership to `role`, removed at `removed_at` when
/// given.
fn membership_sql(role: &str, removed_at: Option<&str>, updated_at: &str) -> String {
    format!(
        "UPDATE chat_conversation_memberships
         SET membership_role = {role}, removed_at = {removed_at}, updated_at = {updated_at},
             revision = revision + 1
         WHERE participant_id = {teammate};",
        role = quoted(role),
        removed_at = optional(removed_at),
        updated_at = quoted(updated_at),
        teammate = quoted(TEAMMATE),
    )
}

/// SQL setting [`TEAMMATE`]'s AI channel access to `read_history` and `participate`, removed at
/// `removed_at` when given.
fn access_sql(
    read_history: bool,
    participate: bool,
    removed_at: Option<&str>,
    updated_at: &str,
) -> String {
    format!(
        "UPDATE chat_teammate_channel_memberships
         SET read_history = {read}, participate = {participate}, removed_at = {removed_at},
             updated_at = {updated_at}
         WHERE teammate_id = {teammate};",
        read = i32::from(read_history),
        participate = i32::from(participate),
        removed_at = optional(removed_at),
        updated_at = quoted(updated_at),
        teammate = quoted(TEAMMATE),
    )
}

/// [`TEAMMATE`]'s grant as `capability approval active|revoked`.
async fn grant_state(replica: &mut Replica) -> String {
    replica
        .scalar_text(
            "SELECT capability || ' ' || coalesce(runtime_approval_policy, 'inherit') || ' '
                    || CASE WHEN revoked_at IS NULL THEN 'active' ELSE 'revoked' END
             FROM chat_teammate_working_folder_grants
             WHERE teammate_id = 'participant:reviewer'",
        )
        .await
        .expect("the grant arrived")
}

/// [`TEAMMATE`]'s conversation membership as `role active|removed`.
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

/// [`TEAMMATE`]'s AI channel access as the number of cleared capabilities, or `None` once
/// removed.
async fn access_cleared(replica: &mut Replica) -> Option<i64> {
    let removed = replica
        .count(
            "SELECT count(*) FROM chat_teammate_channel_memberships
             WHERE teammate_id = 'participant:reviewer' AND removed_at IS NOT NULL",
        )
        .await;
    if removed > 0 {
        return None;
    }
    Some(
        replica
            .count(
                "SELECT (read_history = 0) + (participate = 0)
                        + (history_boundary = 'from_grant')
                 FROM chat_teammate_channel_memberships
                 WHERE teammate_id = 'participant:reviewer'",
            )
            .await,
    )
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
                            || channel.is_default
                     FROM chat_conversations conversation
                     JOIN chat_channels channel ON channel.conversation_id = conversation.id
                     WHERE conversation.id = 'conversation:design'",
                )
                .await
                .as_deref(),
            Some("channel design 0")
        );
        assert_eq!(membership_state(reader).await, "member active");
        assert_eq!(access_cleared(reader).await, Some(0));
        assert_eq!(grant_state(reader).await, "edit inherit active");
        assert_eq!(
            reader
                .scalar_text(
                    "SELECT sync_key FROM chat_teammate_working_folder_grants
                     WHERE teammate_id = 'participant:reviewer'",
                )
                .await,
            Some(key(
                FOLDER_GRANT_KEY_PREFIX,
                &[CONVERSATION, TEAMMATE, FOLDER]
            ))
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
fn chat_concurrent_revocation_and_widening_keep_the_revocation() {
    block_on(async {
        let mut replicas = replicas(2).await;
        replicas[0]
            .exec(&format!("{}{}", identity_sql(), conversation_sql()))
            .await;
        converge(&mut replicas).await;

        // Replica 1 revokes everything first; replica 0 widens everything later.
        let revoked = at(1, 0);
        replicas[1]
            .exec(&format!(
                "{}{}{}",
                grant_sql("edit", None, Some(&revoked)),
                membership_sql("member", Some(&revoked), &revoked),
                access_sql(true, true, Some(&revoked), &revoked),
            ))
            .await;
        let widened = at(2, 0);
        replicas[0]
            .exec(&format!(
                "{}{}{}",
                grant_sql("publish", Some("unattended"), None),
                membership_sql("owner", None, &widened),
                access_sql(true, true, None, &widened),
            ))
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(grant_state(replica).await, "edit inherit revoked");
            assert_eq!(membership_state(replica).await, "member removed");
            assert_eq!(access_cleared(replica).await, None);
        }
    });
}

/// SQL setting [`TEAMMATE`]'s AI channel approval override to `approval`, keeping its
/// capabilities.
fn access_approval_sql(approval: Option<&str>, updated_at: &str) -> String {
    format!(
        "UPDATE chat_teammate_channel_memberships
         SET runtime_approval_policy = {approval}, updated_at = {updated_at}
         WHERE teammate_id = {teammate};",
        approval = optional(approval),
        updated_at = quoted(updated_at),
        teammate = quoted(TEAMMATE),
    )
}

/// [`TEAMMATE`]'s AI channel access as `read_history participate approval`.
async fn access_state(replica: &mut Replica) -> String {
    replica
        .scalar_text(
            "SELECT read_history || ' ' || participate || ' '
                    || coalesce(runtime_approval_policy, 'inherit')
             FROM chat_teammate_channel_memberships
             WHERE teammate_id = 'participant:reviewer'",
        )
        .await
        .expect("the access arrived")
}

#[test]
fn chat_concurrent_inherited_approval_does_not_replace_an_explicit_narrowing() {
    block_on(async {
        let mut replicas = replicas(2).await;
        replicas[0]
            .exec(&format!(
                "{}{}{}{}",
                identity_sql(),
                conversation_sql(),
                grant_sql("edit", Some("provider_custom"), None),
                access_approval_sql(Some("provider_custom"), &at(0, 30)),
            ))
            .await;
        converge(&mut replicas).await;

        // Replica 0 narrows both overrides to unattended; replica 1 later clears them, which
        // falls back to the teammate default that may be wider.
        replicas[0]
            .exec(&format!(
                "{}{}",
                grant_sql("edit", Some("unattended"), None),
                access_approval_sql(Some("unattended"), &at(1, 0)),
            ))
            .await;
        replicas[1]
            .exec(&format!(
                "{}{}",
                grant_sql("edit", None, None),
                access_approval_sql(None, &at(2, 0)),
            ))
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(grant_state(replica).await, "edit unattended active");
            assert_eq!(access_state(replica).await, "1 1 unattended");
        }
    });
}

/// SQL setting [`TEAMMATE`]'s scratch runtime approval override.
fn scratch_approval_sql(approval: Option<&str>, updated_at: &str) -> String {
    format!(
        "UPDATE chat_teammate_channel_memberships
         SET scratch_runtime_approval_policy = {approval}, updated_at = {updated_at}
         WHERE teammate_id = {teammate};",
        approval = optional(approval),
        updated_at = quoted(updated_at),
        teammate = quoted(TEAMMATE),
    )
}

/// [`TEAMMATE`]'s scratch runtime approval override, or `inherit`.
async fn scratch_state(replica: &mut Replica) -> String {
    replica
        .scalar_text(
            "SELECT coalesce(scratch_runtime_approval_policy, 'inherit')
             FROM chat_teammate_channel_memberships
             WHERE teammate_id = 'participant:reviewer'",
        )
        .await
        .expect("the access arrived")
}

/// [`TEAMMATE`]'s folder and scratch approval overrides as the runtime resolves them, each
/// falling back to the channel override: `folder scratch`.
async fn resolved_target_approvals(replica: &mut Replica) -> String {
    replica
        .scalar_text(
            "SELECT coalesce(folder_grant.runtime_approval_policy, access.runtime_approval_policy,
                             'inherit')
                    || ' ' || coalesce(access.scratch_runtime_approval_policy,
                                       access.runtime_approval_policy, 'inherit')
             FROM chat_teammate_channel_memberships access
             JOIN chat_teammate_working_folder_grants folder_grant
               ON folder_grant.conversation_id = access.conversation_id
              AND folder_grant.teammate_id = access.teammate_id
             WHERE access.teammate_id = 'participant:reviewer'",
        )
        .await
        .expect("the access and grant arrived")
}

/// An explicit folder or scratch override beats a concurrent clear by static rank, whatever the
/// channel override holds. With the channel override looser than the explicit value, as here,
/// clearing would inherit the looser value, so the winner is also the stricter resolved
/// approval; the opposite channel case is the open inherit residual.
#[test]
fn chat_concurrent_explicit_target_override_beats_a_clear_under_a_looser_channel_override() {
    block_on(async {
        let mut replicas = replicas(2).await;
        replicas[0]
            .exec(&format!(
                "{}{}{}{}{}",
                identity_sql(),
                conversation_sql(),
                access_approval_sql(Some("unattended"), &at(0, 30)),
                grant_sql("edit", Some("auto_approve"), None),
                scratch_approval_sql(Some("auto_approve"), &at(0, 40)),
            ))
            .await;
        converge(&mut replicas).await;

        replicas[0]
            .exec(&format!(
                "{}{}",
                grant_sql("edit", None, None),
                scratch_approval_sql(None, &at(1, 0)),
            ))
            .await;
        replicas[1]
            .exec(&format!(
                "{}{}",
                grant_sql("edit", Some("ask"), None),
                scratch_approval_sql(Some("ask"), &at(2, 0)),
            ))
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(grant_state(replica).await, "edit ask active");
            assert_eq!(scratch_state(replica).await, "ask");
            assert_eq!(access_state(replica).await, "1 1 unattended");
            assert_eq!(resolved_target_approvals(replica).await, "ask ask");
        }
    });
}

#[test]
fn chat_concurrent_capability_clear_outranks_a_stricter_approval() {
    block_on(async {
        let mut replicas = replicas(2).await;
        replicas[0]
            .exec(&format!("{}{}", identity_sql(), conversation_sql()))
            .await;
        converge(&mut replicas).await;

        // Replica 1 stops participation; replica 0 later asks for every approval but keeps
        // participating.
        replicas[1]
            .exec(&access_sql(true, false, None, &at(1, 0)))
            .await;
        replicas[0]
            .exec(&access_approval_sql(Some("ask"), &at(2, 0)))
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(access_state(replica).await, "1 0 inherit");
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
                "{}{}{}",
                grant_sql("edit", None, Some(&removed)),
                membership_sql("member", Some(&removed), &removed),
                access_sql(true, true, Some(&removed), &removed),
            ))
            .await;
        converge(&mut replicas).await;

        // Replica 1 saw the removal, so its re-add is causally later and replaces it.
        let added = at(2, 0);
        replicas[1]
            .exec(&format!(
                "{}{}{}",
                grant_sql("read", None, None),
                membership_sql("member", None, &added),
                access_sql(true, false, None, &added),
            ))
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(grant_state(replica).await, "read inherit active");
            assert_eq!(membership_state(replica).await, "member active");
            assert_eq!(access_cleared(replica).await, Some(1));
        }
    });
}

/// Raw replicated values of [`TEAMMATE`]'s grant that [`grant_sql`] writes.
const GRANT_VALUE: &str = "SELECT capability || '|' || coalesce(runtime_approval_policy, '') || '|'
        || coalesce(revoked_at, '')
     FROM chat_teammate_working_folder_grants WHERE teammate_id = 'participant:reviewer'";

/// Raw replicated values of [`TEAMMATE`]'s membership that [`membership_sql`] writes.
const MEMBERSHIP_VALUE: &str = "SELECT membership_role || '|' || coalesce(removed_at, '')
     FROM chat_conversation_memberships WHERE participant_id = 'participant:reviewer'";

/// Raw replicated values of [`TEAMMATE`]'s AI channel access that [`access_sql`] writes.
const ACCESS_VALUE: &str = "SELECT read_history || '|' || participate || '|'
        || coalesce(removed_at, '')
     FROM chat_teammate_channel_memberships WHERE teammate_id = 'participant:reviewer'";

/// Runs `sql` on `replica` and says whether it changed the value `value` reads. A write that
/// leaves a group's value as it was captures nothing, so it is no concurrent edit.
async fn edits(replica: &mut Replica, value: &str, sql: &str) -> bool {
    let before = replica.scalar_text(value).await;
    replica.exec(sql).await;
    replica.scalar_text(value).await != before
}

/// The edits of one access round: whether a replica revoked each row, and otherwise the
/// strictest value a replica wrote.
#[derive(Default)]
struct AccessRound {
    grant_revoked: bool,
    grant: Option<(usize, usize)>,
    membership_removed: bool,
    role: Option<usize>,
    access_removed: bool,
    cleared: Option<i64>,
}

#[test]
fn chat_simulated_revoke_against_widen_on_a_grant_and_a_membership_keeps_revocations() {
    block_on(async {
        for seed in 0..ACCESS_SEEDS {
            let mut rng = Rng(seed ^ 0x5eed);
            let mut replicas = replicas(3).await;
            replicas[0]
                .exec(&format!("{}{}", identity_sql(), conversation_sql()))
                .await;
            converge(&mut replicas).await;
            let mut expected_grant = Some("edit inherit active".to_owned());
            let mut expected_role = "member active".to_owned();
            let mut expected_cleared = Some(0);
            for round in 0..ACCESS_ROUNDS {
                let mut round_edits = AccessRound::default();
                for replica in &mut replicas {
                    let stamp = at(round + 1, rng.below(60));
                    if rng.chance(60) {
                        if rng.chance(40) {
                            let sql = grant_sql("edit", None, Some(&stamp));
                            round_edits.grant_revoked |= edits(replica, GRANT_VALUE, &sql).await;
                        } else {
                            let capability = rng.below(CAPABILITIES.len());
                            let approval = rng.below(APPROVALS.len());
                            let sql =
                                grant_sql(CAPABILITIES[capability], APPROVALS[approval], None);
                            if edits(replica, GRANT_VALUE, &sql).await {
                                round_edits.grant =
                                    round_edits.grant.max(Some((capability, approval)));
                            }
                        }
                    }
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
                        if rng.chance(40) {
                            let sql = access_sql(true, true, Some(&stamp), &stamp);
                            round_edits.access_removed |= edits(replica, ACCESS_VALUE, &sql).await;
                        } else {
                            let read = rng.chance(50);
                            let participate = rng.chance(50);
                            let sql = access_sql(read, participate, None, &stamp);
                            if edits(replica, ACCESS_VALUE, &sql).await {
                                let cleared = i64::from(!read) + i64::from(!participate);
                                round_edits.cleared = round_edits.cleared.max(Some(cleared));
                            }
                        }
                    }
                    if rng.chance(50) {
                        replica.seal().await.unwrap();
                    }
                }
                converge(&mut replicas).await;
                assert_converged(&mut replicas).await;

                // Revocation outranks every widening; any revoked value may win among several.
                if round_edits.grant_revoked {
                    expected_grant = None;
                } else if let Some((capability, approval)) = round_edits.grant {
                    expected_grant = Some(format!(
                        "{} {} active",
                        CAPABILITIES[capability],
                        APPROVALS[approval].unwrap_or("inherit")
                    ));
                }
                if round_edits.membership_removed {
                    expected_role = "member removed".to_owned();
                } else if let Some(role) = round_edits.role {
                    expected_role = format!("{} active", ROLES[role]);
                }
                if round_edits.access_removed {
                    expected_cleared = None;
                } else if round_edits.cleared.is_some() {
                    expected_cleared = round_edits.cleared;
                }
                for replica in &mut replicas {
                    let grant = grant_state(replica).await;
                    match &expected_grant {
                        Some(expected) => {
                            assert_eq!(&grant, expected, "seed {seed} round {round}");
                        }
                        None => assert!(grant.ends_with(" revoked"), "seed {seed} round {round}"),
                    }
                    assert_eq!(
                        membership_state(replica).await,
                        expected_role,
                        "seed {seed} round {round}"
                    );
                    assert_eq!(
                        access_cleared(replica).await,
                        expected_cleared,
                        "seed {seed} round {round}"
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

/// A project group deleted on one replica cascades into the conversations of its projects on
/// the other, and the receiver's Chat hook is handed those conversation keys, so it can report
/// them removed.
#[test]
fn chat_hook_sees_conversations_an_applied_group_delete_cascades_away() {
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
        let group = replicas[0]
            .scalar_text(&format!(
                "SELECT group_id FROM projects WHERE id = {}",
                quoted(PROJECT)
            ))
            .await
            .expect("the project has a group");
        replicas[0]
            .exec(&format!(
                "DELETE FROM project_groups WHERE id = {}",
                quoted(&group)
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
