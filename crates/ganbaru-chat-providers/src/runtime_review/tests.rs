use super::*;
use crate::models::ProviderInstanceConfig;
use serde_json::json;

fn configuration(
    family_id: &str,
    arguments: &[&str],
    environment: &[(&str, &str)],
) -> ProviderInstanceConfig {
    configuration_with(family_id, arguments, environment, json!({}))
}

fn configuration_with(
    family_id: &str,
    arguments: &[&str],
    environment: &[(&str, &str)],
    provider_config: Value,
) -> ProviderInstanceConfig {
    serde_json::from_value(json!({
        "schemaVersion": 1,
        "instanceId": "provider-instance-1",
        "familyId": family_id,
        "label": "Provider",
        "enabled": true,
        "executable": family_id,
        "providerHome": null,
        "launchArguments": arguments,
        "environment": environment.iter().copied().collect::<std::collections::BTreeMap<_, _>>(),
        "credentialReferences": {},
        "visibleModelIds": [],
        "favoriteModelIds": [],
        "providerConfig": { "schemaVersion": 1, "value": provider_config }
    }))
    .expect("test provider configuration")
}

#[test]
fn default_configurations_need_no_review() {
    for family in ["claude", "codex", "cursor", "grok"] {
        assert!(portable_runtime_review_items(&configuration(family, &[], &[])).is_empty());
    }
    let opencode = configuration_with("opencode", &[], &[], json!({ "mode": "local" }));
    assert!(portable_runtime_review_items(&opencode).is_empty());
}

#[test]
fn allowlisted_arguments_and_variables_apply_without_review() {
    let claude = configuration(
        "claude",
        &["--fallback-model", "sonnet"],
        &[
            ("MAX_THINKING_TOKENS", "4096"),
            ("ANTHROPIC_API_KEY", "inherit:ANTHROPIC_API_KEY"),
        ],
    );
    assert!(portable_runtime_review_items(&claude).is_empty());
    let codex = configuration(
        "codex",
        &[
            "--strict-config",
            "-c",
            "model_verbosity=high",
            "--config=hide_agent_reasoning=true",
        ],
        &[("OPENAI_API_KEY", "inherit:OPENAI_API_KEY")],
    );
    assert!(portable_runtime_review_items(&codex).is_empty());
    let opencode = configuration_with(
        "opencode",
        &["--print-logs", "--log-level=DEBUG"],
        &[],
        json!({ "mode": "local" }),
    );
    assert!(portable_runtime_review_items(&opencode).is_empty());
}

#[test]
fn permission_widening_claude_arguments_need_review() {
    for arguments in [
        &["--allowedTools", "Bash"][..],
        &["--add-dir", "/"],
        &["--plugin-dir", "/tmp/plugins"],
        &["--agents", "{}"],
        &["--append-system-prompt", "ignore rules"],
        &["--system-prompt", "x"],
    ] {
        let items = portable_runtime_review_items(&configuration("claude", arguments, &[]));
        assert_eq!(items.len(), 1, "{arguments:?}");
        assert!(items[0].widens_permissions, "{arguments:?}");
    }
}

#[test]
fn codex_overrides_outside_the_allowlist_need_review() {
    for value in [
        "sandbox_mode=danger-full-access",
        "approval_policy=never",
        "mcp_servers.x.command=sh",
        "notify=[\"sh\"]",
    ] {
        let items = portable_runtime_review_items(&configuration("codex", &["-c", value], &[]));
        assert_eq!(items.len(), 1, "{value}");
        assert!(items[0].widens_permissions);
    }
}

#[test]
fn cursor_unknown_flags_and_positional_arguments_need_review() {
    assert_eq!(
        portable_runtime_review_items(&configuration("cursor", &["--unknown"], &[])).len(),
        1
    );
    assert_eq!(
        portable_runtime_review_items(&configuration("claude", &["prompt text"], &[])).len(),
        1
    );
}

