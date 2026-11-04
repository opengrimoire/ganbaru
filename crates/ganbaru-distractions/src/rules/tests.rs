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
    assert!(super::is_protected_desktop_app_name("Ganbaru"));
    assert!(super::is_protected_desktop_app_name(
        "org.opengrimoire.ganbaru"
    ));
    assert!(super::is_protected_desktop_app_name(
        "org.opengrimoire.ganbaru.dev"
    ));
    assert!(super::is_protected_desktop_app_name("Terminal"));
    assert!(super::is_protected_desktop_app_name("gnome-shell"));
    assert!(super::is_protected_desktop_app_name("python3.12"));
    assert!(super::is_protected_desktop_app_name("explorer.exe"));
    assert!(!super::is_protected_desktop_app_name("Steam"));
}

#[test]
fn protects_session_services_multiplexers_terminals_and_compositors() {
    for name in [
        "systemd",
        "pipewire",
        "wireplumber",
        "pulseaudio",
        "ssh-agent",
        "gpg-agent",
        "sshd",
        "tmux",
        "screen",
        "alacritty",
        "foot",
        "gnome-session-binary",
        "sway",
        "Hyprland",
    ] {
        assert!(super::is_protected_desktop_app_name(name), "{name}");
    }
}

#[test]
fn is_protected_observed_process_name_covers_session_managers() {
    for name in [
        "i3",
        "sway",
        "awesome",
        "openbox",
        "bspwm",
        "cinnamon",
        "cinnamon-session",
        "cinnamon-sessio",
        "xfce4-session",
        "xfwm4",
        "xfdesktop",
        "mate-session",
        "marco",
        "mate-terminal",
        "lxsession",
        "lxqt-session",
    ] {
        assert!(super::is_protected_observed_process_name(name), "{name}");
    }
}

#[test]
fn is_protected_observed_process_name_covers_x11_window_managers() {
    for name in [
        "dwm",
        "herbstluftwm",
        "qtile",
        "fluxbox",
        "icewm",
        "icewm-session",
        "xmonad",
        "fvwm",
        "fvwm3",
    ] {
        assert!(super::is_protected_observed_process_name(name), "{name}");
    }
}

#[test]
fn protects_truncated_comm_names_of_protected_processes() {
    assert!(super::is_protected_observed_process_name("gnome-terminal-"));
    assert!(super::is_protected_observed_process_name("gnome-session-b"));
    assert!(super::is_protected_observed_process_name("xdg-desktop-por"));
    assert!(super::is_protected_observed_process_name("gnome-terminal"));
    assert!(!super::is_protected_observed_process_name("gnome-chess"));
    assert!(!super::is_protected_observed_process_name(
        "steam-runtime-l"
    ));
    assert!(!super::is_protected_observed_process_name("gnome-ter"));
}

#[test]
fn trusted_match_names_keep_own_aliases_and_catalog_names_only() {
    let catalog = vec![
        "firefox".to_string(),
        "firefox.desktop".to_string(),
        "firefox-bin".to_string(),
    ];
    let trusted = super::trusted_process_match_names(
        "Firefox",
        vec![
            "firefox".to_string(),
            "/usr/lib/firefox/firefox-bin".to_string(),
            "pipewire-media".to_string(),
            "code".to_string(),
        ],
        &catalog,
    );
    assert_eq!(trusted, vec!["Firefox", "firefox-bin"]);

    let typed = super::trusted_process_match_names("steam", vec!["steam".to_string()], &[]);
    assert_eq!(typed, vec!["steam"]);
    let hidden = super::trusted_process_match_names(
        "Solitaire",
        vec!["Solitaire".to_string(), "obsidian".to_string()],
        &[],
    );
    assert_eq!(hidden, vec!["Solitaire"]);
}

#[test]
fn desktop_rule_matchers_keep_a_stored_identity_that_differs_from_the_name() {
    let rule_identity = super::DistractionsDesktopRuleIdentity::DesktopApp {
        rule_id: "stored  Identity".to_string(),
    };
    let matchers = super::desktop_rule_matchers(vec![DistractionsDesktopAppRuleInput {
        rule_identity: rule_identity.clone(),
        name: "Steam".to_string(),
        match_names: vec!["steam".to_string()],
    }]);

    assert_eq!(
        matchers.get("steam").map(|matcher| &matcher.rule_identity),
        Some(&rule_identity)
    );
}

#[test]
fn desktop_rule_matchers_enforce_a_whitespace_identity_and_skip_only_an_empty_one() {
    let desktop_identity = super::DistractionsDesktopRuleIdentity::DesktopApp {
        rule_id: "\t".to_string(),
    };
    let limit_identity = super::DistractionsDesktopRuleIdentity::UsageLimit {
        rule_id: "\u{00A0}".to_string(),
        entry_id: "\t".to_string(),
    };
    let matchers = super::desktop_rule_matchers(vec![
        DistractionsDesktopAppRuleInput {
            rule_identity: desktop_identity.clone(),
            name: "Steam".to_string(),
            match_names: vec!["steam".to_string()],
        },
        DistractionsDesktopAppRuleInput {
            rule_identity: limit_identity.clone(),
            name: "Discord".to_string(),
            match_names: vec!["discord".to_string()],
        },
        DistractionsDesktopAppRuleInput {
            rule_identity: super::DistractionsDesktopRuleIdentity::DesktopApp {
                rule_id: String::new(),
            },
            name: "Spotify".to_string(),
            match_names: vec!["spotify".to_string()],
        },
        DistractionsDesktopAppRuleInput {
            rule_identity: super::DistractionsDesktopRuleIdentity::UsageLimit {
                rule_id: "limit".to_string(),
                entry_id: String::new(),
            },
            name: "Slack".to_string(),
            match_names: vec!["slack".to_string()],
        },
    ]);

    assert_eq!(
        matchers.get("steam").map(|matcher| &matcher.rule_identity),
        Some(&desktop_identity)
    );
    assert_eq!(
        matchers
            .get("discord")
            .map(|matcher| &matcher.rule_identity),
        Some(&limit_identity)
    );
    assert!(!matchers.contains_key("spotify"));
    assert!(!matchers.contains_key("slack"));
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
