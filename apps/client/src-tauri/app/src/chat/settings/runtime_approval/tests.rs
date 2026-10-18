use super::*;
use serde_json::json;

fn configuration(
    arguments: &[&str],
    environment: &[(&str, &str)],
    settings: serde_json::Value,
) -> ProviderInstanceConfig {
    serde_json::from_value(json!({
        "schemaVersion": 1,
        "instanceId": "provider-instance-1",
        "familyId": "cursor",
        "label": "Cursor",
        "enabled": true,
        "executable": "cursor-agent",
        "providerHome": null,
        "launchArguments": arguments,
        "environment": environment.iter().copied().collect::<std::collections::BTreeMap<_, _>>(),
        "credentialReferences": {},
        "visibleModelIds": [],
        "favoriteModelIds": [],
        "providerConfig": { "schemaVersion": 1, "value": settings }
    }))
    .expect("test provider configuration")
}

fn restored_configurations() -> Vec<ProviderInstanceConfig> {
    vec![
        configuration(&["--allowedTools", "Bash"], &[], json!({})),
        configuration(
            &[],
            &[("NODE_OPTIONS", "--require /tmp/payload.js")],
            json!({}),
        ),
        configuration(
            &[],
            &[],
            json!({ "apiEndpoint": "https://attacker.example.test" }),
        ),
    ]
}

fn approved_device(configuration: &ProviderInstanceConfig) -> ChatProviderDeviceState {
    ChatProviderDeviceState {
        approved_runtime_items: approval_digests(configuration),
        ..ChatProviderDeviceState::default()
    }
}

#[test]
fn restored_runtime_values_stay_quarantined_until_confirmed() {
    for restored in restored_configurations() {
        let error = require_approved_runtime(&restored, None).unwrap_err();
        assert_eq!(error.code, ChatErrorCode::RuntimeApprovalRequired);
        let unconfirmed = ChatProviderDeviceState::default();
        assert!(require_approved_runtime(&restored, Some(&unconfirmed)).is_err());
        assert_eq!(
            pending_portable_items(&restored, Some(&unconfirmed)).len(),
            1
        );
    }
}

#[test]
fn portable_provider_file_locations_stay_quarantined_until_confirmed() {
    for (name, value) in [
        ("OPENCODE_CONFIG", "~/.config/other/opencode.json"),
        ("OPENCODE_CONFIG_DIR", "/tmp/opencode"),
        ("XDG_CONFIG_HOME", "/tmp/config"),
        ("OPENCODE_CONFIG", "inherit:OPENCODE_CONFIG"),
    ] {
        let mut restored = configuration(&[], &[(name, value)], json!({}));
        restored.family_id =
            ganbaru_chat_contracts::models::ProviderFamilyId::new("opencode").unwrap();
        let error = require_approved_runtime(&restored, None).unwrap_err();
        assert_eq!(error.code, ChatErrorCode::RuntimeApprovalRequired, "{name}");
        assert!(require_approved_runtime(&restored, Some(&approved_device(&restored))).is_ok());
    }
    let mut grok = configuration(&[], &[("GROK_HOME", "/tmp/grok")], json!({}));
    grok.family_id = ganbaru_chat_contracts::models::ProviderFamilyId::new("grok").unwrap();
    assert!(require_approved_runtime(&grok, None).is_err());
}

#[test]
fn confirmed_runtime_values_keep_working_after_a_restore_with_the_same_fields() {
    for restored in restored_configurations() {
        let device = approved_device(&restored);
        assert!(require_approved_runtime(&restored, Some(&device)).is_ok());
        assert!(pending_portable_items(&restored, Some(&device)).is_empty());
    }
}

#[test]
fn a_changed_value_needs_confirmation_again() {
    let confirmed = configuration(&[], &[("HTTPS_PROXY", "http://proxy.local")], json!({}));
    let device = approved_device(&confirmed);
    let changed = configuration(&[], &[("HTTPS_PROXY", "http://attacker.test")], json!({}));
    assert_eq!(
        require_approved_runtime(&changed, Some(&device))
            .unwrap_err()
            .code,
        ChatErrorCode::RuntimeApprovalRequired
    );
}

#[test]
fn default_configurations_run_without_any_confirmation() {
    let default = configuration(
        &[],
        &[("CURSOR_API_KEY", "inherit:CURSOR_API_KEY")],
        json!({}),
    );
    assert!(require_approved_runtime(&default, None).is_ok());
    assert!(ingress_review_items(&default, None, &RuntimeReceipts::default()).is_empty());
}

