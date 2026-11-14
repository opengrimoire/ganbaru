use super::super::driver::AcpProviderFlavor;
use super::*;
use std::sync::OnceLock;

fn internal_mcp(scope: Option<ProviderOrganizationalScope>) -> ProviderInternalMcpConfig {
    ProviderInternalMcpConfig {
        name: "ganbaru".to_string(),
        url: "http://127.0.0.1:41234/mcp".to_string(),
        bearer_token: "token".to_string(),
        organizational_authority: true,
        pre_approved_tools: Vec::new(),
        organizational_scope: scope,
    }
}

fn folder(capability: ProviderFolderCapability) -> ProviderOrganizationalScope {
    ProviderOrganizationalScope::Folder { capability }
}

fn helper() -> &'static SandboxHelper {
    static HELPER: OnceLock<SandboxHelper> = OnceLock::new();
    HELPER.get_or_init(|| SandboxHelper::at("/usr/lib/ganbaru/ganbaru-sandbox"))
}

#[test]
fn organizational_scopes_map_to_folder_access() {
    for (scope, access) in [
        (
            ProviderOrganizationalScope::Conversation,
            AcpFolderAccess::Conversation,
        ),
        (
            folder(ProviderFolderCapability::Read),
            AcpFolderAccess::Read,
        ),
        (
            folder(ProviderFolderCapability::Edit),
            AcpFolderAccess::Edit,
        ),
        (
            folder(ProviderFolderCapability::Execute),
            AcpFolderAccess::Commands,
        ),
        (
            folder(ProviderFolderCapability::Publish),
            AcpFolderAccess::Commands,
        ),
        (
            ProviderOrganizationalScope::Scratch,
            AcpFolderAccess::Commands,
        ),
    ] {
        assert_eq!(AcpFolderAccess::for_scope(scope), access);
        assert_eq!(
            AcpFolderAccess::for_launch(Some(&internal_mcp(Some(scope)))).unwrap(),
            Some(access)
        );
    }
    assert!(!AcpFolderAccess::Read.writes_files());
    assert!(AcpFolderAccess::Edit.writes_files());
    assert!(!AcpFolderAccess::Edit.runs_commands());
    assert!(AcpFolderAccess::Commands.runs_commands());
}

#[test]
fn direct_threads_hold_no_folder_access_and_unscoped_authority_is_refused() {
    assert_eq!(AcpFolderAccess::for_launch(None).unwrap(), None);
    let mut personal = internal_mcp(None);
    personal.organizational_authority = false;
    assert_eq!(AcpFolderAccess::for_launch(Some(&personal)).unwrap(), None);
    let error = AcpFolderAccess::for_launch(Some(&internal_mcp(None))).unwrap_err();
    assert_eq!(error.code, ChatErrorCode::Permission);
}

#[test]
fn folder_launches_fail_closed_without_an_enforcing_helper() {
    assert_eq!(grok_confinement(None, None).unwrap(), None);
    assert_eq!(grok_confinement(None, Some(helper())).unwrap(), None);
    assert_eq!(
        grok_confinement(Some(AcpFolderAccess::Conversation), None).unwrap(),
        None
    );
    for access in [
        AcpFolderAccess::Read,
        AcpFolderAccess::Edit,
        AcpFolderAccess::Commands,
    ] {
        let error = grok_confinement(Some(access), None).unwrap_err();
        assert_eq!(error.code, ChatErrorCode::Permission, "{access:?}");
        assert_eq!(
            grok_confinement(Some(access), Some(helper())).unwrap(),
            Some(helper())
        );
    }
    assert_eq!(
        grok_confinement(Some(AcpFolderAccess::Conversation), Some(helper())).unwrap(),
        Some(helper())
    );
}

