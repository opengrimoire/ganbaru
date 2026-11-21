//! Organizational target enforcement: the authorized target mode, provider scope boundaries, and
//! person approved commands.

use super::*;

#[tokio::test]
async fn organizational_sends_require_the_exact_authorized_target_mode_and_enforcement() {
    use super::super::coordinator::validate_organizational_authority;
    let pool = crate::chat::revocation::tests::pool_with_folder_runs().await;
    let binding = working_run_binding(&pool).await;
    let full_support = ProviderAuthoritySupport {
        isolated_conversation: true,
        internal_host_tools: true,
        deny_shell: true,
        read_only_root: true,
        writable_root: true,
        confined_commands: true,
        network_boundary: true,
        classified_publish: true,
        person_approved_commands: false,
    };
    let scope = validate_organizational_authority(
        &pool,
        &binding,
        full_support,
        SafetyMode::AskForApproval,
    )
    .await
    .expect("the authorized target, mode, and enforcement are accepted");
    assert_eq!(
        scope,
        ProviderOrganizationalScope::Folder {
            capability: ProviderFolderCapability::Execute
        }
    );

    let other_folder = ganbaru_chat::agent_runs::AgentRunBinding {
        working_folder_id: Some("workspace-2".to_string()),
        ..binding.clone()
    };
    let other_digest = ganbaru_chat::agent_runs::AgentRunBinding {
        authorization_scope_digest: format!("{:064}", 1),
        ..binding.clone()
    };
    let rejected = [
        (&binding, full_support, SafetyMode::FullAccess),
        (&other_folder, full_support, SafetyMode::AskForApproval),
        (&other_digest, full_support, SafetyMode::AskForApproval),
        (
            &binding,
            ProviderAuthoritySupport {
                confined_commands: false,
                ..full_support
            },
            SafetyMode::AskForApproval,
        ),
        (
            &binding,
            ProviderAuthoritySupport {
                internal_host_tools: false,
                ..full_support
            },
            SafetyMode::AskForApproval,
        ),
    ];
    for (candidate, support, mode) in rejected {
        let error = validate_organizational_authority(&pool, candidate, support, mode)
            .await
            .expect_err("a send outside the authorization is refused");
        assert_eq!(error.code, ChatErrorCode::Permission, "{}", error.message);
    }

    sqlx::query(
        "UPDATE chat_assignment_authorization_revisions
         SET decision_state = 'revoked', revoked_at = '2026-10-20T12:01:00.000Z'
         WHERE id = 'authorization:working'",
    )
    .execute(&pool)
    .await
    .unwrap();
    let error = validate_organizational_authority(
        &pool,
        &binding,
        full_support,
        SafetyMode::AskForApproval,
    )
    .await
    .expect_err("a revoked authorization cannot start a send");
    assert_eq!(error.code, ChatErrorCode::Permission);
}

/// Support that confines commands itself, like Codex, plus every other boundary.
const CONFINING_SUPPORT: ProviderAuthoritySupport = ProviderAuthoritySupport {
    isolated_conversation: true,
    internal_host_tools: true,
    deny_shell: true,
    read_only_root: true,
    writable_root: true,
    confined_commands: true,
    network_boundary: true,
    classified_publish: true,
    person_approved_commands: false,
};

/// Support that cannot confine commands but sends every command to the person, as Claude
/// declares it.
const APPROVING_SUPPORT: ProviderAuthoritySupport = ProviderAuthoritySupport {
    confined_commands: false,
    person_approved_commands: true,
    ..CONFINING_SUPPORT
};

#[test]
fn provider_scope_enforcement_requires_each_boundary_of_the_target() {
    use super::super::coordinator::provider_enforces_scope;
    let folder = |capability| ProviderOrganizationalScope::Folder { capability };
    let read = folder(ProviderFolderCapability::Read);
    let edit = folder(ProviderFolderCapability::Edit);
    let execute = folder(ProviderFolderCapability::Execute);
    let publish = folder(ProviderFolderCapability::Publish);
    let scratch = ProviderOrganizationalScope::Scratch;
    let conversation = ProviderOrganizationalScope::Conversation;
    let without = |change: fn(&mut ProviderAuthoritySupport)| {
        let mut support = CONFINING_SUPPORT;
        change(&mut support);
        support
    };
    for scope in [conversation, read, edit, execute, publish, scratch] {
        for ask in [true, false] {
            assert!(
                provider_enforces_scope(CONFINING_SUPPORT, scope, ask),
                "{scope:?} with every boundary"
            );
        }
    }
    let refused = [
        (without(|s| s.isolated_conversation = false), conversation),
        (without(|s| s.read_only_root = false), read),
        (without(|s| s.deny_shell = false), read),
        (without(|s| s.network_boundary = false), read),
        (without(|s| s.writable_root = false), edit),
        (without(|s| s.deny_shell = false), edit),
        (without(|s| s.network_boundary = false), edit),
        (without(|s| s.writable_root = false), execute),
        (without(|s| s.confined_commands = false), execute),
        (without(|s| s.network_boundary = false), execute),
        (without(|s| s.writable_root = false), scratch),
        (without(|s| s.confined_commands = false), scratch),
        (without(|s| s.network_boundary = false), scratch),
        (without(|s| s.classified_publish = false), publish),
        (without(|s| s.confined_commands = false), publish),
    ];
    for (support, scope) in refused {
        for ask in [true, false] {
            assert!(
                !provider_enforces_scope(support, scope, ask),
                "{scope:?} without a boundary: {support:?}"
            );
        }
    }
}

