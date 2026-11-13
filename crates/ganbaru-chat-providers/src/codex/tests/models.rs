use super::*;

#[test]
fn custom_model_keeps_its_optional_display_label() {
    let model = custom_provider_model("exact-model-id", Some("Team model")).unwrap();

    assert_eq!(model.id.as_str(), "exact-model-id");
    assert_eq!(model.display_name, "Team model");
    assert!(model.custom);
    assert_eq!(model.availability, ModelAvailability::Unknown);
}

#[test]
fn safety_modes_map_to_exact_codex_policies() {
    assert_eq!(
        safety_settings(SafetyMode::AskForApproval),
        Some(CodexSafetySettings {
            approval_policy: "on-request",
            approvals_reviewer: "user",
            sandbox: "workspace-write",
            turn_sandbox_type: "workspaceWrite",
        })
    );
    assert_eq!(
        safety_settings(SafetyMode::ApproveForMe),
        Some(CodexSafetySettings {
            approval_policy: "on-request",
            approvals_reviewer: "auto_review",
            sandbox: "workspace-write",
            turn_sandbox_type: "workspaceWrite",
        })
    );
    assert_eq!(
        safety_settings(SafetyMode::FullAccess),
        Some(CodexSafetySettings {
            approval_policy: "never",
            approvals_reviewer: "user",
            sandbox: "danger-full-access",
            turn_sandbox_type: "dangerFullAccess",
        })
    );
    assert_eq!(safety_settings(SafetyMode::Custom), None);

    let custom = thread_open_params(
        None,
        Path::new("/workspace"),
        modes(SafetyMode::Custom, InteractionMode::Build),
        None,
        None,
        None,
    )
    .unwrap();
    assert!(custom.get("approvalPolicy").is_none());
    assert!(custom.get("approvalsReviewer").is_none());
    assert!(custom.get("sandbox").is_none());
}

fn organizational_turn_request(safety_mode: &str) -> SendTurnRequest {
    serde_json::from_value(json!({
        "command": { "clientCommandId": "command-1", "expectedThreadRevision": 3 },
        "sessionId": "session-1",
        "turnId": "chat-turn-1",
        "prompt": "Implement the change",
        "attachments": [],
        "mentions": [],
        "modelId": "gpt-5.4",
        "modelOptions": [],
        "modes": { "safetyMode": safety_mode, "interactionMode": "build" }
    }))
    .unwrap()
}

fn folder(capability: ProviderFolderCapability) -> ProviderOrganizationalScope {
    ProviderOrganizationalScope::Folder { capability }
}

const ALL_ACCESSES: [CodexOrganizationalAccess; 3] = [
    CodexOrganizationalAccess::Read,
    CodexOrganizationalAccess::Edit,
    CodexOrganizationalAccess::Commands,
];

#[test]
fn organizational_scopes_map_to_the_narrowest_codex_access() {
    assert_eq!(
        CodexOrganizationalAccess::for_scope(ProviderOrganizationalScope::Conversation),
        CodexOrganizationalAccess::Read
    );
    assert_eq!(
        CodexOrganizationalAccess::for_scope(folder(ProviderFolderCapability::Read)),
        CodexOrganizationalAccess::Read
    );
    assert_eq!(
        CodexOrganizationalAccess::for_scope(folder(ProviderFolderCapability::Edit)),
        CodexOrganizationalAccess::Edit
    );
    for scope in [
        folder(ProviderFolderCapability::Execute),
        folder(ProviderFolderCapability::Publish),
        ProviderOrganizationalScope::Scratch,
    ] {
        assert_eq!(
            CodexOrganizationalAccess::for_scope(scope),
            CodexOrganizationalAccess::Commands
        );
    }
    assert_eq!(
        CodexOrganizationalAccess::Read.permission_profile(),
        ORGANIZATIONAL_READ_PERMISSION_PROFILE
    );
    assert_eq!(
        CodexOrganizationalAccess::Edit.permission_profile(),
        ORGANIZATIONAL_PERMISSION_PROFILE
    );
    assert_eq!(
        CodexOrganizationalAccess::Commands.permission_profile(),
        ORGANIZATIONAL_PERMISSION_PROFILE
    );
}