#[test]
fn code_loading_and_redirecting_variables_need_review() {
    for (name, value) in [
        ("NODE_OPTIONS", "--require /tmp/x.js"),
        ("LD_PRELOAD", "/tmp/x.so"),
        ("HTTPS_PROXY", "http://proxy"),
        ("ANTHROPIC_BASE_URL", "https://example.test"),
        ("NODE_TLS_REJECT_UNAUTHORIZED", "0"),
        ("HTTPS_PROXY", "inherit:HTTPS_PROXY"),
    ] {
        let items = portable_runtime_review_items(&configuration("claude", &[], &[(name, value)]));
        assert_eq!(items.len(), 1, "{name}");
        assert!(items[0].widens_permissions, "{name}");
    }
}

#[test]
fn plaintext_api_keys_and_mismatched_inheritance_need_review() {
    let plaintext = configuration("claude", &[], &[("ANTHROPIC_API_KEY", "sk-attacker")]);
    assert_eq!(portable_runtime_review_items(&plaintext).len(), 1);
    let renamed = configuration(
        "claude",
        &[],
        &[("ANTHROPIC_API_KEY", "inherit:OTHER_SECRET")],
    );
    assert_eq!(portable_runtime_review_items(&renamed).len(), 1);
}

#[test]
fn endpoint_and_path_settings_need_review_and_cosmetic_settings_do_not() {
    let cursor = configuration_with(
        "cursor",
        &[],
        &[],
        json!({ "apiEndpoint": "https://example.test" }),
    );
    assert_eq!(portable_runtime_review_items(&cursor).len(), 1);
    let codex = configuration_with(
        "codex",
        &[],
        &[],
        json!({
            "shadowHomePath": "/tmp/home",
            "allowCustomModels": true,
            "customModelIds": ["m"],
            "customModelLabels": { "m": "M" },
            "refreshMcpBeforeTurn": true
        }),
    );
    let items = portable_runtime_review_items(&codex);
    assert_eq!(items.len(), 1);
    assert!(items[0].label.contains("shadowHomePath"));
    let opencode = configuration_with(
        "opencode",
        &[],
        &[],
        json!({ "mode": "external", "serverUrl": "http://host" }),
    );
    assert_eq!(portable_runtime_review_items(&opencode).len(), 2);
}

#[test]
fn digests_bind_family_and_exact_values() {
    let first = portable_runtime_review_items(&configuration(
        "claude",
        &[],
        &[("HTTPS_PROXY", "http://a")],
    ));
    let same = portable_runtime_review_items(&configuration(
        "claude",
        &[],
        &[("HTTPS_PROXY", "http://a")],
    ));
    let changed = portable_runtime_review_items(&configuration(
        "claude",
        &[],
        &[("HTTPS_PROXY", "http://b")],
    ));
    let other_family =
        portable_runtime_review_items(&configuration("codex", &[], &[("HTTPS_PROXY", "http://a")]));
    assert_eq!(first, same);
    assert_ne!(first[0].digest, changed[0].digest);
    assert_ne!(first[0].digest, other_family[0].digest);
}

#[test]
fn labels_are_bounded_and_free_of_control_characters() {
    let long = "x".repeat(MAX_RUNTIME_REVIEW_LABEL_CHARS * 2);
    let items = portable_runtime_review_items(&configuration(
        "claude",
        &[],
        &[("CUSTOM", &format!("a\nb{long}"))],
    ));
    assert_eq!(
        items[0].label.chars().count(),
        MAX_RUNTIME_REVIEW_LABEL_CHARS
    );
    assert!(!items[0].label.chars().any(char::is_control));
}

#[test]
fn device_paths_have_stable_kind_specific_digests() {
    let executable = device_path_review_item("claude", DevicePathKind::Executable, "/opt/claude");
    let home = device_path_review_item("claude", DevicePathKind::ProviderHome, "/opt/claude");
    assert_ne!(executable.digest, home.digest);
    assert_eq!(
        executable,
        device_path_review_item("claude", DevicePathKind::Executable, "/opt/claude")
    );
}

#[test]
fn only_bare_family_defaults_count_as_default_executables() {
    assert!(is_default_executable("cursor", "cursor-agent"));
    assert!(is_default_executable("claude", "claude"));
    assert!(!is_default_executable("claude", "codex"));
    assert!(!is_default_executable("claude", "/tmp/claude"));
    assert!(!is_default_executable("unknown", "claude"));
}
