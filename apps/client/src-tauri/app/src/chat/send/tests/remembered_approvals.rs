//! "Allow and don't ask again": which open approvals offer a rule, and which choices record one.

use super::super::remembered_approvals::{offered_rule, prepare_choice, resolve_then_remember};
use super::*;
use crate::chat::send_commands::ResolveChatApprovalCommand;

const ALLOW_ONCE_AND_DENY: &str = r#"[
    {"id": "allow", "label": "Allow once", "decisionKind": "allow_once", "description": null},
    {"id": "deny", "label": "Deny", "decisionKind": "deny", "description": null}
]"#;
const DENY_ONLY: &str =
    r#"[{"id": "deny", "label": "Deny", "decisionKind": "deny", "description": null}]"#;

fn display(kind: &str, text: &str) -> String {
    serde_json::json!({ "subject": { "kind": kind, "text": text, "truncated": false } }).to_string()
}

/// Opens the approval `request:remember` on `thread-1` with the given display data.
async fn open_approval(pool: &sqlx::SqlitePool, safe_display: &str) {
    sqlx::query(
        "INSERT INTO chat_pending_requests
            (id, thread_id, turn_id, provider_request_id, request_kind, safe_display_data,
             allowed_decisions_data, opened_sequence, opened_at)
         VALUES ('request:remember', 'thread-1', NULL, 'native-remember', 'approval', ?, ?, 1,
                 '2026-10-20T12:00:00.000Z')",
    )
    .bind(safe_display)
    .bind(ALLOW_ONCE_AND_DENY)
    .execute(pool)
    .await
    .unwrap();
}

fn resolve(kind: ApprovalDecisionKind, option: &str) -> ResolveChatApprovalCommand {
    ResolveChatApprovalCommand {
        command: ChatCommandContext {
            client_command_id: ChatCommandId::new("resolve-remember").unwrap(),
            expected_thread_revision: None,
        },
        thread_id: ChatThreadId::new("thread-1").unwrap(),
        request_id: ChatRequestId::new("request:remember").unwrap(),
        provider_request_id: ProviderRequestId::new("native-remember").unwrap(),
        decision: ApprovalDecision {
            kind,
            provider_option_id: Some(option.to_string()),
            updated_tool_input: None,
        },
        remember: true,
    }
}

async fn remembered_rules(pool: &sqlx::SqlitePool) -> Vec<(String, String, String)> {
    sqlx::query_as(
        "SELECT subject_kind, subject_key, approved_by_participant_id
         FROM chat_remembered_approvals WHERE revoked_at IS NULL ORDER BY subject_key",
    )
    .fetch_all(pool)
    .await
    .unwrap()
}

#[tokio::test]
async fn only_channel_runs_offering_allow_once_offer_a_rule() {
    let thread = ChatThreadId::new("thread-1").unwrap();
    let web = display("url", "https://docs.example.org/guide?page=2");

    let direct = pool_with_thread().await;
    assert_eq!(
        offered_rule(&direct, &thread, &web, ALLOW_ONCE_AND_DENY)
            .await
            .unwrap(),
        None,
        "a direct thread is not a channel run"
    );

    let channel = crate::chat::revocation::tests::pool_with_folder_runs().await;
    assert_eq!(
        offered_rule(&channel, &thread, &web, ALLOW_ONCE_AND_DENY)
            .await
            .unwrap(),
        Some(ChatRememberableApprovalRead {
            kind: ChatRememberedApprovalKind::UrlOrigin,
            key: "https://docs.example.org".to_string(),
        })
    );
    assert_eq!(
        offered_rule(&channel, &thread, &web, DENY_ONLY)
            .await
            .unwrap(),
        None,
        "no allow once choice"
    );
    for (kind, text) in [
        ("url", "http://192.168.1.10/admin"),
        ("url", "http://printer.local/"),
        ("command", "pnpm test && curl https://example.org"),
        ("path", "src/main.rs"),
    ] {
        assert_eq!(
            offered_rule(&channel, &thread, &display(kind, text), ALLOW_ONCE_AND_DENY)
                .await
                .unwrap(),
            None,
            "{kind}: {text}"
        );
    }
}