#[test]
fn grok_claims_folder_boundaries_only_when_confined() {
    let unconfined = grok_authority_support(false);
    assert_eq!(
        unconfined,
        ProviderAuthoritySupport {
            isolated_conversation: true,
            internal_host_tools: true,
            ..ProviderAuthoritySupport::default()
        }
    );
    let confined = grok_authority_support(true);
    assert!(confined.deny_shell && confined.read_only_root && confined.writable_root);
    assert!(confined.confined_commands && confined.network_boundary);
    assert!(confined.classified_publish);
    assert!(!confined.person_approved_commands);
}

#[test]
fn the_grok_process_reads_its_home_and_folder_and_writes_only_its_own_state() {
    let paths = GrokPolicyPaths {
        home: Path::new("/home/person/.grok"),
        executable: Path::new("/home/person/.grok/downloads/grok"),
        workspace: Path::new("/home/person/Documents/Ganbaru/projects/site"),
    };
    let temporary = Path::new("/tmp/ganbaru-confined-a");
    let policy = grok_process_policy(&paths, temporary, Some(41234), |_| true).unwrap();
    assert!(policy.read.contains(&PathBuf::from("/home/person/.grok")));
    assert!(policy.read.contains(&PathBuf::from(paths.workspace)));
    for path in policy
        .read
        .iter()
        .chain(&policy.write)
        .chain(&policy.execute)
    {
        assert_ne!(path, Path::new("/home/person"));
        assert_ne!(path, Path::new("/home"));
        assert_ne!(path, Path::new("/"));
        assert!(!path.starts_with("/home/person/.ssh"));
        assert!(!path.starts_with("/home/person/Documents/Ganbaru/ganbaru.sqlite"));
    }
    assert_eq!(
        policy.write,
        vec![
            PathBuf::from("/home/person/.grok/sessions"),
            PathBuf::from("/home/person/.grok/logs"),
            PathBuf::from("/home/person/.grok/managed_config.lock"),
            PathBuf::from(temporary),
            PathBuf::from("/dev/null"),
        ]
    );
    assert!(
        !policy
            .write
            .iter()
            .any(|path| path.starts_with(paths.workspace))
    );
    assert_eq!(policy.execute, vec![PathBuf::from(paths.executable)]);
    assert_eq!(policy.network, NetworkAccess::TcpConnect(vec![443, 41234]));
}

#[test]
fn the_grok_process_policy_skips_missing_paths_and_refuses_folder_overlap() {
    let paths = GrokPolicyPaths {
        home: Path::new("/srv/grok"),
        executable: Path::new("/srv/grok/grok"),
        workspace: Path::new("/srv/work"),
    };
    let policy = grok_process_policy(&paths, Path::new("/tmp/t"), Some(443), |path| {
        !path.starts_with("/lib64") && path != Path::new("/srv/grok/managed_config.lock")
    })
    .unwrap();
    assert!(!policy.read.contains(&PathBuf::from("/lib64")));
    assert!(
        !policy
            .write
            .contains(&PathBuf::from("/srv/grok/managed_config.lock"))
    );
    assert_eq!(policy.network, NetworkAccess::TcpConnect(vec![443]));

    let inside = GrokPolicyPaths {
        home: Path::new("/srv/work/.grok"),
        ..paths
    };
    assert!(grok_process_policy(&inside, Path::new("/tmp/t"), None, |_| true).is_err());
    let around = GrokPolicyPaths {
        workspace: Path::new("/tmp/t/work"),
        ..paths
    };
    assert!(grok_process_policy(&around, Path::new("/tmp/t"), None, |_| true).is_err());
    let enclosing = GrokPolicyPaths {
        home: Path::new("/srv"),
        ..paths
    };
    assert!(grok_process_policy(&enclosing, Path::new("/tmp/t"), None, |_| true).is_err());
}

