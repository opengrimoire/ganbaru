use super::*;

#[test]
fn foreground_app_detection_reports_unavailable_without_guessing() {
    let status = super::foreground_desktop_app_status();
    assert!(!status.available);
    assert!(status.app_name.is_none());
    assert!(status.process_name.is_none());
    assert!(status.process_id.is_none());
}

#[test]
fn limit_state_requires_a_local_date() {
    let state = DistractionsLimitState {
        local_date: "today".to_string(),
        week_start_local_date: "2026-05-25".to_string(),
        updated_at: "2026-05-28T00:00:00.000Z".to_string(),
        database_path: Some("/tmp/ganbaru-ai-vault/ganbaru-ai.sqlite".to_string()),
        configuration_digest: None,
        limits: vec![DistractionsLimitStateItem {
            id: "youtube".to_string(),
            period: "day".to_string(),
            window_start_local_date: "2026-05-28".to_string(),
            window_end_local_date: "2026-05-28".to_string(),
            used_seconds: 60,
            limit_seconds: 600,
            remaining_seconds: 540,
            exhausted: false,
        }],
    };

    assert!(super::validate_limit_state(&state).is_err());
}

#[test]
fn sorts_and_deduplicates_desktop_app_candidates() {
    let apps = sort_and_deduplicate_candidates(vec![
        DistractionsDesktopAppCandidate {
            name: "Terminal".to_string(),
            source: "Installed app".to_string(),
            detail: Some("terminal.desktop".to_string()),
            process_names: vec!["gnome-terminal".to_string()],
        },
        DistractionsDesktopAppCandidate {
            name: "Steam".to_string(),
            source: "Installed app".to_string(),
            detail: Some("steam.desktop".to_string()),
            process_names: vec!["Steam".to_string(), "steam".to_string()],
        },
        DistractionsDesktopAppCandidate {
            name: "steam".to_string(),
            source: "Installed app".to_string(),
            detail: Some("duplicate.desktop".to_string()),
            process_names: vec!["steam".to_string()],
        },
        DistractionsDesktopAppCandidate {
            name: "Discord".to_string(),
            source: "Installed app".to_string(),
            detail: None,
            process_names: vec!["Discord".to_string()],
        },
    ]);

    assert_eq!(
        apps.iter().map(|app| app.name.as_str()).collect::<Vec<_>>(),
        vec!["Discord", "Steam"]
    );
}

#[cfg(target_os = "linux")]
#[test]
fn process_scan_cancellation_discards_partial_matches() {
    let apps = vec![DistractionsDesktopAppRuleInput {
        rule_identity: super::DistractionsDesktopRuleIdentity::DesktopApp {
            rule_id: "Steam".to_string(),
        },
        name: "Steam".to_string(),
        match_names: vec!["steam".to_string()],
    }];
    let matches = super::list_blocked_desktop_app_matches(apps, || true);
    assert!(matches.is_empty());
}
