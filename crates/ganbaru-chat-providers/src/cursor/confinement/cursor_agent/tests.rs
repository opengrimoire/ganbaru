use super::*;
use std::sync::OnceLock;

fn helper() -> &'static SandboxHelper {
    static HELPER: OnceLock<SandboxHelper> = OnceLock::new();
    HELPER.get_or_init(|| SandboxHelper::at("/usr/lib/ganbaru/ganbaru-sandbox"))
}

fn installation() -> CursorInstallation {
    let directory = PathBuf::from("/home/person/.local/share/cursor-agent/versions/2026.11.11-b9");
    CursorInstallation {
        runtime: directory.join("node"),
        entry: directory.join("index.js"),
        search: Some(directory.join("rg")),
        directory,
    }
}

fn policy_paths<'a>(
    installation: &'a CursorInstallation,
    workspace: &'a Path,
    loaders: &'a [PathBuf],
) -> CursorPolicyPaths<'a> {
    CursorPolicyPaths {
        installation,
        workspace,
        writes_workspace: false,
        credentials: Some(Path::new("/home/person/.config/cursor")),
        configuration: Path::new("/home/person/.local/state/ganbaru/cursor-agents/ab/config"),
        home: Path::new("/tmp/ganbaru-confined-a/home"),
        data: Path::new("/tmp/ganbaru-confined-a/data"),
        temporary: Path::new("/tmp/ganbaru-confined-a/tmp"),
        loaders,
    }
}

#[test]
fn cursor_claims_folder_boundaries_but_never_commands() {
    assert_eq!(
        cursor_authority_support(false),
        ProviderAuthoritySupport {
            isolated_conversation: true,
            internal_host_tools: true,
            ..ProviderAuthoritySupport::default()
        }
    );
    let confined = cursor_authority_support(true);
    assert!(confined.deny_shell && confined.read_only_root && confined.writable_root);
    assert!(confined.network_boundary);
    assert!(!confined.confined_commands);
    assert!(!confined.classified_publish);
    assert!(!confined.person_approved_commands);
}

#[test]
fn folder_launches_need_the_helper_and_command_launches_are_refused() {
    assert_eq!(cursor_confinement(None, Some(helper())).unwrap(), None);
    assert_eq!(
        cursor_confinement(Some(AcpFolderAccess::Conversation), None).unwrap(),
        None
    );
    for access in [
        AcpFolderAccess::Conversation,
        AcpFolderAccess::Read,
        AcpFolderAccess::Edit,
    ] {
        assert_eq!(
            cursor_confinement(Some(access), Some(helper())).unwrap(),
            Some(helper())
        );
    }
    for access in [AcpFolderAccess::Read, AcpFolderAccess::Edit] {
        let error = cursor_confinement(Some(access), None).unwrap_err();
        assert_eq!(error.code, ChatErrorCode::Permission, "{access:?}");
    }
    for helper in [None, Some(helper())] {
        let error = cursor_confinement(Some(AcpFolderAccess::Commands), helper).unwrap_err();
        assert!(error.message.starts_with("acp_command_outside_scope"));
    }
}

#[test]
fn only_a_standard_installation_at_the_minimum_version_is_recognized() {
    let versions = Path::new("/opt/cursor/versions");
    let launcher = |version: &str| versions.join(version).join("cursor-agent");
    let current = cursor_installation(&launcher("2026.11.11-b912975"), |_| true).unwrap();
    assert_eq!(current.directory, versions.join("2026.11.11-b912975"));
    assert_eq!(current.runtime, current.directory.join("node"));
    assert_eq!(current.entry, current.directory.join("index.js"));
    assert_eq!(current.search, Some(current.directory.join("rg")));

    let without_search =
        cursor_installation(&launcher("2027.01.02-a1"), |path| !path.ends_with("rg")).unwrap();
    assert_eq!(without_search.search, None);

    let old = cursor_installation(&launcher("2026.11.10-ffffff"), |_| true).unwrap_err();
    assert_eq!(old.code, ChatErrorCode::UnsupportedVersion);
    assert!(old.message.contains("2026.11.11"));

    for unrecognized in [
        versions.join("2026.11.11-b9/agent"),
        versions.join("latest/cursor-agent"),
        PathBuf::from("/usr/local/bin/cursor-agent"),
    ] {
        let error = cursor_installation(&unrecognized, |_| true).unwrap_err();
        assert_eq!(
            error.code,
            ChatErrorCode::ConfigurationInvalid,
            "{unrecognized:?}"
        );
    }
    for missing in ["node", "index.js"] {
        let error =
            cursor_installation(&launcher("2026.11.11-b9"), |path| !path.ends_with(missing))
                .unwrap_err();
        assert_eq!(error.code, ChatErrorCode::ConfigurationInvalid, "{missing}");
    }
}

