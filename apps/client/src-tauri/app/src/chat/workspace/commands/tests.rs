use super::{
    FolderSelectionReceipt, MAX_PENDING_FOLDER_SELECTIONS, MessageDialogResult,
    ProjectWorkingFolderSelections, TrustAnswer, UndecidedProjectConfiguration,
    WorkingFolderDeviceScope, WorkingFolderKind, WorkingFolderTrustPrompt,
    preferred_workspace_picker_directory, resolve_loaded_project_configuration,
    resolve_project_configuration, trust_answer, validate_archive_change, validate_trust_prompt,
};
use ganbaru_chat::git::hardened::{
    ProviderConfigurationEntries, provider_configuration_entries_blocking,
};
use ganbaru_chat_contracts::models::{
    ChatError, ChatErrorCode, ProjectConfigurationAccess, ProjectConfigurationRestriction,
    ProjectWorkingFolderId, ProviderConfigurationPath, UtcTimestamp,
};
use ganbaru_working_folders::WorkingFolderTrustRecord;
use std::fs;
use std::path::{Path, PathBuf};

/// A folder holding provider configuration files, removed when dropped.
struct ConfigurationFolder(PathBuf);

impl ConfigurationFolder {
    fn with_files(files: &[&str]) -> Self {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("test clock should be valid")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "ganbaru-chat-project-configuration-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&root).expect("test folder should be created");
        for relative in files {
            let path = root.join(relative);
            fs::create_dir_all(path.parent().expect("file should have a parent"))
                .expect("file folder should be created");
            fs::write(path, "{}").expect("configuration should write");
        }
        Self(root)
    }

    fn entries(&self) -> ProviderConfigurationEntries {
        provider_configuration_entries_blocking(&self.0).expect("entries should read")
    }
}

