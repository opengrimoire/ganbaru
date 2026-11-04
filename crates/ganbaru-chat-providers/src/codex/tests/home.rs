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
fn restricted_launches_mark_folders_with_codex_configuration_untrusted() {
    let workspace = TestDirectory::new("codex-project-configuration");
    let arguments_for = |access| {
        let mut arguments = Vec::new();
        append_project_configuration_arguments(&mut arguments, workspace.path(), access).unwrap();
        arguments
    };
    assert!(arguments_for(ProjectConfigurationAccess::Restricted).is_empty());

    std::fs::create_dir_all(workspace.path().join(".codex")).unwrap();
    assert!(arguments_for(ProjectConfigurationAccess::Trusted).is_empty());
    let key = serde_json::to_string(workspace.path().to_str().unwrap()).unwrap();
    assert_eq!(
        arguments_for(ProjectConfigurationAccess::Restricted),
        vec![
            "-c".to_string(),
            format!("projects={{{key}={{trust_level=\"untrusted\"}}}}"),
        ]
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

#[cfg(unix)]
#[test]
fn shadow_home_folders_created_for_codex_are_owner_only() {
    use std::os::unix::fs::PermissionsExt;
    let shared = TestDirectory::new("private-shared");
    let shadow_parent = TestDirectory::new("private-shadow");
    let shadow = shadow_parent.path().join("effective");
    let config = configuration(shared.path(), Some(&shadow));
    let settings = CodexProviderSettings::parse(&config).unwrap();
    let mut layout = resolve_codex_home_layout(&config, &settings).unwrap();

    materialize_codex_shadow_home(&mut layout).unwrap();
    let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&shadow), 0o700);
    assert_eq!(mode(&shared.path().join("sessions")), 0o700);
}
