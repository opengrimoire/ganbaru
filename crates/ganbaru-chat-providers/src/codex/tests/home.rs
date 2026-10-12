use super::*;

#[test]
fn launch_arguments_are_configuration_only() {
    assert_eq!(
        validated_app_server_arguments(&[
            "--strict-config".to_string(),
            "--enable".to_string(),
            "responses_websockets".to_string(),
            "-c=model=redacted".to_string(),
        ])
        .unwrap()
        .len(),
        4
    );
    assert!(validated_app_server_arguments(&["exec".to_string()]).is_err());
    assert!(
        validated_app_server_arguments(&[
            "--config".to_string(),
            "--dangerously-bypass-approvals-and-sandbox".to_string(),
        ])
        .is_err()
    );
}

#[test]
fn only_confirmed_thread_not_found_errors_allow_fresh_fallback() {
    assert!(confirmed_resume_not_found(&CodexRpcFailure::Remote {
        code: -32602,
        message: "Thread not found".to_string(),
    }));
    assert!(!confirmed_resume_not_found(&CodexRpcFailure::Remote {
        code: -32602,
        message: "Authentication failed".to_string(),
    }));
    assert!(!confirmed_resume_not_found(&CodexRpcFailure::Closed));
}

#[test]
fn direct_and_shadow_homes_share_continuation_identity() {
    let shared = TestDirectory::new("shared-home");
    let shadow = TestDirectory::new("shadow-home");
    let direct_config = configuration(shared.path(), None);
    let shadow_config = configuration(shared.path(), Some(shadow.path()));
    let direct = resolve_codex_home_layout(
        &direct_config,
        &CodexProviderSettings::parse(&direct_config).unwrap(),
    )
    .unwrap();
    let shadowed = resolve_codex_home_layout(
        &shadow_config,
        &CodexProviderSettings::parse(&shadow_config).unwrap(),
    )
    .unwrap();

    assert_eq!(
        direct.continuation_group_with_authority(false).unwrap(),
        shadowed.continuation_group_with_authority(false).unwrap()
    );
    let other = TestDirectory::new("other-home");
    let other_config = configuration(other.path(), None);
    let other_layout = resolve_codex_home_layout(
        &other_config,
        &CodexProviderSettings::parse(&other_config).unwrap(),
    )
    .unwrap();
    assert_ne!(
        direct.continuation_group_with_authority(false).unwrap(),
        other_layout
            .continuation_group_with_authority(false)
            .unwrap()
    );
}

#[cfg(unix)]
#[test]
fn shadow_home_links_shared_state_but_keeps_auth_private() {
    let shared = TestDirectory::new("materialized-shared");
    let shadow_parent = TestDirectory::new("materialized-shadow");
    let shadow = shadow_parent.path().join("effective");
    fs::write(shared.path().join("config.toml"), b"model = 'redacted'").unwrap();
    let config = configuration(shared.path(), Some(&shadow));
    let settings = CodexProviderSettings::parse(&config).unwrap();
    let mut layout = resolve_codex_home_layout(&config, &settings).unwrap();

    materialize_codex_shadow_home(&mut layout).unwrap();
    verify_codex_shadow_home(&layout).unwrap();
    assert!(
        fs::symlink_metadata(shadow.join("sessions"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(
        fs::symlink_metadata(shadow.join("config.toml"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    fs::write(shadow.join("auth.json"), b"redacted private auth").unwrap();
    verify_codex_shadow_home(&layout).unwrap();
    assert!(
        !fs::symlink_metadata(shadow.join("auth.json"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
}
