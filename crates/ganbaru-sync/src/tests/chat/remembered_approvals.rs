//! Remembered approvals across replicas: an approval arrives whole under a derived key that names
//! its channel and subject, a revocation wins over a concurrent re-approval, removing the agent
//! from the channel on one device revokes the approval there and wins over a concurrent
//! re-approval on another, and the subject kind and access are checked on admission.

use super::*;
use ganbaru_sync_contracts::chat_remembered_approvals::{
    MAX_REMEMBERED_APPROVAL_SUBJECT_BYTES, REMEMBERED_APPROVAL_KEY_PREFIX,
};

/// The subject kind every approval names unless a test says otherwise.
const KIND: &str = "url_origin";
/// The subject key every approval names unless a test says otherwise.
const ORIGIN: &str = "https://docs.example.com";

/// The derived key of [`AGENT`]'s approval of `kind` and `subject` in [`CONVERSATION`].
fn approval_key(kind: &str, subject: &str) -> String {
    key(
        REMEMBERED_APPROVAL_KEY_PREFIX,
        &[CONVERSATION, AGENT, kind, subject],
    )
}

/// SQL approving [`AGENT`]'s requests of `kind` on `subject` in [`CONVERSATION`] at `at` under
/// `access`, or approving them again when the row exists, as the resolve path writes it.
fn approve_sql(kind: &str, subject: &str, access: &str, at: &str) -> String {
    format!(
        "INSERT INTO chat_remembered_approvals
            (conversation_id, agent_id, subject_kind, subject_key, sync_key,
             approved_by_participant_id, approved_at, granted_access)
         VALUES ({conversation}, {agent}, {kind}, {subject}, {key}, {owner}, {at}, {access})
         ON CONFLICT (conversation_id, agent_id, subject_kind, subject_key)
         DO UPDATE SET
             approved_by_participant_id = excluded.approved_by_participant_id,
             approved_at = excluded.approved_at, granted_access = excluded.granted_access,
             revoked_at = NULL, revision = revision + 1;",
        conversation = quoted(CONVERSATION),
        agent = quoted(AGENT),
        kind = quoted(kind),
        subject = quoted(subject),
        key = quoted(&approval_key(kind, subject)),
        owner = quoted(OWNER),
        at = quoted(at),
        access = quoted(access),
    )
}

/// SQL revoking the approval of `subject` at `at`, as the settings list does.
fn revoke_sql(subject: &str, at: &str) -> String {
    format!(
        "UPDATE chat_remembered_approvals SET revoked_at = {at}, revision = revision + 1
         WHERE subject_key = {subject} AND revoked_at IS NULL;",
        subject = quoted(subject),
        at = quoted(at),
    )
}

/// The approval of `subject` as `approved_at granted_access active|revoked`, or `None`.
async fn approval_state(replica: &mut Replica, subject: &str) -> Option<String> {
    replica
        .scalar_text(&format!(
            "SELECT approved_at || ' ' || granted_access || ' '
                    || CASE WHEN revoked_at IS NULL THEN 'active' ELSE 'revoked' END
             FROM chat_remembered_approvals WHERE subject_key = {}",
            quoted(subject)
        ))
        .await
}

/// Replicas sharing the conversation rows and an approval of [`ORIGIN`] made at [`CREATED`].
async fn approved_replicas() -> Vec<Replica> {
    let mut replicas = conversation_replicas(2).await;
    replicas[0]
        .exec(&approve_sql(KIND, ORIGIN, "ask", CREATED))
        .await;
    converge(&mut replicas).await;
    replicas
}

#[test]
fn chat_remembered_approvals_round_trip_with_their_derived_key() {
    block_on(async {
        let mut replicas = approved_replicas().await;
        let longest = "c".repeat(MAX_REMEMBERED_APPROVAL_SUBJECT_BYTES);
        replicas[0]
            .exec(&approve_sql("command", &longest, "own", &at(1, 0)))
            .await;
        replicas[0]
            .exec(&approve_sql("tool", "mcp__docs__search", "read", &at(1, 0)))
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        let reader = &mut replicas[1];
        assert_eq!(
            approval_state(reader, ORIGIN).await,
            Some(format!("{CREATED} ask active"))
        );
        assert_eq!(
            approval_state(reader, &longest).await,
            Some(format!("{} own active", at(1, 0)))
        );
        assert_eq!(
            approval_state(reader, "mcp__docs__search").await,
            Some(format!("{} read active", at(1, 0)))
        );
        assert_eq!(
            reader
                .scalar_text(&format!(
                    "SELECT sync_key || ' ' || approved_by_participant_id
                     FROM chat_remembered_approvals WHERE subject_key = {}",
                    quoted(ORIGIN)
                ))
                .await,
            Some(format!("{} {OWNER}", approval_key(KIND, ORIGIN)))
        );
        // The kind is part of the identity, so the same text as a command is another approval.
        assert_ne!(approval_key(KIND, ORIGIN), approval_key("command", ORIGIN));
    });
}

/// One device revokes an approval while another approves the same subject again: the revocation
/// wins everywhere, and an approval made after seeing it makes the approval active again.
#[test]
fn chat_remembered_approval_revocation_wins_over_a_concurrent_re_approval() {
    block_on(async {
        for later in [0_usize, 1] {
            let mut replicas = approved_replicas().await;
            replicas[1 - later]
                .exec(&revoke_sql(ORIGIN, &at(1, 0)))
                .await;
            replicas[later]
                .exec(&approve_sql(KIND, ORIGIN, "own", &at(1, 30)))
                .await;
            replicas[later].now_ms += LATER_MS;
            converge(&mut replicas).await;
            assert_converged(&mut replicas).await;
            for replica in &mut replicas {
                assert!(
                    approval_state(replica, ORIGIN)
                        .await
                        .is_some_and(|state| state.ends_with(" revoked")),
                    "replica {}, later edit from replica {later}",
                    replica.index
                );
            }

            replicas[1]
                .exec(&approve_sql(KIND, ORIGIN, "read", &at(2, 0)))
                .await;
            converge(&mut replicas).await;
            assert_converged(&mut replicas).await;
            for replica in &mut replicas {
                assert_eq!(
                    approval_state(replica, ORIGIN).await,
                    Some(format!("{} read active", at(2, 0)))
                );
            }
        }
    });
}

/// One device removes the agent from the channel, which revokes its approvals there, while
/// another approves the subject again: the removal and the revocation win everywhere.
#[test]
fn chat_channel_removal_revokes_remembered_approvals_against_a_concurrent_re_approval() {
    block_on(async {
        let mut replicas = approved_replicas().await;
        replicas[0]
            .exec(&channel_membership_sql(Some(&at(1, 0)), &at(1, 0)))
            .await;
        assert!(
            approval_state(&mut replicas[0], ORIGIN)
                .await
                .is_some_and(|state| state.ends_with(" revoked"))
        );
        replicas[1]
            .exec(&approve_sql(KIND, ORIGIN, "ask", &at(1, 30)))
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(channel_membership_state(replica).await, "removed");
            assert!(
                approval_state(replica, ORIGIN)
                    .await
                    .is_some_and(|state| state.ends_with(" revoked")),
                "replica {}",
                replica.index
            );
        }
    });
}

/// Deleting the project removes the approvals of its channels on every device.
#[test]
fn chat_deleting_a_project_removes_its_remembered_approvals() {
    block_on(async {
        let mut replicas = approved_replicas().await;
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
                    .count("SELECT count(*) FROM chat_remembered_approvals")
                    .await,
                0
            );
        }
    });
}
