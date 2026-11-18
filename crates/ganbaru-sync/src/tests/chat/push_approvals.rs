//! Standing push approvals across replicas: an approval arrives whole under a derived key that
//! names its channel, a revocation wins over a concurrent re-approval, removing the agent from
//! the channel on one device revokes the approval there and wins over a concurrent re-approval
//! on another, and a re-approval that saw the revocation makes the approval active again.

use super::*;
use ganbaru_sync_contracts::chat_push_approvals::MAX_PUSH_APPROVAL_BRANCH_BYTES;

/// The destination branch every approval names.
const BRANCH: &str = APPROVED_BRANCH;

/// The derived key of [`AGENT`]'s approval in [`CONVERSATION`] on [`FOLDER`] for `branch`.
fn approval_key(branch: &str) -> String {
    key(
        PUSH_APPROVAL_KEY_PREFIX,
        &[CONVERSATION, AGENT, FOLDER, APPROVED_PUSH_URL, branch],
    )
}

/// SQL approving [`AGENT`]'s pushes from [`FOLDER`] to `branch` in [`CONVERSATION`] at `at`, or
/// approving them again when the row exists, as the confirm path writes it.
fn approve_sql(branch: &str, at: &str) -> String {
    format!(
        "INSERT INTO chat_push_approvals
            (conversation_id, agent_id, working_folder_id, push_url, destination_branch,
             sync_key, approved_by_participant_id, approved_at)
         VALUES ({conversation}, {agent}, {folder}, {url}, {branch}, {key}, {owner}, {at})
         ON CONFLICT (conversation_id, agent_id, working_folder_id, push_url, destination_branch)
         DO UPDATE SET
             approved_by_participant_id = excluded.approved_by_participant_id,
             approved_at = excluded.approved_at, revoked_at = NULL, revision = revision + 1;",
        conversation = quoted(CONVERSATION),
        agent = quoted(AGENT),
        folder = quoted(FOLDER),
        url = quoted(APPROVED_PUSH_URL),
        branch = quoted(branch),
        key = quoted(&approval_key(branch)),
        owner = quoted(OWNER),
        at = quoted(at),
    )
}

/// SQL revoking the approval for `branch` at `at`, as the settings list does.
fn revoke_sql(branch: &str, at: &str) -> String {
    format!(
        "UPDATE chat_push_approvals SET revoked_at = {at}, revision = revision + 1
         WHERE destination_branch = {branch} AND revoked_at IS NULL;",
        branch = quoted(branch),
        at = quoted(at),
    )
}

/// The approval for `branch` as `approved_at active|revoked`, or `None` when absent.
async fn approval_state(replica: &mut Replica, branch: &str) -> Option<String> {
    replica
        .scalar_text(&format!(
            "SELECT approved_at || ' '
                    || CASE WHEN revoked_at IS NULL THEN 'active' ELSE 'revoked' END
             FROM chat_push_approvals WHERE destination_branch = {}",
            quoted(branch)
        ))
        .await
}

/// Replicas sharing the conversation rows and an approval for [`BRANCH`] made at [`CREATED`].
async fn approved_replicas() -> Vec<Replica> {
    let mut replicas = conversation_replicas(2).await;
    replicas[0].exec(&approve_sql(BRANCH, CREATED)).await;
    converge(&mut replicas).await;
    replicas
}

#[test]
fn chat_push_approvals_round_trip_with_their_derived_key() {
    block_on(async {
        let mut replicas = approved_replicas().await;
        let longest = "b".repeat(MAX_PUSH_APPROVAL_BRANCH_BYTES);
        replicas[0].exec(&approve_sql(&longest, &at(1, 0))).await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        let reader = &mut replicas[1];
        assert_eq!(
            approval_state(reader, BRANCH).await,
            Some(format!("{CREATED} active"))
        );
        assert_eq!(
            approval_state(reader, &longest).await,
            Some(format!("{} active", at(1, 0)))
        );
        assert_eq!(
            reader
                .scalar_text(&format!(
                    "SELECT sync_key || ' ' || conversation_id || ' ' || approved_by_participant_id
                     FROM chat_push_approvals WHERE destination_branch = {}",
                    quoted(BRANCH)
                ))
                .await,
            Some(format!("{} {CONVERSATION} {OWNER}", approval_key(BRANCH)))
        );
        // The channel is part of the identity, so the same push approved in another channel
        // is another approval under another key.
        assert_ne!(
            approval_key(BRANCH),
            key(
                PUSH_APPROVAL_KEY_PREFIX,
                &[
                    "conversation:other",
                    AGENT,
                    FOLDER,
                    APPROVED_PUSH_URL,
                    BRANCH
                ],
            )
        );
    });
}

/// One device revokes an approval while another approves the same push again: the revocation
/// wins everywhere, and an approval made after seeing it makes the approval active again.
#[test]
fn chat_push_approval_revocation_wins_over_a_concurrent_re_approval() {
    block_on(async {
        for later in [0_usize, 1] {
            let mut replicas = approved_replicas().await;
            replicas[1 - later]
                .exec(&revoke_sql(BRANCH, &at(1, 0)))
                .await;
            replicas[later].exec(&approve_sql(BRANCH, &at(1, 30))).await;
            replicas[later].now_ms += LATER_MS;
            converge(&mut replicas).await;
            assert_converged(&mut replicas).await;
            for replica in &mut replicas {
                assert!(
                    approval_state(replica, BRANCH)
                        .await
                        .is_some_and(|state| state.ends_with(" revoked")),
                    "replica {}, later edit from replica {later}",
                    replica.index
                );
            }

            replicas[1].exec(&approve_sql(BRANCH, &at(2, 0))).await;
            converge(&mut replicas).await;
            assert_converged(&mut replicas).await;
            for replica in &mut replicas {
                assert_eq!(
                    approval_state(replica, BRANCH).await,
                    Some(format!("{} active", at(2, 0)))
                );
            }
        }
    });
}

/// One device removes the agent from the channel, which revokes its approvals there, while
/// another approves the push again: the removal and the revocation win everywhere.
#[test]
fn chat_channel_removal_revokes_push_approvals_against_a_concurrent_re_approval() {
    block_on(async {
        let mut replicas = approved_replicas().await;
        replicas[0]
            .exec(&channel_membership_sql(Some(&at(1, 0)), &at(1, 0)))
            .await;
        assert!(
            approval_state(&mut replicas[0], BRANCH)
                .await
                .is_some_and(|state| state.ends_with(" revoked"))
        );
        replicas[1].exec(&approve_sql(BRANCH, &at(1, 30))).await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(channel_membership_state(replica).await, "removed");
            assert!(
                approval_state(replica, BRANCH)
                    .await
                    .is_some_and(|state| state.ends_with(" revoked")),
                "replica {}",
                replica.index
            );
        }
    });
}

/// Deleting the project removes the approvals on its working folders on every device.
#[test]
fn chat_deleting_a_project_removes_its_push_approvals() {
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
                    .count("SELECT count(*) FROM chat_push_approvals")
                    .await,
                0
            );
        }
    });
}
