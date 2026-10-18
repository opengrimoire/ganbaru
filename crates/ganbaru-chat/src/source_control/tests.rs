use super::*;

#[test]
fn detects_supported_https_and_ssh_remotes() {
    assert_eq!(
        detect_remote("git@github.com:owner/project.git"),
        Some((HostedSourceControlKind::Github, "owner/project".to_string()))
    );
    assert_eq!(
        detect_remote("https://gitlab.com/group/nested/project.git"),
        Some((
            HostedSourceControlKind::Gitlab,
            "group/nested/project".to_string()
        ))
    );
    assert_eq!(
        detect_remote("https://bitbucket.org/team/project.git"),
        Some((
            HostedSourceControlKind::Bitbucket,
            "team/project".to_string()
        ))
    );
}

#[test]
fn normalizes_change_requests_from_each_cli_shape() {
    let github = parse_change_request_list(
        HostedSourceControlKind::Github,
        r#"[{"number":17,"title":"Improve chat","url":"https://github.com/o/r/pull/17","state":"OPEN","baseRefName":"dev","headRefName":"feature","author":{"login":"alice"},"isDraft":true}]"#,
    )
    .unwrap();
    assert_eq!(github[0].number, 17);
    assert_eq!(github[0].author.as_deref(), Some("alice"));
    assert!(github[0].draft);

    let gitlab = parse_change_request_list(
        HostedSourceControlKind::Gitlab,
        r#"[{"iid":4,"title":"Improve chat","web_url":"https://gitlab.com/o/r/-/merge_requests/4","state":"opened","target_branch":"dev","source_branch":"feature","author":{"username":"alice"}}]"#,
    )
    .unwrap();
    assert_eq!(gitlab[0].url, "https://gitlab.com/o/r/-/merge_requests/4");

    let azure = parse_change_request_list(
        HostedSourceControlKind::AzureDevops,
        r#"[{"pullRequestId":8,"title":"Improve chat","url":"https://api.invalid/8","status":"active","targetRefName":"refs/heads/dev","sourceRefName":"refs/heads/feature","createdBy":{"displayName":"Alice"},"repository":{"webUrl":"https://dev.azure.com/o/p/_git/r"}}]"#,
    )
    .unwrap();
    assert_eq!(azure[0].base_branch, "dev");
    assert_eq!(
        azure[0].url,
        "https://dev.azure.com/o/p/_git/r/pullrequest/8"
    );
}

#[test]
fn source_control_arguments_keep_values_as_distinct_process_arguments() {
    let request = CreateHostedChangeRequest {
        working_folder_id: ProjectWorkingFolderId::new("workspace").unwrap(),
        execution_environment_id: None,
        provider_kind: HostedSourceControlKind::Github,
        repository_slug: "owner/repository".to_string(),
        title: "Title with shell syntax $(ignored)".to_string(),
        body: "Body with `literal text`".to_string(),
        base_branch: "dev".to_string(),
        head_branch: "feature/chat".to_string(),
        draft: false,
    };
    let arguments = create_arguments(&request, None).unwrap();
    assert!(arguments.contains(&request.title));
    assert!(arguments.contains(&request.body));
}

#[test]
fn normalizes_bitbucket_change_requests() {
    let change_request = parse_bitbucket_change_request(&serde_json::json!({
        "id": 23,
        "title": "Improve chat",
        "state": "OPEN",
        "destination": { "branch": { "name": "dev" } },
        "source": { "branch": { "name": "feature/chat" } },
        "author": { "display_name": "Alice" },
        "links": { "html": { "href": "https://bitbucket.org/team/project/pull-requests/23" } },
        "draft": true
    }))
    .unwrap();

    assert_eq!(change_request.number, 23);
    assert_eq!(change_request.base_branch, "dev");
    assert_eq!(change_request.head_branch, "feature/chat");
    assert_eq!(change_request.author.as_deref(), Some("Alice"));
    assert!(change_request.draft);
}