#[test]
fn organizational_launch_access_fails_closed_without_a_scope() {
    let server = |organizational_authority, organizational_scope| ProviderInternalMcpConfig {
        name: "ganbaru-chat".to_string(),
        url: "http://127.0.0.1:41827/mcp".to_string(),
        bearer_token: "token".to_string(),
        organizational_authority,
        pre_approved_tools: Vec::new(),
        organizational_scope,
    };

    assert_eq!(CodexOrganizationalAccess::for_launch(None).unwrap(), None);
    assert_eq!(
        CodexOrganizationalAccess::for_launch(Some(&server(false, None))).unwrap(),
        None
    );
    assert_eq!(
        CodexOrganizationalAccess::for_launch(Some(&server(
            true,
            Some(folder(ProviderFolderCapability::Edit))
        )))
        .unwrap(),
        Some(CodexOrganizationalAccess::Edit)
    );
    let error = CodexOrganizationalAccess::for_launch(Some(&server(true, None))).unwrap_err();
    assert_eq!(error.code, ChatErrorCode::Permission);
}

#[test]
fn organizational_requests_name_the_scope_profile_without_legacy_sandbox_fields() {
    let workspace = Path::new("/workspace");
    let request = organizational_turn_request("ask_for_approval");

    for access in ALL_ACCESSES {
        let thread = thread_open_params(
            None,
            workspace,
            request.modes,
            request.model_id.as_ref(),
            None,
            Some(access),
        )
        .unwrap();
        let turn = turn_start_params(
            "provider-thread-1",
            workspace,
            "fallback-model",
            &request,
            None,
            Some(access),
            false,
        )
        .unwrap();

        for params in [&thread, &turn] {
            assert_eq!(params["permissions"], access.permission_profile());
            assert_eq!(params["runtimeWorkspaceRoots"], json!(["/workspace"]));
            assert_eq!(params["approvalPolicy"], "on-request");
            assert_eq!(params["approvalsReviewer"], "user");
            assert!(params.get("sandbox").is_none());
            assert!(params.get("sandboxPolicy").is_none());
        }
    }
}

#[test]
fn organizational_safety_never_reaches_the_automatic_reviewer() {
    let ask = organizational_safety(SafetyMode::AskForApproval).unwrap();
    assert_eq!(
        (ask.approval_policy, ask.approvals_reviewer),
        ("on-request", "user")
    );
    for mode in [SafetyMode::ApproveForMe, SafetyMode::FullAccess] {
        let safety = organizational_safety(mode).unwrap();
        assert_eq!(
            (safety.approval_policy, safety.approvals_reviewer),
            ("never", "user")
        );
    }
    assert_eq!(
        organizational_safety(SafetyMode::Custom).unwrap_err().code,
        ChatErrorCode::Permission
    );

    let request = organizational_turn_request("approve_for_me");
    let turn = turn_start_params(
        "provider-thread-1",
        Path::new("/workspace"),
        "fallback-model",
        &request,
        None,
        Some(CodexOrganizationalAccess::Edit),
        false,
    )
    .unwrap();
    assert_eq!(turn["approvalPolicy"], "never");
    assert_eq!(turn["approvalsReviewer"], "user");
}

#[test]
fn organizational_launch_arguments_replace_authority_and_disable_inherited_mcp() {
    let mut arguments = Vec::new();
    append_organizational_base_arguments(&mut arguments, CodexOrganizationalAccess::Commands);
    append_disabled_mcp_arguments(
        &mut arguments,
        &["user-server".to_string(), "ganbaru-chat".to_string()],
        "ganbaru-chat",
    )
    .unwrap();
    append_internal_mcp_arguments(&mut arguments, "ganbaru-chat", "http://127.0.0.1:41827/mcp")
        .unwrap();

    assert!(arguments.iter().any(|argument| {
        argument.starts_with("permissions.ganbaru_organizational=")
            && argument.contains("filesystem")
            && argument.contains("network={enabled=false}")
    }));
    assert!(
        arguments
            .iter()
            .any(|argument| argument == "mcp_servers.user-server.enabled=false")
    );
    assert!(
        arguments
            .iter()
            .any(|argument| argument == "mcp_servers.ganbaru-chat.required=true")
    );
    assert!(
        arguments
            .iter()
            .all(|argument| !argument.contains("sandbox_mode"))
    );
    assert!(
        append_internal_mcp_arguments(&mut Vec::new(), "ganbaru-chat", "https://example.test/mcp",)
            .is_err()
    );
}