#[test]
fn person_approved_commands_confine_commands_only_under_ask() {
    use super::super::coordinator::provider_enforces_scope;
    let folder = |capability| ProviderOrganizationalScope::Folder { capability };
    for scope in [
        ProviderOrganizationalScope::Scratch,
        folder(ProviderFolderCapability::Execute),
    ] {
        assert!(
            provider_enforces_scope(APPROVING_SUPPORT, scope, true),
            "{scope:?}"
        );
        assert!(
            !provider_enforces_scope(APPROVING_SUPPORT, scope, false),
            "{scope:?}"
        );
        let without_network = ProviderAuthoritySupport {
            network_boundary: false,
            ..APPROVING_SUPPORT
        };
        assert!(
            !provider_enforces_scope(without_network, scope, true),
            "{scope:?}"
        );
    }
    for scope in [
        ProviderOrganizationalScope::Conversation,
        folder(ProviderFolderCapability::Read),
        folder(ProviderFolderCapability::Edit),
    ] {
        for ask in [true, false] {
            assert!(
                provider_enforces_scope(APPROVING_SUPPORT, scope, ask),
                "{scope:?}"
            );
        }
    }
    let publish = folder(ProviderFolderCapability::Publish);
    assert!(provider_enforces_scope(APPROVING_SUPPORT, publish, true));
    assert!(!provider_enforces_scope(APPROVING_SUPPORT, publish, false));
    assert!(!provider_enforces_scope(
        ProviderAuthoritySupport {
            classified_publish: false,
            ..APPROVING_SUPPORT
        },
        publish,
        true
    ));
    let without_approval = ProviderAuthoritySupport {
        person_approved_commands: false,
        ..APPROVING_SUPPORT
    };
    assert!(!provider_enforces_scope(
        without_approval,
        folder(ProviderFolderCapability::Execute),
        true
    ));
}

#[tokio::test]
async fn organizational_sends_admit_person_approved_commands_only_with_ask_approval() {
    use super::super::coordinator::validate_organizational_authority;
    let pool = crate::chat::revocation::tests::pool_with_folder_runs().await;
    let binding = working_run_binding(&pool).await;
    // The fixture varies one stored authorization in place instead of issuing new revisions.
    for statement in [
        "DROP TRIGGER chat_authorized_folder_source_immutable_update",
        "DROP TRIGGER chat_assignment_authorization_scope_immutable",
    ] {
        sqlx::query(statement).execute(&pool).await.unwrap();
    }
    // At own, the channel access covers every approval the cases freeze.
    sqlx::query("UPDATE chat_channels SET agent_access = 'own'")
        .execute(&pool)
        .await
        .unwrap();
    let set_target = |capability: &'static str, runtime: &'static str| {
        let pool = pool.clone();
        async move {
            sqlx::query(
                "UPDATE chat_assignment_authorized_folder_sources SET capability = ?
                 WHERE authorization_revision_id = 'authorization:working'
                   AND is_execution_target = 1",
            )
            .bind(capability)
            .execute(&pool)
            .await
            .unwrap();
            sqlx::query(
                "UPDATE chat_assignment_authorization_revisions
                 SET resolved_runtime_approval_policy = ? WHERE id = 'authorization:working'",
            )
            .bind(runtime)
            .execute(&pool)
            .await
            .unwrap();
        }
    };
    let validate =
        |support, mode| validate_organizational_authority(&pool, &binding, support, mode);

    set_target("execute", "ask").await;
    assert_eq!(
        validate(APPROVING_SUPPORT, SafetyMode::AskForApproval)
            .await
            .expect("each command reaches the person under Ask"),
        ProviderOrganizationalScope::Folder {
            capability: ProviderFolderCapability::Execute
        }
    );
    set_target("execute", "auto_approve").await;
    let error = validate(APPROVING_SUPPORT, SafetyMode::ApproveForMe)
        .await
        .expect_err("auto approval cannot stand in for confined commands");
    assert_eq!(error.code, ChatErrorCode::Permission);
    validate(CONFINING_SUPPORT, SafetyMode::ApproveForMe)
        .await
        .expect("confined commands need no person approval");

    for (capability, expected) in [
        ("read", ProviderFolderCapability::Read),
        ("edit", ProviderFolderCapability::Edit),
    ] {
        set_target(capability, "auto_approve").await;
        assert_eq!(
            validate(APPROVING_SUPPORT, SafetyMode::ApproveForMe)
                .await
                .expect("a target without commands needs no person approval"),
            ProviderOrganizationalScope::Folder {
                capability: expected
            }
        );
    }

    set_target("execute", "ask").await;
    sqlx::query(
        "UPDATE chat_assignment_authorized_folder_sources SET is_execution_target = 0
         WHERE authorization_revision_id = 'authorization:working'",
    )
    .execute(&pool)
    .await
    .unwrap();
    let error = validate(CONFINING_SUPPORT, SafetyMode::AskForApproval)
        .await
        .expect_err("a folder target needs an execution target grant");
    assert_eq!(error.code, ChatErrorCode::Permission);
}