#[test]
fn encodes_bitbucket_repository_path_segments() {
    assert_eq!(percent_encode_segment("team name"), "team%20name");
    assert_eq!(percent_encode_segment("project/name"), "project%2Fname");
    assert_eq!(percent_encode_segment("safe._-~"), "safe._-~");
}

#[test]
fn parses_azure_repository_remote_shapes() {
    let expected = |organization_url: &str, project: &str, repository: &str| {
        Some(AzureRepository {
            organization_url: organization_url.to_string(),
            project: project.to_string(),
            repository: repository.to_string(),
        })
    };
    assert_eq!(
        azure_repository("https://contoso@dev.azure.com/contoso/Fabrikam%20Web/_git/portal"),
        expected("https://dev.azure.com/contoso", "Fabrikam Web", "portal")
    );
    assert_eq!(
        azure_repository("git@ssh.dev.azure.com:v3/contoso/Fabrikam/portal"),
        expected("https://dev.azure.com/contoso", "Fabrikam", "portal")
    );
    assert_eq!(
        azure_repository("ssh://git@ssh.dev.azure.com/v3/contoso/Fabrikam/portal"),
        expected("https://dev.azure.com/contoso", "Fabrikam", "portal")
    );
    assert_eq!(
        azure_repository("contoso@vs-ssh.visualstudio.com:v3/contoso/Fabrikam/portal"),
        expected("https://dev.azure.com/contoso", "Fabrikam", "portal")
    );
    assert_eq!(
        azure_repository("https://contoso.visualstudio.com/Fabrikam/_git/portal.git"),
        expected("https://contoso.visualstudio.com", "Fabrikam", "portal")
    );
    assert_eq!(
        azure_repository("https://contoso.visualstudio.com/DefaultCollection/Fabrikam/_git/portal"),
        expected("https://contoso.visualstudio.com", "Fabrikam", "portal")
    );
    for rejected in [
        "https://github.com/owner/project.git",
        "https://dev.azure.com/contoso/Fabrikam/portal",
        "https://dev.azure.com/contoso/Fabrikam/_git/-portal",
        "https://dev.azure.com/con%2Ftoso/Fabrikam/_git/portal",
        "https://dev.azure.com/contoso/Fab%0Arikam/_git/portal",
        "https://dev.azure.com/contoso/Fabrikam/_git/por%ZZtal",
    ] {
        assert_eq!(azure_repository(rejected), None, "{rejected}");
    }
    assert_eq!(
        detect_remote("git@ssh.dev.azure.com:v3/contoso/Fabrikam/portal"),
        Some((
            HostedSourceControlKind::AzureDevops,
            "v3/contoso/Fabrikam/portal".to_string()
        ))
    );
}

#[test]
fn azure_arguments_name_the_repository_instead_of_detecting_it() {
    let azure = AzureRepository {
        organization_url: "https://dev.azure.com/contoso".to_string(),
        project: "Fabrikam".to_string(),
        repository: "portal".to_string(),
    };
    let list = list_arguments(
        HostedSourceControlKind::AzureDevops,
        "contoso/Fabrikam/_git/portal",
        Some(&azure),
        20,
    )
    .unwrap();
    let request = CreateHostedChangeRequest {
        working_folder_id: ProjectWorkingFolderId::new("workspace").unwrap(),
        execution_environment_id: None,
        provider_kind: HostedSourceControlKind::AzureDevops,
        repository_slug: "contoso/Fabrikam/_git/portal".to_string(),
        title: "Title".to_string(),
        body: "Body".to_string(),
        base_branch: "main".to_string(),
        head_branch: "feature".to_string(),
        draft: false,
    };
    let create = create_arguments(&request, Some(&azure)).unwrap();
    let view = view_arguments(
        HostedSourceControlKind::AzureDevops,
        &request.repository_slug,
        Some(&azure),
        "8",
    )
    .unwrap();
    for arguments in [&list, &create, &view] {
        let detect = arguments
            .iter()
            .position(|argument| argument == "--detect")
            .expect("detection should be named");
        assert_eq!(arguments[detect + 1], "false");
        let organization = arguments
            .iter()
            .position(|argument| argument == "--organization")
            .expect("organization should be explicit");
        assert_eq!(arguments[organization + 1], azure.organization_url);
    }
    for arguments in [&list, &create] {
        assert!(arguments.contains(&"--project".to_string()));
        assert!(arguments.contains(&"--repository".to_string()));
    }
    assert!(
        list_arguments(
            HostedSourceControlKind::AzureDevops,
            "contoso/Fabrikam/_git/portal",
            None,
            20
        )
        .is_err()
    );
}