#[test]
fn organizational_launch_arguments_follow_the_scope_access() {
    let arguments = |access| {
        let mut arguments = Vec::new();
        append_organizational_base_arguments(&mut arguments, access);
        arguments
    };
    let has = |arguments: &[String], expected: &str| {
        arguments.iter().any(|argument| argument == expected)
    };

    let read = arguments(CodexOrganizationalAccess::Read);
    assert!(has(
        &read,
        r#"permissions.ganbaru_organizational_read={filesystem={":minimal"="read",":workspace_roots"={"."="read"}},network={enabled=false}}"#
    ));
    assert!(has(
        &read,
        "default_permissions=\"ganbaru_organizational_read\""
    ));
    let edit = arguments(CodexOrganizationalAccess::Edit);
    assert!(has(
        &edit,
        r#"permissions.ganbaru_organizational={filesystem={":minimal"="read",":workspace_roots"={"."="write"}},network={enabled=false}}"#
    ));
    assert!(has(&edit, "default_permissions=\"ganbaru_organizational\""));
    for arguments in [&read, &edit] {
        assert!(has(arguments, "features.shell_tool=false"));
        assert!(has(arguments, "features.unified_exec=false"));
    }

    let commands = arguments(CodexOrganizationalAccess::Commands);
    assert!(!commands.iter().any(|argument| {
        argument.starts_with("features.shell_tool") || argument.starts_with("features.unified_exec")
    }));
    for arguments in [&read, &edit, &commands] {
        assert!(has(arguments, "features.view_image=false"));
        assert!(has(arguments, "features.code_mode.enabled=false"));
        assert!(has(arguments, "features.multi_agent=false"));
        assert!(has(arguments, "web_search=\"disabled\""));
        assert!(has(arguments, "allow_login_shell=false"));
        assert!(
            !arguments
                .iter()
                .any(|argument| argument.starts_with("features.code_mode_host"))
        );
    }
}

#[test]
fn organizational_verification_rejects_profile_and_root_mismatches() {
    let response = |profile: &str, root: &str, policy: &str| ThreadOpenResponse {
        thread: CodexThread {
            id: "provider-thread-1".to_string(),
        },
        model: "gpt-5.4".to_string(),
        approval_policy: json!(policy),
        approvals_reviewer: "user".to_string(),
        sandbox: json!({ "type": "workspaceWrite" }),
        active_permission_profile: Some(CodexActivePermissionProfile {
            id: profile.to_string(),
            extends: None,
        }),
        runtime_workspace_roots: vec![PathBuf::from(root)],
    };
    let verify = |mode, response: &ThreadOpenResponse, access| {
        verify_organizational_thread_open(mode, Path::new("/workspace"), response, access)
    };
    let ask = SafetyMode::AskForApproval;
    let read = CodexOrganizationalAccess::Read;
    let edit = CodexOrganizationalAccess::Edit;

    assert!(
        verify(
            ask,
            &response(
                ORGANIZATIONAL_PERMISSION_PROFILE,
                "/workspace",
                "on-request"
            ),
            edit
        )
        .is_ok()
    );
    assert!(
        verify(
            ask,
            &response(
                ORGANIZATIONAL_READ_PERMISSION_PROFILE,
                "/workspace",
                "on-request"
            ),
            read
        )
        .is_ok()
    );
    assert!(
        verify(
            SafetyMode::ApproveForMe,
            &response(ORGANIZATIONAL_PERMISSION_PROFILE, "/workspace", "never"),
            edit
        )
        .is_ok()
    );
    assert!(
        verify(
            SafetyMode::ApproveForMe,
            &response(
                ORGANIZATIONAL_PERMISSION_PROFILE,
                "/workspace",
                "on-request"
            ),
            edit
        )
        .is_err()
    );
    assert!(
        verify(
            ask,
            &response(
                ORGANIZATIONAL_PERMISSION_PROFILE,
                "/workspace",
                "on-request"
            ),
            read
        )
        .is_err()
    );
    assert!(
        verify(
            ask,
            &response("user-profile", "/workspace", "on-request"),
            edit
        )
        .is_err()
    );
    assert!(
        verify(
            ask,
            &response(ORGANIZATIONAL_PERMISSION_PROFILE, "/other", "on-request"),
            edit
        )
        .is_err()
    );
}

fn verify_config(config: Value, access: CodexOrganizationalAccess) -> ChatResult<()> {
    verify_organizational_effective_config(
        ConfigReadResponse { config },
        Some(("ganbaru-chat", "http://127.0.0.1:41827/mcp")),
        access,
    )
}