#[test]
fn a_grok_home_that_holds_the_home_directory_is_refused() {
    let user_home = crate::probe_directory::ProbeDirectory::create().unwrap();
    let environment = |grok: Option<&Path>| {
        let mut environment = std::collections::BTreeMap::new();
        environment.insert("HOME".to_string(), user_home.display().to_string());
        if let Some(grok) = grok {
            environment.insert("GROK_HOME".to_string(), grok.display().to_string());
        }
        environment
    };
    let canonical = std::fs::canonicalize(&*user_home).unwrap();
    assert_eq!(
        grok_home(&environment(None)).unwrap(),
        canonical.join(".grok")
    );
    let dedicated = user_home.join("tools/grok");
    assert_eq!(
        grok_home(&environment(Some(&dedicated))).unwrap(),
        canonical.join("tools/grok")
    );
    for enclosing in [user_home.to_path_buf(), user_home.join("..")] {
        assert!(grok_home(&environment(Some(&enclosing))).is_err());
    }
}

fn session(
    flavor: AcpProviderFlavor,
    scope: Option<ProviderOrganizationalScope>,
    workspace: &Path,
    helper: Option<&'static SandboxHelper>,
) -> ChatResult<Option<AcpCallbackConfinement>> {
    AcpCallbackConfinement::with_helper(flavor, Some(&internal_mcp(scope)), workspace, helper)
}

fn refusal_code(error: ChatError) -> String {
    assert_eq!(error.code, ChatErrorCode::Permission, "{}", error.message);
    error
        .message
        .split(':')
        .next()
        .unwrap_or_default()
        .to_string()
}

#[test]
fn unconfined_sessions_keep_the_plain_workspace_boundary() {
    let workspace = crate::probe_directory::ProbeDirectory::create().unwrap();
    let commands = Some(folder(ProviderFolderCapability::Execute));
    assert!(
        session(
            AcpProviderFlavor::Generic,
            commands,
            &workspace,
            Some(helper())
        )
        .unwrap()
        .is_none()
    );
    assert!(
        session(
            AcpProviderFlavor::Cursor,
            Some(ProviderOrganizationalScope::Conversation),
            &workspace,
            None
        )
        .unwrap()
        .is_none()
    );
    assert!(
        AcpCallbackConfinement::with_helper(
            AcpProviderFlavor::Grok,
            None,
            &workspace,
            Some(helper())
        )
        .unwrap()
        .is_none()
    );
    assert!(
        session(
            AcpProviderFlavor::Grok,
            Some(ProviderOrganizationalScope::Conversation),
            &workspace,
            None
        )
        .unwrap()
        .is_none()
    );
    let Err(error) = session(AcpProviderFlavor::Grok, commands, &workspace, None) else {
        panic!("a folder session started without the sandbox");
    };
    assert_eq!(error.code, ChatErrorCode::Permission);
}

#[test]
fn read_and_edit_sessions_never_offer_or_run_commands() {
    let workspace = crate::probe_directory::ProbeDirectory::create().unwrap();
    let conversation = session(
        AcpProviderFlavor::Grok,
        Some(ProviderOrganizationalScope::Conversation),
        &workspace,
        Some(helper()),
    )
    .unwrap()
    .unwrap();
    let read = session(
        AcpProviderFlavor::Grok,
        Some(folder(ProviderFolderCapability::Read)),
        &workspace,
        Some(helper()),
    )
    .unwrap()
    .unwrap();
    let edit = session(
        AcpProviderFlavor::Grok,
        Some(folder(ProviderFolderCapability::Edit)),
        &workspace,
        Some(helper()),
    )
    .unwrap()
    .unwrap();
    for confinement in [&conversation, &read, &edit] {
        assert!(!confinement.offers_terminal());
        assert!(confinement.git_metadata_tripped().is_none());
        assert_eq!(
            refusal_code(confinement.terminal_sandbox().err().unwrap()),
            "acp_command_outside_scope"
        );
    }
    for confinement in [&conversation, &read] {
        assert_eq!(
            refusal_code(confinement.verify_write().unwrap_err()),
            "acp_write_outside_scope"
        );
    }
    edit.verify_write().unwrap();
}