fn receipt(accepted: bool) -> DriverOperationReceipt {
    DriverOperationReceipt {
        accepted,
        operation_id: "resolve-remember".to_string(),
        detail: None,
    }
}

#[tokio::test]
async fn remembering_records_the_derived_rule_for_a_person_only_with_allow_once() {
    let pool = crate::chat::revocation::tests::pool_with_folder_runs().await;
    open_approval(&pool, &display("command", "pnpm test")).await;

    let denied = prepare_choice(&pool, &resolve(ApprovalDecisionKind::Deny, "deny"))
        .await
        .unwrap_err();
    assert_eq!(denied.code, ChatErrorCode::Validation);
    assert!(remembered_rules(&pool).await.is_empty());

    let choice = prepare_choice(&pool, &resolve(ApprovalDecisionKind::AllowOnce, "allow"))
        .await
        .unwrap();
    assert!(
        remembered_rules(&pool).await.is_empty(),
        "checking a choice records nothing"
    );
    resolve_then_remember(&pool, Some(choice), async { Ok(receipt(true)) })
        .await
        .unwrap();
    assert_eq!(
        remembered_rules(&pool).await,
        vec![(
            "command".to_string(),
            "pnpm test".to_string(),
            "participant:local-owner".to_string()
        )]
    );
}

#[tokio::test]
async fn no_rule_is_remembered_for_an_approval_the_provider_did_not_accept() {
    let pool = crate::chat::revocation::tests::pool_with_folder_runs().await;
    open_approval(&pool, &display("command", "pnpm test")).await;
    let allow = resolve(ApprovalDecisionKind::AllowOnce, "allow");

    let choice = prepare_choice(&pool, &allow).await.unwrap();
    let error = resolve_then_remember(&pool, Some(choice), async {
        Err(ChatError::new(
            ChatErrorCode::Protocol,
            "The provider refused the choice",
            true,
        ))
    })
    .await
    .unwrap_err();
    assert_eq!(error.code, ChatErrorCode::Protocol);
    assert!(remembered_rules(&pool).await.is_empty(), "provider error");

    let choice = prepare_choice(&pool, &allow).await.unwrap();
    let declined = resolve_then_remember(&pool, Some(choice), async { Ok(receipt(false)) })
        .await
        .unwrap();
    assert!(!declined.accepted);
    assert!(remembered_rules(&pool).await.is_empty(), "declined receipt");
}

#[tokio::test]
async fn a_rule_that_cannot_be_recorded_after_the_provider_accepted_reports_the_sent_approval() {
    let pool = crate::chat::revocation::tests::pool_with_folder_runs().await;
    open_approval(&pool, &display("command", "pnpm test")).await;
    let choice = prepare_choice(&pool, &resolve(ApprovalDecisionKind::AllowOnce, "allow"))
        .await
        .unwrap();
    sqlx::query("DROP TABLE chat_remembered_approvals")
        .execute(&pool)
        .await
        .unwrap();

    let error = resolve_then_remember(&pool, Some(choice), async { Ok(receipt(true)) })
        .await
        .unwrap_err();
    assert_eq!(error.code, ChatErrorCode::Persistence);
    assert!(error.message.starts_with("The approval was sent"));
}

#[tokio::test]
async fn remembering_is_refused_without_a_rule_or_a_channel_run() {
    let channel = crate::chat::revocation::tests::pool_with_folder_runs().await;
    open_approval(&channel, &display("path", "src/main.rs")).await;
    let error = prepare_choice(&channel, &resolve(ApprovalDecisionKind::AllowOnce, "allow"))
        .await
        .unwrap_err();
    assert_eq!(error.code, ChatErrorCode::Validation);

    let direct = pool_with_thread().await;
    open_approval(&direct, &display("command", "pnpm test")).await;
    let error = prepare_choice(&direct, &resolve(ApprovalDecisionKind::AllowOnce, "allow"))
        .await
        .unwrap_err();
    assert_eq!(error.code, ChatErrorCode::Validation);
    assert!(remembered_rules(&direct).await.is_empty());

    let stale = pool_with_thread().await;
    let error = prepare_choice(&stale, &resolve(ApprovalDecisionKind::AllowOnce, "allow"))
        .await
        .unwrap_err();
    assert_eq!(error.code, ChatErrorCode::Conflict);
}