#[test]
fn organizational_config_requires_one_enabled_internal_mcp_server() {
    for access in ALL_ACCESSES {
        assert!(verify_config(organizational_config(access), access).is_ok());
        let mut extra = organizational_config(access);
        extra["mcp_servers"]["openaiDeveloperDocs"]["enabled"] = json!(true);
        assert!(verify_config(extra, access).is_err());
    }
}

#[test]
fn organizational_config_rejects_a_shell_the_scope_does_not_grant() {
    for access in [
        CodexOrganizationalAccess::Read,
        CodexOrganizationalAccess::Edit,
    ] {
        for feature in ["shell_tool", "unified_exec"] {
            let mut config = organizational_config(access);
            config["features"][feature] = json!(true);
            assert!(verify_config(config, access).is_err(), "{feature}");
            let mut missing = organizational_config(access);
            missing["features"].as_object_mut().unwrap().remove(feature);
            assert!(verify_config(missing, access).is_err(), "{feature}");
        }
    }
    let mut image = organizational_config(CodexOrganizationalAccess::Commands);
    image["features"]["view_image"] = json!(true);
    assert!(verify_config(image, CodexOrganizationalAccess::Commands).is_err());
}

#[test]
fn organizational_config_rejects_root_access_wider_than_the_scope() {
    let read = CodexOrganizationalAccess::Read;
    let mut writable = organizational_config(read);
    writable["permissions"][ORGANIZATIONAL_READ_PERMISSION_PROFILE]["filesystem"][":workspace_roots"]
        ["."] = json!("write");
    assert!(verify_config(writable, read).is_err());

    let mut wrong_default = organizational_config(read);
    wrong_default["default_permissions"] = json!(ORGANIZATIONAL_PERMISSION_PROFILE);
    assert!(verify_config(wrong_default, read).is_err());

    let edit_config = organizational_config(CodexOrganizationalAccess::Edit);
    assert!(verify_config(edit_config, read).is_err());

    // A trusted project config merges its own filesystem rules into the launch profile.
    let edit = CodexOrganizationalAccess::Edit;
    let mut merged_root = organizational_config(edit);
    merged_root["permissions"][ORGANIZATIONAL_PERMISSION_PROFILE]["filesystem"][":root"] =
        json!("write");
    assert!(verify_config(merged_root, edit).is_err());

    let mut network = organizational_config(edit);
    network["permissions"][ORGANIZATIONAL_PERMISSION_PROFILE]["network"]["enabled"] = json!(true);
    assert!(verify_config(network, edit).is_err());
}

#[test]
fn organizational_profile_list_requires_the_scope_profile() {
    let list = |ids: &[&str]| PermissionProfileListResponse {
        data: ids
            .iter()
            .map(|id| PermissionProfileSummary {
                id: id.to_string(),
                allowed: true,
            })
            .collect(),
        next_cursor: None,
    };
    let read = CodexOrganizationalAccess::Read;

    assert!(
        verify_permission_profile_list(&list(&[ORGANIZATIONAL_READ_PERMISSION_PROFILE]), read)
            .is_ok()
    );
    assert!(
        verify_permission_profile_list(&list(&[ORGANIZATIONAL_PERMISSION_PROFILE]), read).is_err()
    );
    assert!(
        verify_permission_profile_list(
            &list(&[ORGANIZATIONAL_READ_PERMISSION_PROFILE]),
            CodexOrganizationalAccess::Commands
        )
        .is_err()
    );
}

#[test]
fn organizational_sessions_decline_every_native_approval() {
    for method in [
        "item/fileChange/requestApproval",
        "item/commandExecution/requestApproval",
    ] {
        assert_eq!(
            organizational_approval_denial(method),
            Some(json!({ "decision": "decline" }))
        );
    }
    assert_eq!(
        organizational_approval_denial("item/permissions/requestApproval"),
        Some(json!({ "permissions": {}, "scope": "turn" }))
    );
    assert_eq!(
        organizational_approval_denial("item/tool/requestUserInput"),
        None
    );
    assert_eq!(
        organizational_approval_denial("mcpServer/elicitation/request"),
        None
    );
}

#[test]
fn organizational_authority_support_claims_only_enforced_boundaries() {
    let support = organizational_authority_support();
    assert!(support.isolated_conversation);
    assert!(support.internal_host_tools);
    assert!(support.deny_shell);
    assert!(support.read_only_root);
    assert!(support.writable_root);
    assert!(support.confined_commands);
    assert!(support.network_boundary);
    assert!(!support.person_approved_commands);
    assert!(!support.classified_publish);
}