#[test]
fn hosted_cli_operations_leave_the_folder_unless_git_context_is_needed() {
    for kind in [
        HostedSourceControlKind::Github,
        HostedSourceControlKind::Gitlab,
        HostedSourceControlKind::AzureDevops,
    ] {
        assert!(!HostedCliOperation::Read.runs_in_repository(kind));
        assert!(HostedCliOperation::Checkout.runs_in_repository(kind));
    }
    assert!(HostedCliOperation::Create.runs_in_repository(HostedSourceControlKind::Github));
    assert!(HostedCliOperation::Create.runs_in_repository(HostedSourceControlKind::Gitlab));
    assert!(!HostedCliOperation::Create.runs_in_repository(HostedSourceControlKind::AzureDevops));
}

#[test]
fn hosted_cli_commands_carry_git_hardening_and_python_isolation() {
    let directory = cli::EmptyWorkingDirectory::create().unwrap();
    let environment = crate::git::hardened::child_environment_blocking(
        &directory.0,
        crate::git::hardened::GitPolicy::internal(crate::git::hardened::GitFolderTrust::Trusted),
    )
    .unwrap();
    for kind in [
        HostedSourceControlKind::Github,
        HostedSourceControlKind::Gitlab,
        HostedSourceControlKind::AzureDevops,
    ] {
        let command =
            hosted_cli_command(kind, &["--version".to_string()], &directory.0, &environment)
                .unwrap();
        assert_eq!(command.get_current_dir(), Some(directory.0.as_path()));
        let variables = command
            .get_envs()
            .map(|(name, value)| {
                (
                    name.to_string_lossy().into_owned(),
                    value.map(|value| value.to_string_lossy().into_owned()),
                )
            })
            .collect::<Vec<_>>();
        let value = |name: &str| {
            variables
                .iter()
                .find(|(candidate, _)| candidate == name)
                .and_then(|(_, value)| value.clone())
        };
        let count = value("GIT_CONFIG_COUNT")
            .and_then(|count| count.parse::<usize>().ok())
            .expect("configuration count should be set");
        let keys = (0..count)
            .filter_map(|index| value(&format!("GIT_CONFIG_KEY_{index}")))
            .collect::<Vec<_>>();
        for key in ["protocol.ext.allow", "core.fsmonitor", "core.hooksPath"] {
            assert!(keys.iter().any(|candidate| candidate == key), "{key}");
        }
        assert!(
            variables
                .iter()
                .any(|(name, value)| name == "GIT_DIR" && value.is_none())
        );
        let isolated = kind == HostedSourceControlKind::AzureDevops;
        assert_eq!(value("PYTHONSAFEPATH").is_some(), isolated);
        assert_eq!(value("PYTHONNOUSERSITE").is_some(), isolated);
    }
}

#[test]
fn empty_working_directory_is_private_and_removed() {
    let directory = cli::EmptyWorkingDirectory::create().unwrap();
    let path = directory.0.clone();
    assert!(path.is_dir());
    assert_eq!(std::fs::read_dir(&path).unwrap().count(), 0);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);
    }
    drop(directory);
    assert!(!path.exists());
}

