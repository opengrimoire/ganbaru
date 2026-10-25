//! Chat identity and policy across replicas: participants, teammates, and access profiles
//! arrive with their revisions, a crash while sealing or applying them leaves nothing partial,
//! and concurrent edits of a teammate's current policy or a profile's current revision keep the
//! more restrictive one whatever order the replicas exchange in. Device-local Chat rows stay on
//! their device.

use super::block_on;
use super::replica::{Replica, converge, quoted};
use super::sim::Rng;
use crate::error::failpoints;
use crate::manifest::vault::chat::{PARTICIPANTS_TABLE, PROFILE_REVISIONS_TABLE};
use crate::{SyncError, SyncResult};

const SALT: u64 = 73;
const TEAMMATE: &str = "participant:reviewer";
const PROFILE: &str = "access-profile:reviewers";
const CREATED: &str = "2026-10-24T09:00:00.000Z";

/// Seeds of the concurrent policy edit scenario.
const POLICY_SEEDS: u64 = 8;

/// Edit rounds of one policy scenario.
const POLICY_ROUNDS: usize = 4;

/// Teammate safety modes, least restrictive first, as the adapter ranks them.
const SAFETY_MODES: [&str; 4] = [
    "full_access",
    "approve_for_me",
    "custom",
    "ask_for_approval",
];

/// Folder capabilities, least restrictive first, as the adapter ranks them.
const CAPABILITIES: [&str; 5] = ["publish", "execute", "edit", "read", "none"];

async fn replicas(count: usize) -> Vec<Replica> {
    let mut replicas = Vec::with_capacity(count);
    for index in 0..count {
        replicas.push(Replica::new(index, SALT).await);
    }
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

/// Captured Chat identity rows as `table key`, in table and key order.
async fn chat_captures(replica: &mut Replica) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT table_id || ' ' || row_key FROM sync_capture
         WHERE table_id BETWEEN ? AND ? ORDER BY table_id, row_key",
    )
    .bind(i64::from(PARTICIPANTS_TABLE.0))
    .bind(i64::from(PROFILE_REVISIONS_TABLE.0))
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
async fn after_rollback<T: std::fmt::Debug>(
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
        replicas[0].exec(&identity_sql()).await;
        let captured = chat_captures(&mut replicas[0]).await;
        assert_eq!(captured.len(), 5, "{captured:?}");
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