#[test]
fn organizational_mcp_status_ignores_disabled_inherited_servers() {
    let status = |user_server_enabled| McpStatusRead {
        servers: vec![
            McpServerStatusRead {
                name: "user-server".to_string(),
                auth_status: None,
                enabled: user_server_enabled,
                runtime_status: None,
            },
            McpServerStatusRead {
                name: "ganbaru-chat".to_string(),
                auth_status: Some("bearerToken".to_string()),
                enabled: true,
                runtime_status: Some("ready".to_string()),
            },
        ],
    };

    assert!(verify_organizational_mcp_status(&status(false), "ganbaru-chat").is_ok());
    assert!(verify_organizational_mcp_status(&status(true), "ganbaru-chat").is_err());
}

#[test]
fn turn_builder_preserves_model_traits_modes_and_verified_images() {
    let workspace = TestDirectory::new("turn-builder");
    fs::write(workspace.path().join("prompt.png"), b"redacted image").unwrap();
    let request: SendTurnRequest = serde_json::from_value(json!({
        "command": { "clientCommandId": "command-1", "expectedThreadRevision": 3 },
        "sessionId": "session-1",
        "turnId": "chat-turn-1",
        "prompt": "Implement the change",
        "attachments": [{
            "attachmentId": "attachment-1",
            "kind": "image",
            "displayName": "prompt.png",
            "managedRelativePath": "prompt.png",
            "resourceUri": "ganbaru://chat/resource/attachment-1",
            "mimeType": "image/png",
            "byteSize": 14,
            "localPath": workspace.path().join("prompt.png").to_string_lossy(),
            "textContent": null
        }],
        "mentions": [],
        "modelId": "gpt-5.4",
        "modelOptions": [
            { "key": "reasoning_effort", "value": { "kind": "choice", "value": "high" } },
            { "key": "service_tier", "value": { "kind": "choice", "value": "fast" } }
        ],
        "modes": { "safetyMode": "approve_for_me", "interactionMode": "plan" },
        "developerInstructions": "Use the repository conventions."
    }))
    .unwrap();
    let params = turn_start_params(
        "provider-thread-1",
        workspace.path(),
        "fallback-model",
        &request,
        None,
        None,
        false,
    )
    .unwrap();

    assert_eq!(params["approvalPolicy"], "on-request");
    assert_eq!(params["approvalsReviewer"], "auto_review");
    assert_eq!(params["sandboxPolicy"]["type"], "workspaceWrite");
    assert_eq!(params["sandboxPolicy"]["writableRoots"], json!([]));
    assert_eq!(params["sandboxPolicy"]["networkAccess"], false);
    assert_eq!(params["model"], "gpt-5.4");
    assert_eq!(params["effort"], "high");
    assert_eq!(params["serviceTier"], "fast");
    assert_eq!(params["collaborationMode"]["mode"], "plan");
    assert_eq!(
        params["collaborationMode"]["settings"]["developer_instructions"],
        "Use the repository conventions."
    );
    assert_eq!(params["input"][0]["type"], "text");
    assert_eq!(params["input"][1]["type"], "localImage");
    assert_eq!(
        params["input"][1]["path"],
        workspace
            .path()
            .join("prompt.png")
            .to_string_lossy()
            .as_ref()
    );

    let mut standard_request = request.clone();
    standard_request.model_options[1].value = ModelOptionValue::Choice("standard".to_string());
    let standard_params = turn_start_params(
        "provider-thread-1",
        workspace.path(),
        "fallback-model",
        &standard_request,
        None,
        None,
        false,
    )
    .unwrap();
    assert!(standard_params.get("serviceTier").is_none());
    assert!(params["input"][1].get("url").is_none());

    let mut custom_request = request.clone();
    custom_request.modes.safety_mode = SafetyMode::Custom;
    let custom_safety = CodexCustomSafetySettings {
        approval_policy: json!("on-request"),
        approvals_reviewer: "user".to_string(),
        sandbox_policy: None,
        permissions: Some("project-edit".to_string()),
    };
    let custom_params = turn_start_params(
        "provider-thread-1",
        workspace.path(),
        "fallback-model",
        &custom_request,
        Some(&custom_safety),
        None,
        false,
    )
    .unwrap();
    assert_eq!(custom_params["permissions"], "project-edit");
    assert!(custom_params.get("sandboxPolicy").is_none());

    let mut escaping = request;
    escaping.attachments[0].local_path = Some("../outside.png".to_string());
    assert!(
        turn_start_params(
            "provider-thread-1",
            workspace.path(),
            "fallback-model",
            &escaping,
            None,
            None,
            false,
        )
        .is_err()
    );
}

