use ganbaru_chat_contracts::config::{
    CHAT_VAULT_CONFIG_SCHEMA_VERSION, ChatVaultConfig, parse_chat_config_branch,
    replace_chat_config_branch,
};
use ganbaru_chat_contracts::models::ChatErrorCode;
use serde_json::json;

fn valid_config() -> serde_json::Value {
    json!({
        "schemaVersion": 1,
        "providers": [{
            "schemaVersion": 1,
            "instanceId": "codex-personal",
            "familyId": "codex",
            "label": "Personal Codex",
            "enabled": true,
            "launchArguments": [],
            "environment": { "RUST_LOG": "warn" },
            "credentialReferences": { "API_TOKEN": "credential-1" },
            "visibleModelIds": ["gpt-5-codex"],
            "favoriteModelIds": ["gpt-5-codex"],
            "providerConfig": { "schemaVersion": 1, "value": {} },
            "futurePortableField": { "preserved": true }
        }],
        "automaticProviderSetupDisabled": [],
        "rememberedSelections": [{
            "workingFolderId": "working-folder-1",
            "providerInstanceId": "codex-personal",
            "modelId": "gpt-5-codex",
            "providerManagedModel": false,
            "modelOptions": [],
            "safetyMode": "ask_for_approval",
            "interactionMode": "build"
        }],
        "workingFolderProviderPreferences": {},
        "panels": { "inspectorWidthPx": 360 },
        "behavior": {
            "sendKey": "enter",
            "restoreLastSelectedThread": true,
            "showReasoningSummaries": true,
            "automaticallyFoldSettledWork": true,
            "terminalScrollbackLines": 10000,
            "idleSessionTimeoutSeconds": 900,
            "confirmMultilineTerminalPaste": true
        },
        "futureChatField": [1, 2, 3]
    })
}

#[test]
fn missing_chat_branch_uses_safe_defaults() {
    let config = parse_chat_config_branch(&json!({ "theme": { "activeId": "dark" } })).unwrap();

    assert_eq!(config.schema_version, CHAT_VAULT_CONFIG_SCHEMA_VERSION);
    assert!(config.providers.is_empty());
    assert_eq!(config.panels.inspector_width_px, 520);
    assert!(config.behavior.confirm_multiline_terminal_paste);
}

#[test]
fn existing_chat_branch_requires_the_current_complete_shape() {
    let result = parse_chat_config_branch(&json!({
        "chat": { "schemaVersion": CHAT_VAULT_CONFIG_SCHEMA_VERSION }
    }));

    assert!(result.is_err());

    let mut missing_nullable_model = valid_config();
    missing_nullable_model["rememberedSelections"][0]
        .as_object_mut()
        .unwrap()
        .remove("modelId");
    assert!(parse_chat_config_branch(&json!({ "chat": missing_nullable_model })).is_err());
}

#[test]
fn valid_chat_branch_preserves_unknown_portable_fields() {
    let input = valid_config();
    let config = parse_chat_config_branch(&json!({ "chat": input.clone() })).unwrap();
    let serialized = serde_json::to_value(&config).unwrap();

    assert_eq!(serialized, input);
    assert_eq!(
        config.providers[0]
            .unknown_fields
            .get("futurePortableField"),
        Some(&json!({ "preserved": true }))
    );
    assert_eq!(
        config.unknown_fields.get("futureChatField"),
        Some(&json!([1, 2, 3]))
    );
}

#[test]
fn config_rejects_machine_paths_and_invalid_remembered_models() {
    let mut machine_path = valid_config();
    machine_path["providers"][0]["executable"] = json!("/usr/bin/codex");
    let error = parse_chat_config_branch(&json!({ "chat": machine_path })).unwrap_err();
    assert_eq!(error.code, ChatErrorCode::Validation);
    assert_eq!(error.field.as_deref(), Some("chat.providers[0]"));

    let mut missing_model = valid_config();
    missing_model["rememberedSelections"][0]["modelId"] = serde_json::Value::Null;
    let error = parse_chat_config_branch(&json!({ "chat": missing_model })).unwrap_err();
    assert_eq!(error.code, ChatErrorCode::Validation);
    assert_eq!(
        error.field.as_deref(),
        Some("chat.rememberedSelections[0].modelId")
    );
}

#[test]
fn config_rejects_unknown_schema_and_out_of_range_behavior() {
    let mut unknown_schema = valid_config();
    unknown_schema["schemaVersion"] = json!(2);
    assert!(parse_chat_config_branch(&json!({ "chat": unknown_schema })).is_err());

    let mut oversized_scrollback = valid_config();
    oversized_scrollback["behavior"]["terminalScrollbackLines"] = json!(100_001);
    let error = parse_chat_config_branch(&json!({ "chat": oversized_scrollback })).unwrap_err();
    assert_eq!(error.code, ChatErrorCode::Validation);
    assert_eq!(
        error.field.as_deref(),
        Some("chat.behavior.terminalScrollbackLines")
    );
}

#[test]
fn replacing_chat_branch_preserves_unrelated_vault_config() {
    let config: ChatVaultConfig =
        parse_chat_config_branch(&json!({ "chat": valid_config() })).unwrap();
    let mut root = json!({ "theme": { "activeId": "dark" }, "preferences": { "fontScale": 1.1 } });

    replace_chat_config_branch(&mut root, config).unwrap();

    assert_eq!(root["theme"]["activeId"], json!("dark"));
    assert_eq!(root["preferences"]["fontScale"], json!(1.1));
    assert_eq!(root["chat"]["schemaVersion"], json!(1));
}
