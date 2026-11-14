use super::*;
use crate::chat::revocation::tests::pool_with_folder_runs;

const TEAMMATE: &str = "participant:folder-agent";

/// Approves the fixture teammate's pushes from `folder` to `branch` under key `key`.
async fn approve(pool: &SqlitePool, key: &str, folder: &str, branch: &str) {
    sqlx::query(
        "UPDATE chat_teammates SET current_policy_revision_id = 'policy:folders',
             current_policy_safety_mode = 'ask_for_approval'
         WHERE participant_id = ? AND current_policy_safety_mode IS NULL",
    )
    .bind(TEAMMATE)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO chat_push_approvals
            (teammate_id, working_folder_id, push_url, destination_branch, sync_key,
             runtime_approval_policy, safety_mode, approved_by_participant_id, approved_at)
         VALUES (?, ?, 'git@example.invalid:org/repo.git', ?, ?, 'ask', 'ask_for_approval',
                 'participant:local-owner', '2026-10-20T12:00:00.000Z')",
    )
    .bind(TEAMMATE)
    .bind(folder)
    .bind(branch)
    .bind(key)
    .execute(pool)
    .await
    .unwrap();
}

fn listed_keys(approvals: &[ChatPushApprovalRead]) -> Vec<&str> {
    approvals
        .iter()
        .map(|approval| approval.approval_id.as_str())
        .collect()
}

#[tokio::test]
async fn listing_shows_active_approvals_by_folder_name_and_revoking_hides_one() {
    let pool = pool_with_folder_runs().await;
    approve(&pool, "push-approval-unrelated", "workspace-3", "main").await;
    approve(&pool, "push-approval-reference", "workspace-2", "main").await;
    approve(&pool, "push-approval-release", "workspace-2", "release").await;
    let teammate = ChatParticipantId::new(TEAMMATE).unwrap();

    let approvals = list_push_approvals(&pool, &teammate).await.unwrap();
    assert_eq!(
        listed_keys(&approvals),
        [
            "push-approval-reference",
            "push-approval-release",
            "push-approval-unrelated"
        ]
    );
    assert_eq!(approvals[0].working_folder_name, "Reference");
    assert_eq!(approvals[0].working_folder_id.as_str(), "workspace-2");
    assert_eq!(approvals[0].push_url, "git@example.invalid:org/repo.git");
    assert_eq!(approvals[0].destination_branch, "main");

    let revoke = |approval_id: &str| RevokeChatPushApprovalCommand {
        approval_id: approval_id.to_string(),
    };
    revoke_push_approval(&pool, &revoke("push-approval-release"))
        .await
        .unwrap();
    revoke_push_approval(&pool, &revoke("push-approval-release"))
        .await
        .unwrap();
    revoke_push_approval(&pool, &revoke("push-approval-missing"))
        .await
        .unwrap();
    let approvals = list_push_approvals(&pool, &teammate).await.unwrap();
    assert_eq!(
        listed_keys(&approvals),
        ["push-approval-reference", "push-approval-unrelated"]
    );
    let revision: i64 = sqlx::query_scalar(
        "SELECT revision FROM chat_push_approvals WHERE sync_key = 'push-approval-release'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(revision, 2, "revoking twice changes the row once");

    for malformed in ["", "  ", &"k".repeat(MAX_APPROVAL_ID_BYTES + 1)] {
        assert_eq!(
            revoke_push_approval(&pool, &revoke(malformed))
                .await
                .unwrap_err()
                .code,
            ChatErrorCode::Validation
        );
    }
    let other = ChatParticipantId::new("participant:local-owner").unwrap();
    assert!(list_push_approvals(&pool, &other).await.unwrap().is_empty());
}