#[test]
fn custom_permissions_resolve_profiles_and_legacy_sandbox_settings() {
    let profile = custom_safety_settings(ConfigReadResponse {
        config: json!({
            "approval_policy": "on-request",
            "approvals_reviewer": "auto_review",
            "default_permissions": "project-edit"
        }),
    })
    .unwrap();
    assert_eq!(profile.permissions.as_deref(), Some("project-edit"));
    assert_eq!(profile.sandbox_policy, None);

    let writable_root = TestDirectory::new("custom-writable-root");
    let writable_root_value = writable_root.path().to_string_lossy().into_owned();
    let legacy = custom_safety_settings(ConfigReadResponse {
        config: json!({
            "approval_policy": "untrusted",
            "approvals_reviewer": "user",
            "sandbox_mode": "workspace-write",
            "sandbox_workspace_write": {
                "writable_roots": [writable_root_value],
                "network_access": true,
                "exclude_tmpdir_env_var": true,
                "exclude_slash_tmp": false
            }
        }),
    })
    .unwrap();
    assert_eq!(legacy.permissions, None);
    assert_eq!(
        legacy.sandbox_policy.as_ref().unwrap()["type"],
        "workspaceWrite"
    );
    assert_eq!(
        legacy.sandbox_policy.as_ref().unwrap()["networkAccess"],
        true
    );
    assert_eq!(
        legacy.sandbox_policy.as_ref().unwrap()["writableRoots"],
        json!([writable_root.path().to_string_lossy()])
    );
}

#[test]
fn model_catalog_keeps_options_and_image_capability() {
    let model: CodexModel = serde_json::from_value(json!({
        "id": "gpt-5.4",
        "model": "gpt-5.4",
        "displayName": "GPT-5.4",
        "description": "Current coding model",
        "hidden": false,
        "isDefault": true,
        "defaultReasoningEffort": "medium",
        "supportedReasoningEfforts": [
            { "reasoningEffort": "medium", "description": "Balanced" },
            { "reasoningEffort": "high", "description": "Deeper" }
        ],
        "inputModalities": ["text", "image"],
        "serviceTiers": [{ "id": "fast", "name": "Fast", "description": "Low latency" }],
        "defaultServiceTier": "fast",
        "supportsPersonality": true,
        "upgrade": null
    }))
    .unwrap();
    let model = provider_model(model).unwrap();

    assert_eq!(model.id.as_str(), "gpt-5.4");
    assert!(model.capabilities.contains(&ProviderCapability::Images));
    assert!(model.capabilities.contains(&ProviderCapability::NativePlan));
    assert!(
        model
            .capabilities
            .contains(&ProviderCapability::StructuredPlans)
    );
    assert_eq!(model.options.len(), 2);
    let ModelOptionDefinition::Choice {
        options,
        default_value,
        ..
    } = &model.options[1]
    else {
        panic!("service tier must be a choice option");
    };
    assert_eq!(
        options
            .iter()
            .map(|option| option.value.as_str())
            .collect::<Vec<_>>(),
        ["standard", "fast"]
    );
    assert_eq!(default_value.as_deref(), Some("fast"));
}

#[test]
fn turn_network_access_follows_the_codex_sandbox_policy() {
    assert_eq!(
        preset_network_access(SafetyMode::AskForApproval),
        Some(false)
    );
    assert_eq!(preset_network_access(SafetyMode::ApproveForMe), Some(false));
    assert_eq!(preset_network_access(SafetyMode::FullAccess), Some(true));
    assert_eq!(preset_network_access(SafetyMode::Custom), None);

    assert!(sandbox_network_access(
        &json!({ "sandboxPolicy": { "type": "dangerFullAccess" } })
    ));
    assert!(!sandbox_network_access(
        &json!({ "sandboxPolicy": { "type": "workspaceWrite", "networkAccess": false } })
    ));
    assert!(sandbox_network_access(
        &json!({ "sandboxPolicy": { "type": "workspaceWrite", "networkAccess": true } })
    ));
    assert!(!sandbox_network_access(
        &json!({ "sandboxPolicy": { "type": "readOnly" } })
    ));
    assert!(!sandbox_network_access(
        &json!({ "permissions": "project-edit" })
    ));
}