#[test]
fn only_an_https_endpoint_on_the_api_port_is_reachable() {
    ensure_confined_endpoint(None).unwrap();
    ensure_confined_endpoint(Some("https://api.example.com")).unwrap();
    ensure_confined_endpoint(Some("https://api.example.com:443/v1")).unwrap();
    for endpoint in [
        "http://api.example.com",
        "https://api.example.com:8443",
        "not a url",
    ] {
        assert_eq!(
            ensure_confined_endpoint(Some(endpoint)).unwrap_err().code,
            ChatErrorCode::Permission,
            "{endpoint}"
        );
    }
}

#[test]
fn a_read_process_reads_the_folder_and_writes_only_its_private_state() {
    let installation = installation();
    let loaders = [PathBuf::from(
        "/usr/lib/x86_64-linux-gnu/ld-linux-x86-64.so.2",
    )];
    let workspace = Path::new("/home/person/Documents/Ganbaru/projects/site");
    let paths = policy_paths(&installation, workspace, &loaders);
    let policy = cursor_process_policy(&paths, Some(41234), |_| true).unwrap();

    assert!(policy.read.contains(&workspace.to_path_buf()));
    assert!(policy.read.contains(&installation.directory));
    assert!(
        policy
            .read
            .contains(&PathBuf::from("/home/person/.config/cursor"))
    );
    assert_eq!(
        policy.execute,
        vec![
            installation.runtime.clone(),
            installation.directory.join("rg"),
            loaders[0].clone(),
        ]
    );
    assert_eq!(
        policy.write,
        vec![
            PathBuf::from(paths.configuration),
            PathBuf::from(paths.home),
            PathBuf::from(paths.data),
            PathBuf::from(paths.temporary),
            installation.directory.join(".running"),
            PathBuf::from("/dev/null"),
        ]
    );
    for path in policy
        .read
        .iter()
        .chain(&policy.write)
        .chain(&policy.execute)
    {
        assert_ne!(path, Path::new("/home/person"));
        assert_ne!(path, Path::new("/"));
        assert!(!path.starts_with("/home/person/.ssh"));
        assert!(!path.starts_with("/home/person/Documents/Ganbaru/ganbaru.sqlite"));
    }
    assert_eq!(policy.network, NetworkAccess::TcpConnect(vec![443, 41234]));
}

#[test]
fn an_edit_process_writes_the_folder_without_reading_it_twice() {
    let installation = installation();
    let workspace = Path::new("/srv/work");
    let paths = CursorPolicyPaths {
        writes_workspace: true,
        credentials: None,
        ..policy_paths(&installation, workspace, &[])
    };
    let policy = cursor_process_policy(&paths, None, |path| path != Path::new("/lib64")).unwrap();
    assert!(policy.write.contains(&workspace.to_path_buf()));
    assert!(!policy.read.contains(&workspace.to_path_buf()));
    assert!(!policy.read.contains(&PathBuf::from("/lib64")));
    assert!(!policy.read.iter().any(|path| path.ends_with("cursor")));
    assert_eq!(policy.network, NetworkAccess::TcpConnect(vec![443]));
}

#[test]
fn overlapping_folders_are_refused() {
    let installation = installation();
    let workspace = Path::new("/srv/work");
    let base = policy_paths(&installation, workspace, &[]);
    let refused = |paths: CursorPolicyPaths<'_>| {
        assert!(cursor_process_policy(&paths, None, |_| true).is_err());
    };
    // The working folder holding or inside a private folder, the installation, or the sign-in.
    refused(CursorPolicyPaths {
        workspace: Path::new("/tmp/ganbaru-confined-a"),
        ..base
    });
    refused(CursorPolicyPaths {
        workspace: Path::new("/home/person/.local/share/cursor-agent"),
        ..base
    });
    refused(CursorPolicyPaths {
        workspace: Path::new("/home/person/.config"),
        ..base
    });
    // A private folder reaching the runtime, the entry script, or the sign-in.
    refused(CursorPolicyPaths {
        home: &installation.directory,
        ..base
    });
    refused(CursorPolicyPaths {
        temporary: Path::new("/home/person/.config/cursor/tmp"),
        ..base
    });
    let inside = CursorInstallation {
        directory: PathBuf::from("/tmp/ganbaru-confined-a/data/cursor"),
        runtime: PathBuf::from("/tmp/ganbaru-confined-a/data/cursor/node"),
        entry: PathBuf::from("/tmp/ganbaru-confined-a/data/cursor/index.js"),
        search: None,
    };
    refused(CursorPolicyPaths {
        installation: &inside,
        ..base
    });
}

