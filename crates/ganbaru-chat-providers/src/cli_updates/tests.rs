use super::*;
use std::path::PathBuf;

const RANGE: ProviderCompatibilityRange = ProviderCompatibilityRange {
    minimum: "2.1.170",
    maximum_tested: "2.1.322",
    ceiling_exclusive: Some("3.0.0"),
};

#[test]
fn a_newer_release_below_the_ceiling_is_offered() {
    assert!(offers_update(&RANGE, "2.1.311", "2.1.326"));
    assert!(offers_update(&RANGE, "2.1.311-beta", "2.2.0"));
}

#[test]
fn the_same_an_older_or_an_unsupported_release_is_not_offered() {
    assert!(!offers_update(&RANGE, "2.1.326", "2.1.326"));
    assert!(!offers_update(&RANGE, "2.1.326", "2.1.311"));
    assert!(!offers_update(&RANGE, "2.1.326", "3.0.0"));
    assert!(!offers_update(&RANGE, "2.1.326", "nonsense"));
    assert!(!offers_update(&RANGE, "unknown", "2.1.330"));
    let open = ProviderCompatibilityRange {
        ceiling_exclusive: None,
        ..RANGE
    };
    assert!(offers_update(&open, "2.1.326", "9.0.0"));
}

#[test]
fn release_documents_yield_only_plain_versions() {
    assert_eq!(parse_plain_version(b"2.1.326\n"), Some("2.1.326".into()));
    assert_eq!(parse_plain_version(b"<html>moved</html>"), None);
    assert_eq!(
        parse_npm_version(br#"{"name":"opencode-ai","version":"1.19.6"}"#),
        Some("1.19.6".into())
    );
    assert_eq!(parse_npm_version(br#"{"version":"1.19.6 && rm"}"#), None);
    assert_eq!(parse_npm_version(b"[]"), None);
    assert_eq!(
        parse_grok_check(
            br#"{"currentVersion":"1.2.0","latestVersion":"1.3.1","updateAvailable":true}"#
        ),
        Some("1.3.1".into())
    );
    assert_eq!(parse_grok_check(br#"{"latestVersion":null}"#), None);
}

#[test]
fn claude_follows_its_configured_channel() {
    assert_eq!(
        claude_channel(br#"{"autoUpdatesChannel":"stable"}"#),
        ClaudeChannel::Stable
    );
    assert_eq!(
        claude_channel(br#"{"autoUpdatesChannel":"latest"}"#),
        ClaudeChannel::Latest
    );
    assert_eq!(claude_channel(b"{}"), ClaudeChannel::Latest);
    assert_eq!(claude_channel(b"not json"), ClaudeChannel::Latest);
}

#[test]
fn update_arguments_drop_the_program_name() {
    assert_eq!(update_arguments("opencode upgrade"), vec!["upgrade"]);
    assert_eq!(update_arguments("claude update"), vec!["update"]);
    assert!(update_arguments("claude").is_empty());
}

#[test]
fn package_manager_and_system_installs_are_left_to_their_owner() {
    for path in [
        "/opt/homebrew/bin/claude",
        "/home/linuxbrew/.linuxbrew/bin/codex",
        "/usr/local/Cellar/opencode/1.19.6/bin/opencode",
        "/home/person/.local/share/mise/installs/node/22/bin/codex",
        "/home/person/.asdf/shims/opencode",
    ] {
        assert!(package_managed(&PathBuf::from(path)), "{path}");
    }
    for path in [
        "/home/person/.local/bin/claude",
        "/home/person/.npm-global/bin/codex",
        "/home/person/.opencode/bin/opencode",
        "/home/person/.grok/bin/grok",
    ] {
        assert!(!package_managed(&PathBuf::from(path)), "{path}");
    }
}

#[cfg(unix)]
#[test]
fn system_roots_are_package_managed_on_unix() {
    assert!(package_managed(&PathBuf::from("/usr/bin/claude")));
    assert!(package_managed(&PathBuf::from(
        "/nix/store/abc-codex/bin/codex"
    )));
    assert!(package_managed(&PathBuf::from("/snap/bin/opencode")));
}

#[test]
fn a_failed_update_keeps_the_last_lines_of_its_explanation() {
    let stderr = (1..=20)
        .map(|line| format!("line {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    let tail = output_tail(b"ignored", stderr.as_bytes()).unwrap();
    assert!(tail.starts_with("line 9\n"));
    assert!(tail.ends_with("line 20"));
    assert_eq!(
        output_tail(b"only stdout\n\n", b"  \n"),
        Some("only stdout".into())
    );
    assert_eq!(output_tail(b"", b""), None);
}

#[test]
fn every_checked_family_has_a_supported_range() {
    for family in ["claude", "codex", "opencode", "grok"] {
        assert!(release_source(family).is_some(), "{family}");
        assert!(compatibility(family).is_some(), "{family}");
    }
    assert_eq!(release_source("cursor"), None);
    assert_eq!(release_source("acp"), None);
}

#[test]
fn updates_pin_the_checked_release_where_the_cli_takes_one() {
    assert_eq!(
        pinned_update_command("opencode", "opencode upgrade", "1.15.2"),
        "opencode upgrade 1.15.2"
    );
    assert_eq!(
        pinned_update_command("grok", "grok update", "0.1.160"),
        "grok update --version 0.1.160"
    );
    assert_eq!(
        pinned_update_command("codex", "codex update", "0.170.0"),
        "codex update"
    );
    assert_eq!(
        pinned_update_command("claude", "claude update", "2.1.330"),
        "claude update"
    );
}

#[test]
fn an_update_request_takes_only_an_offered_plain_version() {
    let ceiling = crate::claude::CLAUDE_COMPATIBILITY
        .ceiling_exclusive
        .expect("Claude has a ceiling");
    assert!(takes_target("claude", "2.1.170", "2.1.171"));
    assert!(!takes_target("claude", "2.1.171", "2.1.171"));
    assert!(!takes_target("claude", "2.1.170", ceiling));
    assert!(!takes_target("opencode", "1.14.19", " 1.15.0"));
    assert!(!takes_target("opencode", "1.14.19", "1.15.0 --force"));
    assert!(!takes_target("acp", "1.0.0", "2.0.0"));
}