/// A fake hosted CLI that, like `gh`, starts Git in its working directory.
#[cfg(unix)]
struct ShimFixture {
    base: std::path::PathBuf,
}

#[cfg(unix)]
impl ShimFixture {
    fn new() -> Self {
        use std::os::unix::fs::PermissionsExt;
        let base = std::env::temp_dir().join(format!(
            "ganbaru-hosted-cli-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let fixture = Self { base };
        std::fs::create_dir_all(fixture.repository()).unwrap();
        std::fs::write(fixture.global(), "[init]\n\tdefaultBranch = main\n").unwrap();
        let script = |path: &std::path::Path, body: &str| {
            std::fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        };
        script(
            &fixture.base.join("fsmonitor"),
            &format!("touch '{}'", fixture.marker().display()),
        );
        script(
            &fixture.shim(),
            "git status --porcelain >/dev/null 2>&1\necho '[]'",
        );
        fixture.git(&["init", "--quiet"]);
        fixture.git(&[
            "config",
            "core.fsmonitor",
            &fixture.base.join("fsmonitor").display().to_string(),
        ]);
        fixture
    }

    fn repository(&self) -> std::path::PathBuf {
        self.base.join("repo")
    }

    fn global(&self) -> std::path::PathBuf {
        self.base.join("global.gitconfig")
    }

    fn marker(&self) -> std::path::PathBuf {
        self.base.join("marker")
    }

    fn shim(&self) -> std::path::PathBuf {
        self.base.join("gh")
    }

    fn git(&self, arguments: &[&str]) {
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(self.repository())
            .args(arguments)
            .env("GIT_CONFIG_GLOBAL", self.global())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .status()
            .unwrap();
        assert!(status.success());
    }

    /// Runs the shim with the directory and environment the real CLI gets.
    async fn run(
        &self,
        directory: &std::path::Path,
        policy: crate::git::hardened::GitPolicy,
    ) -> bool {
        let _ = std::fs::remove_file(self.marker());
        let environment =
            crate::git::hardened::child_environment_blocking(directory, policy).unwrap();
        let built = hosted_cli_command(
            HostedSourceControlKind::Github,
            &["pr".to_string(), "list".to_string()],
            directory,
            &environment,
        )
        .unwrap();
        let mut command = std::process::Command::new(self.shim());
        command.current_dir(built.get_current_dir().unwrap());
        for (name, value) in built.get_envs() {
            match value {
                Some(value) => command.env(name, value),
                None => command.env_remove(name),
            };
        }
        command
            .env("GIT_CONFIG_GLOBAL", self.global())
            .env("GIT_CONFIG_NOSYSTEM", "1");
        let output =
            crate::git::hardened::run_program(command, std::time::Duration::from_secs(30), 1024)
                .await
                .unwrap();
        assert!(output.status.success());
        self.marker().exists()
    }
}

#[cfg(unix)]
impl Drop for ShimFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

#[cfg(unix)]
#[tokio::test]
async fn hosted_cli_git_children_skip_repository_fsmonitor_unless_trusted_by_the_user() {
    use crate::git::hardened::{GitFolderTrust, GitPolicy};
    let fixture = ShimFixture::new();
    let repository = fixture.repository();
    assert!(
        fixture
            .run(&repository, GitPolicy::user(GitFolderTrust::Trusted))
            .await,
        "a trusted user operation behaves like the terminal"
    );
    let isolated = cli::EmptyWorkingDirectory::create().unwrap();
    assert!(
        !fixture
            .run(&isolated.0, GitPolicy::internal(GitFolderTrust::Trusted))
            .await
    );
    for policy in [
        GitPolicy::internal(GitFolderTrust::Trusted),
        GitPolicy::internal(GitFolderTrust::Untrusted),
        GitPolicy::user(GitFolderTrust::Untrusted),
    ] {
        assert!(!fixture.run(&repository, policy).await, "{policy:?}");
    }
}