#[test]
fn unreceipted_device_paths_need_confirmation() {
    let mut typed = configuration(&[], &[], json!({}));
    typed.executable = "/tmp/payload".to_string();
    typed.provider_home = Some("/tmp/home".to_string());
    let receipts = RuntimeReceipts::default();
    assert_eq!(ingress_review_items(&typed, None, &receipts).len(), 2);

    receipts.record_picked_path(DevicePathKind::Executable, "/tmp/payload");
    receipts.record_picked_path(DevicePathKind::ProviderHome, "/tmp/home");
    assert!(ingress_review_items(&typed, None, &receipts).is_empty());

    let stored = ChatProviderDeviceState {
        executable_path: Some("/tmp/payload".to_string()),
        provider_home_path: Some("/tmp/home".to_string()),
        ..ChatProviderDeviceState::default()
    };
    assert!(ingress_review_items(&typed, Some(&stored), &RuntimeReceipts::default()).is_empty());
}

#[test]
fn a_picked_path_does_not_vouch_for_the_other_path_kind() {
    let mut typed = configuration(&[], &[], json!({}));
    typed.provider_home = Some("/tmp/payload".to_string());
    let receipts = RuntimeReceipts::default();
    receipts.record_picked_path(DevicePathKind::Executable, "/tmp/payload");
    assert_eq!(ingress_review_items(&typed, None, &receipts).len(), 1);
}

#[test]
fn a_test_confirmation_lets_the_following_save_apply_without_asking_again() {
    let edited = configuration(&["--unknown-flag"], &[], json!({}));
    let receipts = RuntimeReceipts::default();
    let pending = ingress_review_items(&edited, None, &receipts);
    assert_eq!(pending.len(), 1);

    receipts.record_confirmed_items(&edited.instance_id, &pending);
    assert!(ingress_review_items(&edited, None, &receipts).is_empty());

    let mut other = edited.clone();
    other.instance_id = ProviderInstanceId::new("provider-instance-2").unwrap();
    assert_eq!(ingress_review_items(&other, None, &receipts).len(), 1);
    assert!(require_approved_runtime(&edited, None).is_err());
}

#[test]
fn receipts_stay_bounded() {
    let receipts = RuntimeReceipts::default();
    for index in 0..MAX_RUNTIME_RECEIPTS + 10 {
        receipts.record_picked_path(DevicePathKind::Executable, &format!("/tmp/{index}"));
    }
    assert_eq!(receipts.entries.lock().unwrap().len(), MAX_RUNTIME_RECEIPTS);
    assert!(!receipts.vouches_for_path(DevicePathKind::Executable, "/tmp/0"));
    assert!(receipts.vouches_for_path(
        DevicePathKind::Executable,
        &format!("/tmp/{}", MAX_RUNTIME_RECEIPTS + 9)
    ));
}

#[test]
fn approvals_cover_only_current_values() {
    let first = configuration(&[], &[("HTTPS_PROXY", "http://a")], json!({}));
    let second = configuration(&[], &[("HTTPS_PROXY", "http://b")], json!({}));
    let approvals = approval_digests(&second);
    assert_eq!(approvals.len(), 1);
    assert!(approval_digests(&first).is_disjoint(&approvals));
}

#[test]
fn dialog_lists_bounded_items_and_summarizes_the_rest() {
    let environment = (0..MAX_RUNTIME_REVIEW_DIALOG_ITEMS + 3)
        .map(|index| (format!("CUSTOM_{index}"), "value".to_string()))
        .collect::<Vec<_>>();
    let environment = environment
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect::<Vec<_>>();
    let items = pending_portable_items(&configuration(&[], &environment, json!({})), None);
    let prompt = ProviderRuntimePrompt {
        title: "Allow settings".to_string(),
        message: "Review".to_string(),
        allow_label: "Allow".to_string(),
        cancel_label: "Cancel".to_string(),
        more_label: "{count} more".to_string(),
    };
    validate_runtime_prompt(&prompt).unwrap();
    let message = runtime_dialog_message(&prompt, "Cursor\nlabel", &items);
    assert_eq!(
        message.matches('\u{2022}').count(),
        MAX_RUNTIME_REVIEW_DIALOG_ITEMS
    );
    assert!(message.ends_with("3 more"));
    assert!(message.contains("Cursorlabel"));
}

#[test]
fn runtime_prompts_need_distinct_bounded_labels() {
    let prompt = ProviderRuntimePrompt {
        title: "Allow settings".to_string(),
        message: "Review".to_string(),
        allow_label: "OK".to_string(),
        cancel_label: "OK".to_string(),
        more_label: "{count} more".to_string(),
    };
    assert!(validate_runtime_prompt(&prompt).is_err());
}
