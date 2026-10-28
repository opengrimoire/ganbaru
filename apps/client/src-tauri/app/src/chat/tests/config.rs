use crate::chat::configuration::{behavior_from_view, behavior_patches};
use ganbaru_chat_contracts::config::{
    ChatBehaviorPreferences, ChatConfiguration, ChatDeviceConfig, ChatSendKey,
};
use ganbaru_chat_contracts::models::ChatErrorCode;
use serde_json::json;

fn valid_device_config() -> serde_json::Value {
    json!({
        "providers": [{
            "schemaVersion": 1,
            "instanceId": "codex-personal",
            "familyId": "codex",
            "label": "Personal Codex",
            "enabled": true,
            "launchArguments": ["--profile", "work"],
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
        "panels": { "inspectorWidthPx": 360 }
    })
}

#[test]
fn missing_device_entry_uses_safe_defaults() {
    let config = ChatDeviceConfig::from_stored(None).unwrap();

    assert!(config.providers.is_empty());
    assert!(config.remembered_selections.is_empty());
    assert_eq!(config.panels.inspector_width_px, 520);
}

#[test]
fn stored_device_entry_requires_the_current_complete_shape() {
    assert!(ChatDeviceConfig::from_stored(Some(&json!({}))).is_err());
    assert!(ChatDeviceConfig::from_stored(Some(&json!([]))).is_err());

    let mut missing_nullable_model = valid_device_config();
    missing_nullable_model["rememberedSelections"][0]
        .as_object_mut()
        .unwrap()
        .remove("modelId");
    assert!(ChatDeviceConfig::from_stored(Some(&missing_nullable_model)).is_err());
}

#[test]
fn device_entry_round_trips_provider_fields_and_drops_retired_top_level_fields() {
    let mut input = valid_device_config();
    input["schemaVersion"] = json!(1);
    input["behavior"] = json!({ "sendKey": "mod_enter" });
    input["futureChatField"] = json!([1, 2, 3]);
    let config = ChatDeviceConfig::from_stored(Some(&input)).unwrap();
    let stored = config.to_stored().unwrap();

    assert_eq!(stored, valid_device_config());
    assert_eq!(
        config.providers[0]
            .unknown_fields
            .get("futurePortableField"),
        Some(&json!({ "preserved": true }))
    );
    assert_eq!(
        config.providers[0].credential_references["API_TOKEN"].as_str(),
        "credential-1"
    );
    assert_eq!(
        ChatDeviceConfig::from_stored(Some(&stored)).unwrap(),
        config
    );
}

#[test]
fn device_entry_rejects_machine_paths_and_invalid_remembered_models() {
    for field in ["executable", "providerHome", "canonicalPath", "lastProbe"] {
        let mut machine_path = valid_device_config();
        machine_path["providers"][0][field] = json!("/usr/bin/codex");
        let error = ChatDeviceConfig::from_stored(Some(&machine_path)).unwrap_err();
        assert_eq!(error.code, ChatErrorCode::Validation);
        assert_eq!(error.field.as_deref(), Some("chat.providers[0]"));
    }

    let mut missing_model = valid_device_config();
    missing_model["rememberedSelections"][0]["modelId"] = serde_json::Value::Null;
    let error = ChatDeviceConfig::from_stored(Some(&missing_model)).unwrap_err();
    assert_eq!(error.code, ChatErrorCode::Validation);
    assert_eq!(
        error.field.as_deref(),
        Some("chat.rememberedSelections[0].modelId")
    );
}

#[test]
fn behavior_validation_refuses_values_outside_the_bounds() {
    let oversized = ChatBehaviorPreferences {
        terminal_scrollback_lines: 100_001,
        ..ChatBehaviorPreferences::default()
    };
    let error = oversized.validate().unwrap_err();
    assert_eq!(error.code, ChatErrorCode::Validation);
    assert_eq!(
        error.field.as_deref(),
        Some("chat.behavior.terminalScrollbackLines")
    );

    let short_timeout = ChatBehaviorPreferences {
        idle_session_timeout_seconds: 59,
        ..ChatBehaviorPreferences::default()
    };
    assert_eq!(
        short_timeout.validate().unwrap_err().field.as_deref(),
        Some("chat.behavior.idleSessionTimeoutSeconds")
    );
    assert!(ChatBehaviorPreferences::default().validate().is_ok());
}

#[test]
fn behavior_writes_one_portable_preference_per_field() {
    let behavior = ChatBehaviorPreferences {
        send_key: ChatSendKey::ModEnter,
        terminal_scrollback_lines: 2_000,
        ..ChatBehaviorPreferences::default()
    };
    let patches = behavior_patches(&behavior).unwrap();
    let paths = patches
        .iter()
        .map(|patch| patch.path.join("."))
        .collect::<Vec<_>>();

    assert_eq!(patches.len(), 7);
    assert!(patches.iter().all(|patch| !patch.remove));
    assert!(paths.iter().all(|path| path.starts_with("chat.behavior.")));
    let send_key = patches
        .iter()
        .find(|patch| patch.path.join(".") == "chat.behavior.sendKey")
        .unwrap();
    assert_eq!(send_key.value, json!("mod_enter"));
}

#[test]
fn behavior_reads_defaults_for_missing_keys_of_the_composed_view() {
    assert_eq!(
        behavior_from_view(&json!({ "theme": { "activeId": "dark" } })),
        ChatBehaviorPreferences::default()
    );
    let view = json!({
        "chat": { "behavior": { "sendKey": "mod_enter", "showReasoningSummaries": false } }
    });
    let behavior = behavior_from_view(&view);
    assert_eq!(behavior.send_key, ChatSendKey::ModEnter);
    assert!(!behavior.show_reasoning_summaries);
    assert_eq!(
        behavior.terminal_scrollback_lines,
        ChatBehaviorPreferences::default().terminal_scrollback_lines
    );
}

#[test]
fn configuration_reads_flatten_device_fields_beside_the_behavior() {
    let configuration = ChatConfiguration {
        device: ChatDeviceConfig::from_stored(Some(&valid_device_config())).unwrap(),
        behavior: ChatBehaviorPreferences::default(),
    };
    let serialized = serde_json::to_value(&configuration).unwrap();

    assert_eq!(serialized["panels"]["inspectorWidthPx"], json!(360));
    assert_eq!(serialized["behavior"]["sendKey"], json!("enter"));
    assert!(serialized.get("schemaVersion").is_none());
}
