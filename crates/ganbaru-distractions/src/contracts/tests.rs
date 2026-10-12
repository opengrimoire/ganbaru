use super::*;

#[test]
fn desktop_rule_identity_preserves_tagged_wire_shape() {
    let desktop = DistractionsDesktopRuleIdentity::DesktopApp {
        rule_id: "Steam".to_string(),
    };
    let usage = DistractionsDesktopRuleIdentity::UsageLimit {
        rule_id: "games".to_string(),
        entry_id: "steam".to_string(),
    };

    assert_eq!(
        serde_json::to_value(desktop).unwrap(),
        serde_json::json!({"kind": "desktop-app", "ruleId": "Steam"})
    );
    assert_eq!(
        serde_json::to_value(usage).unwrap(),
        serde_json::json!({
            "kind": "usage-limit",
            "ruleId": "games",
            "entryId": "steam"
        })
    );
}

#[test]
fn public_status_preserves_camel_case_wire_shape() {
    let status = DistractionsForegroundDesktopAppStatus {
        process_identity: None,
        available: true,
        app_name: Some("Steam".to_string()),
        process_name: Some("steam".to_string()),
        process_id: Some(42),
        match_names: vec!["steam".to_string()],
        reason: None,
    };

    assert_eq!(
        serde_json::to_value(status).unwrap(),
        serde_json::json!({
            "available": true,
            "appName": "Steam",
            "processName": "steam",
            "processId": 42,
            "matchNames": ["steam"],
            "reason": null
        })
    );
}
