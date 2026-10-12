use super::*;

#[test]
fn foreground_close_rejects_reused_processes_changed_apps_and_protected_aliases() {
    let mut status =
        foreground_status_from_parts("Game", Some("game".into()), Some(42), vec!["game".into()]);
    status.process_identity = Some("process-start-1".into());
    let expected = DistractionsForegroundDesktopAppExpectation {
        app_name: status.app_name.clone(),
        process_name: status.process_name.clone(),
        process_id: status.process_id,
        process_identity: status.process_identity.clone(),
        match_names: status.match_names.clone(),
    };
    assert!(foreground_expectation_matches(&status, &expected));
    status.process_identity = Some("process-start-2".into());
    assert!(!foreground_expectation_matches(&status, &expected));
    status.process_identity = expected.process_identity.clone();
    status.app_name = Some("Other game".into());
    assert!(!foreground_expectation_matches(&status, &expected));
    status.app_name = expected.app_name.clone();
    status.match_names.push("bash".into());
    assert!(validate_foreground_status_is_closeable(&status).is_err());
    let protected =
        foreground_status_from_parts("Game", Some("bash".into()), Some(42), vec!["game".into()]);
    assert!(validate_foreground_status_is_closeable(&protected).is_err());
}

#[test]
fn normalizes_desktop_app_candidate_names() {
    assert_eq!(
        normalize_app_candidate_name("  Visual   Studio Code  ").as_deref(),
        Some("Visual Studio Code")
    );
    assert!(normalize_app_candidate_name("   ").is_none());
}

#[test]
fn recognizes_protected_desktop_app_and_process_names() {
    assert!(super::is_protected_desktop_app_name("Ganbaru AI"));
    assert!(super::is_protected_desktop_app_name(
        "org.opengrimoire.ganbaruai"
    ));
    assert!(super::is_protected_desktop_app_name(
        "org.opengrimoire.ganbaruai.dev"
    ));
    assert!(super::is_protected_desktop_app_name("Terminal"));
    assert!(super::is_protected_desktop_app_name("gnome-shell"));
    assert!(super::is_protected_desktop_app_name("python3.12"));
    assert!(super::is_protected_desktop_app_name("explorer.exe"));
    assert!(!super::is_protected_desktop_app_name("Steam"));
}

#[test]
fn desktop_rule_matchers_skip_protected_process_names() {
    let matchers = super::desktop_rule_matchers(vec![
        DistractionsDesktopAppRuleInput {
            rule_identity: super::DistractionsDesktopRuleIdentity::DesktopApp {
                rule_id: "Terminal".to_string(),
            },
            name: "Terminal".to_string(),
            match_names: vec!["gnome-terminal".to_string()],
        },
        DistractionsDesktopAppRuleInput {
            rule_identity: super::DistractionsDesktopRuleIdentity::DesktopApp {
                rule_id: "Steam".to_string(),
            },
            name: "Steam".to_string(),
            match_names: vec!["sh".to_string(), "steam".to_string()],
        },
    ]);

    assert!(!matchers.contains_key("terminal"));
    assert!(!matchers.contains_key("gnome-terminal"));
    assert!(!matchers.contains_key("sh"));
    assert_eq!(
        matchers
            .get("steam")
            .map(|matcher| matcher.app_name.as_str()),
        Some("Steam")
    );
}