impl Drop for ConfigurationFolder {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn trust_folder_id() -> ProjectWorkingFolderId {
    ProjectWorkingFolderId::new("working-folder").unwrap()
}

/// A device scope holding a decision about `entries`.
fn scope_deciding(
    entries: &ProviderConfigurationEntries,
    trusted: bool,
) -> WorkingFolderDeviceScope {
    let mut scope = WorkingFolderDeviceScope::default();
    scope.trust.insert(
        trust_folder_id(),
        WorkingFolderTrustRecord {
            git_configuration_digest: String::new(),
            provider_configuration_digest: entries.digest().to_hex(),
            trusted,
            decided_at: UtcTimestamp::new("2026-10-18T12:00:00.000Z").unwrap(),
        },
    );
    scope
}

fn assert_refused_naming(error: ChatError, file: &str) {
    assert_eq!(error.code, ChatErrorCode::TrustDecisionRequired);
    assert!(
        error.message.contains(file),
        "{file} must be named in: {}",
        error.message
    );
    let details = error.details.expect("the error must name the folder");
    assert_eq!(details["workingFolderId"], trust_folder_id().as_str());
}

#[test]
fn restricted_run_refuses_unenforced_provider_with_configuration() {
    for file in [".mcp.json", "opencode.json", ".cursor/mcp.json"] {
        let folder = ConfigurationFolder::with_files(&[file]);
        let entries = folder.entries();

        let unattended = resolve_project_configuration(
            &WorkingFolderDeviceScope::default(),
            &trust_folder_id(),
            &entries,
            &entries,
            UndecidedProjectConfiguration::Restrict,
            ProjectConfigurationRestriction::Unavailable,
        );
        assert_refused_naming(
            unattended.expect_err("an unattended run must be refused"),
            file,
        );

        let declined = resolve_project_configuration(
            &scope_deciding(&entries, false),
            &trust_folder_id(),
            &entries,
            &entries,
            UndecidedProjectConfiguration::Ask,
            ProjectConfigurationRestriction::Unavailable,
        );
        assert_refused_naming(
            declined.expect_err("a declined folder must be refused"),
            file,
        );
    }
}

#[test]
fn restricted_run_refuses_exceeded_configuration_only_for_providers_loading_it() {
    const CLAUDE: &[ProviderConfigurationPath] = &[
        ProviderConfigurationPath::ClaudeSettings,
        ProviderConfigurationPath::ClaudeSkills,
    ];
    const OPENCODE: &[ProviderConfigurationPath] = &[
        ProviderConfigurationPath::OpenCodeJson,
        ProviderConfigurationPath::OpenCode,
    ];
    let folder = ConfigurationFolder::with_files(&[
        ".claude/settings.json",
        ".opencode/node_modules/a/b/c/d/e/f/g/h/index.js",
    ]);
    let entries = folder.entries();
    let resolve = |scope: &WorkingFolderDeviceScope,
                   undecided: UndecidedProjectConfiguration,
                   restriction: ProjectConfigurationRestriction,
                   reads: &[ProviderConfigurationPath]| {
        resolve_loaded_project_configuration(
            scope,
            &trust_folder_id(),
            &entries,
            &entries,
            undecided,
            restriction,
            reads,
        )
    };
    let trusted = scope_deciding(&entries, true);

    for scope in [&trusted, &WorkingFolderDeviceScope::default()] {
        let error = resolve(
            scope,
            UndecidedProjectConfiguration::Ask,
            ProjectConfigurationRestriction::Unavailable,
            OPENCODE,
        )
        .expect_err("OpenCode must not load an unchecked tree");
        assert_eq!(error.code, ChatErrorCode::ConfigurationInvalid);
        assert!(error.message.contains(".opencode"), "{}", error.message);
        assert!(error.details.is_none(), "trusting cannot settle it");
    }
    assert_eq!(
        resolve(
            &trusted,
            UndecidedProjectConfiguration::Ask,
            ProjectConfigurationRestriction::Enforced,
            CLAUDE,
        )
        .expect("Claude does not load the oversized tree"),
        ProjectConfigurationAccess::Trusted
    );
    assert_eq!(
        resolve(
            &WorkingFolderDeviceScope::default(),
            UndecidedProjectConfiguration::Restrict,
            ProjectConfigurationRestriction::Enforced,
            ProviderConfigurationPath::ALL,
        )
        .expect("a restricted enforcing start loads no project configuration"),
        ProjectConfigurationAccess::Restricted
    );
    assert_eq!(
        resolve(
            &trusted,
            UndecidedProjectConfiguration::Ask,
            ProjectConfigurationRestriction::Enforced,
            ProviderConfigurationPath::ALL,
        )
        .expect_err("a trusted start loading the tree must be refused")
        .code,
        ChatErrorCode::ConfigurationInvalid
    );
}

#[test]
fn restricted_run_refuses_worktree_configuration_the_trusted_folder_lacks() {
    let root = ConfigurationFolder::with_files(&[".mcp.json"]);
    let worktree = ConfigurationFolder::with_files(&[".mcp.json", "opencode.json"]);
    let result = resolve_project_configuration(
        &scope_deciding(&root.entries(), true),
        &trust_folder_id(),
        &root.entries(),
        &worktree.entries(),
        UndecidedProjectConfiguration::Restrict,
        ProjectConfigurationRestriction::Unavailable,
    );
    assert_worktree_refusal(
        result.expect_err("an uncovered worktree must be refused"),
        "opencode.json",
        Some(".mcp.json"),
    );
}

#[test]
fn restricted_run_keeps_folder_guidance_for_a_covered_worktree() {
    let root = ConfigurationFolder::with_files(&[".cursor/mcp.json"]);
    let worktree = ConfigurationFolder::with_files(&[".cursor/mcp.json"]);
    let result = resolve_project_configuration(
        &scope_deciding(&root.entries(), false),
        &trust_folder_id(),
        &root.entries(),
        &worktree.entries(),
        UndecidedProjectConfiguration::Ask,
        ProjectConfigurationRestriction::Unavailable,
    );
    assert_refused_naming(
        result.expect_err("a declined folder must be refused"),
        ".cursor/mcp.json",
    );
}

#[test]
fn restricted_run_still_asks_for_an_enforced_provider_in_an_uncovered_worktree() {
    let root = ConfigurationFolder::with_files(&[".mcp.json"]);
    let worktree = ConfigurationFolder::with_files(&[".mcp.json", ".claude/settings.json"]);
    let error = resolve_project_configuration(
        &WorkingFolderDeviceScope::default(),
        &trust_folder_id(),
        &root.entries(),
        &worktree.entries(),
        UndecidedProjectConfiguration::Ask,
        ProjectConfigurationRestriction::Enforced,
    )
    .expect_err("an interactive start must ask");
    assert!(error.details.is_some(), "the folder must stay decidable");
}

/// A refusal only worktree changes can settle: it names the worktree and
/// the uncovered file, and leaves no folder for the caller to ask about.
fn assert_worktree_refusal(error: ChatError, uncovered: &str, covered: Option<&str>) {
    assert_eq!(error.code, ChatErrorCode::TrustDecisionRequired);
    assert!(
        error.message.contains("worktree"),
        "the worktree must be named in: {}",
        error.message
    );
    assert!(
        error.message.contains(uncovered),
        "{uncovered} must be named in: {}",
        error.message
    );
    if let Some(covered) = covered {
        assert!(
            !error.message.contains(covered),
            "{covered} must not be named in: {}",
            error.message
        );
    }
    assert!(
        !error.message.contains("Project settings"),
        "a trust action cannot help: {}",
        error.message
    );
    assert!(
        error.details.is_none(),
        "a folder decision cannot settle this refusal"
    );
}

#[test]
fn worktree_refusal_names_the_worktree() {
    let trusted_root = ConfigurationFolder::with_files(&[".mcp.json"]);
    let added = ConfigurationFolder::with_files(&[".mcp.json", ".cursor/mcp.json"]);
    for undecided in [
        UndecidedProjectConfiguration::Ask,
        UndecidedProjectConfiguration::Restrict,
    ] {
        let refused = resolve_project_configuration(
            &scope_deciding(&trusted_root.entries(), true),
            &trust_folder_id(),
            &trusted_root.entries(),
            &added.entries(),
            undecided,
            ProjectConfigurationRestriction::Unavailable,
        );
        assert_worktree_refusal(
            refused.expect_err("an uncovered worktree must be refused"),
            ".cursor/mcp.json",
            Some(".mcp.json"),
        );
    }

    let empty_root = ConfigurationFolder::with_files(&[]);
    let refused = resolve_project_configuration(
        &WorkingFolderDeviceScope::default(),
        &trust_folder_id(),
        &empty_root.entries(),
        &added.entries(),
        UndecidedProjectConfiguration::Ask,
        ProjectConfigurationRestriction::Unavailable,
    );
    assert_worktree_refusal(
        refused.expect_err("a root without configuration covers nothing"),
        ".cursor/mcp.json",
        None,
    );

    let undecided = resolve_project_configuration(
        &WorkingFolderDeviceScope::default(),
        &trust_folder_id(),
        &trusted_root.entries(),
        &added.entries(),
        UndecidedProjectConfiguration::Ask,
        ProjectConfigurationRestriction::Unavailable,
    );
    assert_worktree_refusal(
        undecided.expect_err("no folder decision can cover this worktree"),
        ".cursor/mcp.json",
        None,
    );
}

#[test]
fn restricted_run_allows_unenforced_provider_without_configuration() {
    let empty = ConfigurationFolder::with_files(&[]);
    for undecided in [
        UndecidedProjectConfiguration::Ask,
        UndecidedProjectConfiguration::Restrict,
    ] {
        assert_eq!(
            resolve_project_configuration(
                &WorkingFolderDeviceScope::default(),
                &trust_folder_id(),
                &empty.entries(),
                &empty.entries(),
                undecided,
                ProjectConfigurationRestriction::Unavailable,
            )
            .expect("a folder without configuration must run"),
            ProjectConfigurationAccess::Trusted
        );
    }

    let root = ConfigurationFolder::with_files(&[".mcp.json"]);
    assert_eq!(
        resolve_project_configuration(
            &WorkingFolderDeviceScope::default(),
            &trust_folder_id(),
            &root.entries(),
            &empty.entries(),
            UndecidedProjectConfiguration::Restrict,
            ProjectConfigurationRestriction::Unavailable,
        )
        .expect("a launch folder without configuration must run"),
        ProjectConfigurationAccess::Restricted
    );
}

#[test]
fn restricted_run_keeps_enforced_provider_with_configuration() {
    let folder = ConfigurationFolder::with_files(&[".mcp.json"]);
    let entries = folder.entries();
    assert_eq!(
        resolve_project_configuration(
            &WorkingFolderDeviceScope::default(),
            &trust_folder_id(),
            &entries,
            &entries,
            UndecidedProjectConfiguration::Restrict,
            ProjectConfigurationRestriction::Enforced,
        )
        .expect("an enforced provider must start restricted"),
        ProjectConfigurationAccess::Restricted
    );
}

#[test]
fn restricted_run_asks_before_refusing_an_undecided_folder() {
    let folder = ConfigurationFolder::with_files(&[".mcp.json"]);
    let entries = folder.entries();
    let error = resolve_project_configuration(
        &WorkingFolderDeviceScope::default(),
        &trust_folder_id(),
        &entries,
        &entries,
        UndecidedProjectConfiguration::Ask,
        ProjectConfigurationRestriction::Unavailable,
    )
    .expect_err("an interactive start must ask");
    assert_eq!(error.code, ChatErrorCode::TrustDecisionRequired);
    assert!(!error.message.contains(".mcp.json"));
}

#[test]
fn trusted_folder_runs_cursor_with_configuration() {
    let folder = ConfigurationFolder::with_files(&[".cursor/mcp.json", "opencode.json"]);
    let entries = folder.entries();
    assert_eq!(
        resolve_project_configuration(
            &scope_deciding(&entries, true),
            &trust_folder_id(),
            &entries,
            &entries,
            UndecidedProjectConfiguration::Ask,
            ProjectConfigurationRestriction::Unavailable,
        )
        .expect("a trusted folder must run"),
        ProjectConfigurationAccess::Trusted
    );
}

#[test]
fn staged_folder_selections_are_bound_to_the_project_target_and_vault() {
    let selections = ProjectWorkingFolderSelections::default();
    let target = ProjectWorkingFolderId::new("external-folder").unwrap();
    selections
        .insert(FolderSelectionReceipt {
            id: "selection".to_string(),
            project_id: "project".to_string(),
            working_folder_id: Some(target.clone()),
            vault_root: "vault".into(),
            path: "chosen".into(),
            filesystem_identity: "identity".to_string(),
        })
        .unwrap();
    assert!(
        selections
            .read("selection", "project", Some(&target), Path::new("vault"))
            .is_ok()
    );
    assert!(
        selections
            .read("unknown", "project", Some(&target), Path::new("vault"))
            .is_err()
    );
    assert!(
        selections
            .read(
                "selection",
                "another-project",
                Some(&target),
                Path::new("vault")
            )
            .is_err()
    );
    assert!(
        selections
            .read("selection", "project", None, Path::new("vault"))
            .is_err()
    );
    assert!(
        selections
            .read(
                "selection",
                "project",
                Some(&ProjectWorkingFolderId::new("other").unwrap()),
                Path::new("vault")
            )
            .is_err()
    );
    assert!(
        selections
            .read(
                "selection",
                "project",
                Some(&target),
                Path::new("another-vault")
            )
            .is_err()
    );
    assert!(
        selections
            .read("selection", "project", Some(&target), Path::new("vault"))
            .is_ok()
    );
}

#[test]
fn native_selection_receipts_are_bounded_and_oldest_selections_expire() {
    let selections = ProjectWorkingFolderSelections::default();
    for index in 0..=MAX_PENDING_FOLDER_SELECTIONS {
        selections
            .insert(FolderSelectionReceipt {
                id: index.to_string(),
                project_id: "project".to_string(),
                working_folder_id: None,
                vault_root: "vault".into(),
                path: "chosen".into(),
                filesystem_identity: "identity".to_string(),
            })
            .unwrap();
    }
    assert!(
        selections
            .read("0", "project", None, Path::new("vault"))
            .is_err()
    );
    assert!(
        selections
            .read(
                &MAX_PENDING_FOLDER_SELECTIONS.to_string(),
                "project",
                None,
                Path::new("vault")
            )
            .is_ok()
    );
}

#[test]
fn workspace_picker_prefers_the_existing_bound_folder() {
    let current = std::env::current_dir().expect("read current directory");
    let documents = std::env::temp_dir();

    assert_eq!(
        preferred_workspace_picker_directory(Some(&current), Some(&documents)),
        Some(current),
    );
}

#[test]
fn workspace_picker_falls_back_to_documents_without_a_binding() {
    let documents = std::env::temp_dir();

    assert_eq!(
        preferred_workspace_picker_directory(None, Some(&documents)),
        Some(documents),
    );
    assert_eq!(
        preferred_workspace_picker_directory(None, Some(Path::new("missing-documents"))),
        None,
    );
}

fn trust_prompt() -> WorkingFolderTrustPrompt {
    WorkingFolderTrustPrompt {
        title: "Trust this folder?".to_string(),
        message: "Hooks can run programs.\nDecide once.".to_string(),
        trust_label: "Trust".to_string(),
        untrusted_label: "Run without hooks".to_string(),
        cancel_label: "Cancel".to_string(),
    }
}

#[test]
fn trust_prompts_accept_bounded_multiline_text() {
    assert!(validate_trust_prompt(&trust_prompt()).is_ok());
}

#[test]
fn trust_prompts_reject_blank_oversized_or_control_text() {
    let mut blank = trust_prompt();
    blank.title = "  ".to_string();
    assert!(validate_trust_prompt(&blank).is_err());

    let mut oversized = trust_prompt();
    oversized.trust_label = "x".repeat(81);
    assert!(validate_trust_prompt(&oversized).is_err());

    let mut control = trust_prompt();
    control.message = "Trust\u{1b}[2J".to_string();
    assert!(validate_trust_prompt(&control).is_err());
}

#[test]
fn trust_prompts_need_distinct_buttons() {
    let mut duplicate = trust_prompt();
    duplicate.cancel_label = " Trust ".to_string();
    assert!(validate_trust_prompt(&duplicate).is_err());
}

#[test]
fn trust_answers_map_labels_and_treat_anything_else_as_cancel() {
    let answer = |result| trust_answer(&result, "Trust", "Run without hooks");
    assert_eq!(answer(MessageDialogResult::Yes), TrustAnswer::Trust);
    assert_eq!(answer(MessageDialogResult::No), TrustAnswer::Untrusted);
    assert_eq!(
        answer(MessageDialogResult::Custom("Trust".to_string())),
        TrustAnswer::Trust
    );
    assert_eq!(
        answer(MessageDialogResult::Custom("Run without hooks".to_string())),
        TrustAnswer::Untrusted
    );
    assert_eq!(
        answer(MessageDialogResult::Custom("Cancel".to_string())),
        TrustAnswer::Cancel
    );
    assert_eq!(answer(MessageDialogResult::Cancel), TrustAnswer::Cancel);
    assert_eq!(answer(MessageDialogResult::Ok), TrustAnswer::Cancel);
}

#[test]
fn archiving_a_working_folder_stops_its_terminals() {
    let source = include_str!("../commands.rs");
    let start = source
        .find("async fn set_workspace_archived(")
        .expect("archive command exists");
    let end = start + source[start..].find("\n}\n").expect("archive command ends");
    let archived_branch = &source[start..end];
    let archived_branch = &archived_branch[archived_branch
        .find("if archived {")
        .expect("archive has an archived branch")..];
    assert!(
        archived_branch.contains(".shutdown_workspace(&working_folder_id)?;"),
        "archiving a folder must stop the terminals started in it"
    );
}

#[test]
fn managed_folders_refuse_archive_but_allow_restore() {
    let refused = validate_archive_change(WorkingFolderKind::Managed, true)
        .expect_err("archiving a managed folder is refused");
    assert_eq!(refused.code, ChatErrorCode::Validation);
    assert!(
        validate_archive_change(WorkingFolderKind::Managed, false).is_ok(),
        "restoring an archived managed folder must stay allowed"
    );
}

#[test]
fn external_folders_allow_archive_and_restore() {
    assert!(validate_archive_change(WorkingFolderKind::External, true).is_ok());
    assert!(validate_archive_change(WorkingFolderKind::External, false).is_ok());
}
