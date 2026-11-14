//! Standing push approvals across replicas: an approval arrives whole under its derived key, a
//! revocation wins over a concurrent re-approval, a grant narrowing on one device revokes the
//! approval there and wins over a concurrent re-approval on another, and a re-approval that saw
//! the revocation makes the approval active again.

use super::*;
use ganbaru_sync_contracts::chat_push_approvals::{
    MAX_PUSH_APPROVAL_BRANCH_BYTES, PUSH_APPROVAL_KEY_PREFIX,
};

/// The push URL every approval names.
const PUSH_URL: &str = "git@example.com:team/app.git";

/// The destination branch every approval names.
const BRANCH: &str = "main";

/// The derived key of [`TEAMMATE`]'s approval on [`FOLDER`] for `branch`.
fn approval_key(branch: &str) -> String {
    key(
        PUSH_APPROVAL_KEY_PREFIX,
        &[TEAMMATE, FOLDER, PUSH_URL, branch],
    )
}

/// SQL approving [`TEAMMATE`]'s pushes from [`FOLDER`] to `branch` at `at` for `policy`, or
/// approving them again when the row exists, as the confirm path writes it.
fn approve_sql(branch: &str, policy: &str, at: &str) -> String {
    format!(
        "INSERT INTO chat_push_approvals
            (teammate_id, working_folder_id, push_url, destination_branch, sync_key,
             runtime_approval_policy, safety_mode, approved_by_participant_id, approved_at)
         VALUES ({teammate}, {folder}, {url}, {branch}, {key}, {policy}, 'ask_for_approval',
                 {owner}, {at})
         ON CONFLICT (teammate_id, working_folder_id, push_url, destination_branch) DO UPDATE SET
             runtime_approval_policy = excluded.runtime_approval_policy,
             safety_mode = excluded.safety_mode,
             approved_by_participant_id = excluded.approved_by_participant_id,
             approved_at = excluded.approved_at, revoked_at = NULL, revision = revision + 1;",
        teammate = quoted(TEAMMATE),
        folder = quoted(FOLDER),
        url = quoted(PUSH_URL),
        branch = quoted(branch),
        key = quoted(&approval_key(branch)),
        policy = quoted(policy),
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

/// The approval for `branch` as `policy active|revoked`, or `None` when absent.
async fn approval_state(replica: &mut Replica, branch: &str) -> Option<String> {
    replica
        .scalar_text(&format!(
            "SELECT runtime_approval_policy || ' '
                    || CASE WHEN revoked_at IS NULL THEN 'active' ELSE 'revoked' END
             FROM chat_push_approvals WHERE destination_branch = {}",
            quoted(branch)
        ))
        .await
}

/// Replicas sharing the conversation rows with [`TEAMMATE`] allowed to publish from [`FOLDER`]
/// and an approval for [`BRANCH`].
async fn approved_replicas() -> Vec<Replica> {
    let mut replicas = conversation_replicas(2).await;
    replicas[0]
        .exec(&format!(
            "{}{}",
            grant_sql("publish", None, None),
            approve_sql(BRANCH, "ask", CREATED)
        ))
        .await;
    converge(&mut replicas).await;
    replicas
}

#[test]
fn chat_push_approvals_round_trip_with_their_derived_key() {
    block_on(async {
        let mut replicas = approved_replicas().await;
        let longest = "b".repeat(MAX_PUSH_APPROVAL_BRANCH_BYTES);
        replicas[0]
            .exec(&approve_sql(&longest, "auto_approve", &at(1, 0)))
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        let reader = &mut replicas[1];
        assert_eq!(
            approval_state(reader, BRANCH).await.as_deref(),
            Some("ask active")
        );
        assert_eq!(
            approval_state(reader, &longest).await.as_deref(),
            Some("auto_approve active")
        );
        assert_eq!(
            reader
                .scalar_text(&format!(
                    "SELECT sync_key || ' ' || approved_by_participant_id
                     FROM chat_push_approvals WHERE destination_branch = {}",
                    quoted(BRANCH)
                ))
                .await,
            Some(format!("{} {OWNER}", approval_key(BRANCH)))
        );
    });
}

/// One device revokes an approval while another approves the same push again: the revocation
/// wins everywhere, and an approval made after seeing it makes the approval active again.
#[test]
fn chat_push_approval_revocation_wins_over_a_concurrent_re_approval() {
    block_on(async {
        let mut replicas = approved_replicas().await;
        replicas[0].exec(&revoke_sql(BRANCH, &at(1, 0))).await;
        replicas[1]
            .exec(&approve_sql(BRANCH, "auto_approve", &at(1, 30)))
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(
                approval_state(replica, BRANCH).await.as_deref(),
                Some("ask revoked"),
                "replica {}",
                replica.index
            );
        }

        replicas[1]
            .exec(&approve_sql(BRANCH, "ask", &at(2, 0)))
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(
                approval_state(replica, BRANCH).await.as_deref(),
                Some("ask active")
            );
        }
    });
}

/// One device narrows the teammate's grant, which revokes its approvals there, while another
/// approves the push again: the narrowing and the revocation win everywhere.
#[test]
fn chat_grant_narrowing_revokes_push_approvals_against_a_concurrent_re_approval() {
    block_on(async {
        let mut replicas = approved_replicas().await;
        replicas[0].exec(&grant_sql("edit", None, None)).await;
        assert_eq!(
            approval_state(&mut replicas[0], BRANCH).await.as_deref(),
            Some("ask revoked")
        );
        replicas[1]
            .exec(&approve_sql(BRANCH, "ask", &at(1, 30)))
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(grant_state(replica).await, "edit inherit active");
            assert_eq!(
                approval_state(replica, BRANCH).await.as_deref(),
                Some("ask revoked")
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