#[test]
fn the_configuration_folder_is_reseeded_without_following_links() {
    let state = crate::probe_directory::ProbeDirectory::create().unwrap();
    let outside = crate::probe_directory::ProbeDirectory::create().unwrap();
    let workspace = Path::new("/srv/work");
    let folder =
        prepare_configuration_folder(&state, workspace, &agent_cli_configuration(None)).unwrap();
    assert!(folder.starts_with(std::fs::canonicalize(&*state).unwrap()));
    assert_eq!(
        prepare_configuration_folder(&state, workspace, &agent_cli_configuration(None)).unwrap(),
        folder
    );
    assert_ne!(
        prepare_configuration_folder(
            &state,
            Path::new("/srv/other"),
            &agent_cli_configuration(None)
        )
        .unwrap(),
        folder
    );

    std::fs::write(folder.join("mcp.json"), "{}").unwrap();
    std::fs::create_dir(folder.join("hooks.json")).unwrap();
    std::fs::write(folder.join("session.json"), "kept").unwrap();
    let target = outside.join("victim.json");
    std::fs::write(&target, "untouched").unwrap();
    std::fs::remove_file(folder.join("cli-config.json")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&target, folder.join("cli-config.json")).unwrap();

    prepare_configuration_folder(&state, workspace, &agent_cli_configuration(None)).unwrap();
    assert!(!folder.join("mcp.json").exists());
    assert!(!folder.join("hooks.json").exists());
    assert_eq!(
        std::fs::read_to_string(folder.join("session.json")).unwrap(),
        "kept"
    );
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "untouched");
    let seeded = folder.join("cli-config.json");
    assert!(!std::fs::symlink_metadata(&seeded).unwrap().is_symlink());
    assert!(std::fs::symlink_metadata(&seeded).unwrap().is_file());
    let configuration: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(seeded).unwrap()).unwrap();
    assert_eq!(
        configuration["permissions"]["deny"],
        serde_json::json!(["Shell(*)"])
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&folder).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);
    }
}

#[test]
fn a_running_marker_that_is_not_a_folder_is_refused() {
    let installation = crate::probe_directory::ProbeDirectory::create().unwrap();
    prepare_running_marker(&installation).unwrap();
    prepare_running_marker(&installation).unwrap();
    assert!(installation.join(".running").is_dir());

    let linked = crate::probe_directory::ProbeDirectory::create().unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&*installation, linked.join(".running")).unwrap();
        assert!(prepare_running_marker(&linked).is_err());
    }
    std::fs::remove_file(linked.join(".running")).ok();
    std::fs::write(linked.join(".running"), "").unwrap();
    assert!(prepare_running_marker(&linked).is_err());
}

#[test]
fn the_configured_environment_cannot_redirect_the_private_folders() {
    for name in RESERVED_ENVIRONMENT {
        let configured = BTreeMap::from([(name.to_string(), "/elsewhere".to_string())]);
        let error = prepare_cursor_launch(
            Path::new("/opt/cursor/versions/2026.11.11-b9/cursor-agent"),
            Path::new("/srv/work"),
            AcpFolderAccess::Read,
            &BTreeMap::new(),
            &configured,
            None,
        )
        .err()
        .unwrap();
        assert_eq!(error.code, ChatErrorCode::Validation, "{name}");
    }
}

#[test]
fn the_agent_configuration_keeps_allowlisted_user_preferences_only() {
    let user = serde_json::json!({
        "maxMode": true,
        "network": { "useHttp1ForAgent": true, "proxy": "http://example.invalid" },
        "attribution": { "attributeCommitsToAgent": false, "attributePRsToAgent": "no" },
        "permissions": { "allow": ["Shell(*)"], "deny": [] },
        "approvalMode": "unrestricted",
        "sandbox": { "mode": "enabled" },
        "model": { "modelId": "other" },
    });
    let configuration: serde_json::Value =
        serde_json::from_str(&agent_cli_configuration(Some(user.to_string().as_bytes()))).unwrap();
    assert_eq!(
        configuration,
        serde_json::json!({
            "version": 1,
            "permissions": { "allow": [], "deny": ["Shell(*)"] },
            "approvalMode": "allowlist",
            "sandbox": { "mode": "disabled" },
            "maxMode": true,
            "network": { "useHttp1ForAgent": true },
            "attribution": { "attributeCommitsToAgent": false },
        })
    );
    for user in [None, Some(&b"{ not json"[..]), Some(&b"[true]"[..])] {
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&agent_cli_configuration(user)).unwrap(),
            serde_json::from_str::<serde_json::Value>(CURSOR_CLI_CONFIGURATION).unwrap()
        );
    }
}

#[test]
fn the_user_configuration_is_read_where_the_cli_reads_it() {
    let home = crate::probe_directory::ProbeDirectory::create().unwrap();
    let xdg = home.join("xdg");
    let mut environment = BTreeMap::new();
    assert_eq!(
        user_cli_configuration_path(&environment, &home, &home.join(".config")),
        home.join(".cursor/cli-config.json")
    );
    environment.insert("XDG_CONFIG_HOME".to_string(), xdg.display().to_string());
    assert_eq!(
        user_cli_configuration_path(&environment, &home, &xdg),
        home.join(".cursor/cli-config.json"),
        "an XDG folder without the file is skipped"
    );
    std::fs::create_dir_all(xdg.join("cursor")).unwrap();
    std::fs::write(xdg.join("cursor/cli-config.json"), "{}").unwrap();
    assert_eq!(
        user_cli_configuration_path(&environment, &home, &xdg),
        xdg.join("cursor/cli-config.json")
    );
    environment.insert("CURSOR_CONFIG_DIR".to_string(), "/srv/cursor".to_string());
    assert_eq!(
        user_cli_configuration_path(&environment, &home, &xdg),
        Path::new("/srv/cursor/cli-config.json")
    );
}