#[tokio::test]
async fn a_changed_git_folder_ends_the_session_and_refuses_later_commands() {
    let workspace = crate::probe_directory::ProbeDirectory::create().unwrap();
    std::fs::create_dir_all(workspace.join(".git/hooks")).unwrap();
    std::fs::write(workspace.join(".git/config"), "[core]\n").unwrap();
    let confinement = session(
        AcpProviderFlavor::Grok,
        Some(folder(ProviderFolderCapability::Execute)),
        &workspace,
        Some(helper()),
    )
    .unwrap()
    .unwrap();
    assert!(confinement.offers_terminal());
    confinement.verify_write().unwrap();
    confinement.terminal_sandbox().unwrap();
    let tripped = confinement.git_metadata_tripped().unwrap();

    std::fs::write(workspace.join(".git/hooks/pre-commit"), "#!/bin/sh\n").unwrap();
    assert_eq!(
        refusal_code(confinement.verify_git_metadata().unwrap_err()),
        "acp_git_metadata_changed"
    );
    tokio::time::timeout(std::time::Duration::from_secs(5), tripped.notified())
        .await
        .expect("the trip signals the session router");

    // Restoring the files does not clear the trip.
    std::fs::remove_file(workspace.join(".git/hooks/pre-commit")).unwrap();
    assert_eq!(
        refusal_code(confinement.terminal_sandbox().err().unwrap()),
        "acp_git_metadata_changed"
    );
    assert_eq!(
        refusal_code(confinement.verify_write().unwrap_err()),
        "acp_git_metadata_changed"
    );
}

#[test]
fn cursor_sessions_never_run_commands_and_need_the_helper_for_a_folder() {
    let workspace = crate::probe_directory::ProbeDirectory::create().unwrap();
    for helper in [None, Some(helper())] {
        let Err(error) = session(
            AcpProviderFlavor::Cursor,
            Some(folder(ProviderFolderCapability::Execute)),
            &workspace,
            helper,
        ) else {
            panic!("a Cursor session was offered commands");
        };
        assert_eq!(refusal_code(error), "acp_command_outside_scope");
    }
    for capability in [
        ProviderFolderCapability::Read,
        ProviderFolderCapability::Edit,
    ] {
        let Err(error) = session(
            AcpProviderFlavor::Cursor,
            Some(folder(capability)),
            &workspace,
            None,
        ) else {
            panic!("a Cursor folder session started without the sandbox");
        };
        assert_eq!(error.code, ChatErrorCode::Permission);
    }
}

#[tokio::test]
async fn a_cursor_edit_session_watches_its_direct_writes_to_folder_configuration() {
    let workspace = crate::probe_directory::ProbeDirectory::create().unwrap();
    std::fs::create_dir_all(workspace.join(".git/hooks")).unwrap();
    std::fs::create_dir_all(workspace.join(".cursor")).unwrap();
    let read = session(
        AcpProviderFlavor::Cursor,
        Some(folder(ProviderFolderCapability::Read)),
        &workspace,
        Some(helper()),
    )
    .unwrap()
    .unwrap();
    assert!(!read.watches_direct_writes());
    assert!(!read.offers_terminal());
    let edit = session(
        AcpProviderFlavor::Cursor,
        Some(folder(ProviderFolderCapability::Edit)),
        &workspace,
        Some(helper()),
    )
    .unwrap()
    .unwrap();
    assert!(edit.watches_direct_writes());
    assert!(!edit.offers_terminal());
    assert_eq!(
        refusal_code(edit.terminal_sandbox().err().unwrap()),
        "acp_command_outside_scope"
    );
    let tripped = edit.git_metadata_tripped().unwrap();

    std::fs::write(workspace.join("README.md"), "ordinary edit").unwrap();
    std::fs::write(workspace.join(".cursor/notes.md"), "not command bearing").unwrap();
    edit.verify_git_metadata().unwrap();

    std::fs::write(
        workspace.join(".cursor/mcp.json"),
        r#"{"mcpServers":{"x":{"command":"sh"}}}"#,
    )
    .unwrap();
    assert_eq!(
        refusal_code(edit.verify_git_metadata().unwrap_err()),
        "acp_git_metadata_changed"
    );
    tokio::time::timeout(std::time::Duration::from_secs(5), tripped.notified())
        .await
        .expect("the trip signals the session router");
}
