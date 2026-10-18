use super::parsing::*;
use super::*;
use crate::git::hardened::{self, GitChildEnvironment, GitFolderTrust, GitPolicy, GitRunError};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn provider_executable(kind: HostedSourceControlKind) -> ChatResult<&'static str> {
    match kind {
        HostedSourceControlKind::Github => Ok("gh"),
        HostedSourceControlKind::Gitlab => Ok("glab"),
        HostedSourceControlKind::AzureDevops => Ok("az"),
        HostedSourceControlKind::Bitbucket => Err(ChatError::unsupported(
            "Bitbucket uses the bounded REST adapter instead of a local CLI",
        )),
    }
}

/// Organization, project, and repository of an Azure Repos remote, passed to
/// `az` explicitly so it never inspects the working folder.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AzureRepository {
    pub organization_url: String,
    pub project: String,
    pub repository: String,
}

impl AzureRepository {
    fn arguments(&self) -> [String; 8] {
        [
            "--organization".into(),
            self.organization_url.clone(),
            "--project".into(),
            self.project.clone(),
            "--repository".into(),
            self.repository.clone(),
            "--detect".into(),
            "false".into(),
        ]
    }
}

/// Which hosted CLI operation runs, deciding its working directory and Git
/// policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HostedCliOperation {
    /// Listing or viewing change requests for an explicit repository.
    Read,
    /// Creating a change request the user submitted.
    Create,
    /// Checking out a change request the user chose.
    Checkout,
}

impl HostedCliOperation {
    /// Reads run in an empty directory, so there is no repository
    /// configuration to trust.
    fn git_policy(self, trust: GitFolderTrust) -> GitPolicy {
        match self {
            Self::Read => GitPolicy::internal(GitFolderTrust::Untrusted),
            Self::Create | Self::Checkout => GitPolicy::user(trust),
        }
    }

    /// Reads and Azure creation name the repository explicitly. GitHub and
    /// GitLab creation stays in the folder so fork remotes resolve as in the
    /// user's terminal, and checkout changes the folder itself.
    pub(super) fn runs_in_repository(self, kind: HostedSourceControlKind) -> bool {
        match self {
            Self::Read => false,
            Self::Create => kind != HostedSourceControlKind::AzureDevops,
            Self::Checkout => true,
        }
    }
}

pub fn list_arguments(
    kind: HostedSourceControlKind,
    repository_slug: &str,
    azure: Option<&AzureRepository>,
    limit: u32,
) -> ChatResult<Vec<String>> {
    Ok(match kind {
        HostedSourceControlKind::Github => vec![
            "pr".into(),
            "list".into(),
            "--repo".into(),
            repository_slug.into(),
            "--state".into(),
            "all".into(),
            "--limit".into(),
            limit.to_string(),
            "--json".into(),
            "number,title,url,state,baseRefName,headRefName,author,isDraft".into(),
        ],
        HostedSourceControlKind::Gitlab => vec![
            "mr".into(),
            "list".into(),
            "--repo".into(),
            repository_slug.into(),
            "--all".into(),
            "--per-page".into(),
            limit.to_string(),
            "--output".into(),
            "json".into(),
        ],
        HostedSourceControlKind::AzureDevops => {
            let mut arguments = vec!["repos".into(), "pr".into(), "list".into()];
            arguments.extend(required_azure(azure)?.arguments());
            arguments.extend([
                "false".into(),
                "--status".into(),
                "all".into(),
                "--top".into(),
                limit.to_string(),
                "--only-show-errors".into(),
                "--output".into(),
                "json".into(),
            ]);
            arguments
        }
        HostedSourceControlKind::Bitbucket => {
            return Err(ChatError::unsupported(
                "Bitbucket pull request listing requires configured keyring credentials",
            ));
        }
    })
}

pub fn view_arguments(
    kind: HostedSourceControlKind,
    repository_slug: &str,
    azure: Option<&AzureRepository>,
    reference: &str,
) -> ChatResult<Vec<String>> {
    Ok(match kind {
        HostedSourceControlKind::Github => vec![
            "pr".into(),
            "view".into(),
            reference.into(),
            "--repo".into(),
            repository_slug.into(),
            "--json".into(),
            "number,title,url,state,baseRefName,headRefName,author,isDraft".into(),
        ],
        HostedSourceControlKind::Gitlab => vec![
            "mr".into(),
            "view".into(),
            reference.into(),
            "--repo".into(),
            repository_slug.into(),
            "--output".into(),
            "json".into(),
        ],
        HostedSourceControlKind::AzureDevops => vec![
            "repos".into(),
            "pr".into(),
            "show".into(),
            "--organization".into(),
            required_azure(azure)?.organization_url.clone(),
            "--detect".into(),
            "false".into(),
            "--id".into(),
            reference.into(),
            "--only-show-errors".into(),
            "--output".into(),
            "json".into(),
        ],
        HostedSourceControlKind::Bitbucket => {
            return Err(ChatError::unsupported(
                "Bitbucket pull request details require configured keyring credentials",
            ));
        }
    })
}

pub fn create_arguments(
    request: &CreateHostedChangeRequest,
    azure: Option<&AzureRepository>,
) -> ChatResult<Vec<String>> {
    Ok(match request.provider_kind {
        HostedSourceControlKind::Github => {
            let mut arguments = vec![
                "pr".into(),
                "create".into(),
                "--repo".into(),
                request.repository_slug.clone(),
                "--title".into(),
                request.title.clone(),
                "--body".into(),
                request.body.clone(),
                "--base".into(),
                request.base_branch.clone(),
                "--head".into(),
                request.head_branch.clone(),
            ];
            if request.draft {
                arguments.push("--draft".into());
            }
            arguments
        }
        HostedSourceControlKind::Gitlab => {
            let mut arguments = vec![
                "mr".into(),
                "create".into(),
                "--repo".into(),
                request.repository_slug.clone(),
                "--title".into(),
                request.title.clone(),
                "--description".into(),
                request.body.clone(),
                "--target-branch".into(),
                request.base_branch.clone(),
                "--source-branch".into(),
                request.head_branch.clone(),
                "--yes".into(),
            ];
            if request.draft {
                arguments.push("--draft".into());
            }
            arguments
        }
        HostedSourceControlKind::AzureDevops => {
            if request.draft {
                return Err(ChatError::unsupported(
                    "Azure DevOps CLI does not expose draft creation in this adapter",
                ));
            }
            let mut arguments = vec!["repos".into(), "pr".into(), "create".into()];
            arguments.extend(required_azure(azure)?.arguments());
            arguments.extend([
                "--title".into(),
                request.title.clone(),
                "--description".into(),
                request.body.clone(),
                "--target-branch".into(),
                request.base_branch.clone(),
                "--source-branch".into(),
                request.head_branch.clone(),
                "--only-show-errors".into(),
                "--output".into(),
                "json".into(),
            ]);
            arguments
        }
        HostedSourceControlKind::Bitbucket => {
            return Err(ChatError::unsupported(
                "Bitbucket pull request creation requires configured keyring credentials",
            ));
        }
    })
}

pub fn checkout_arguments(
    kind: HostedSourceControlKind,
    reference: &str,
    remote_name: Option<&str>,
) -> ChatResult<Vec<String>> {
    Ok(match kind {
        HostedSourceControlKind::Github => {
            vec!["pr".into(), "checkout".into(), reference.into()]
        }
        HostedSourceControlKind::Gitlab => {
            vec!["mr".into(), "checkout".into(), reference.into()]
        }
        HostedSourceControlKind::AzureDevops => vec![
            "repos".into(),
            "pr".into(),
            "checkout".into(),
            "--detect".into(),
            "true".into(),
            "--id".into(),
            reference.into(),
            "--remote-name".into(),
            remote_name.unwrap_or("origin").into(),
            "--only-show-errors".into(),
        ],
        HostedSourceControlKind::Bitbucket => {
            return Err(ChatError::unsupported(
                "Bitbucket pull request checkout requires configured keyring credentials",
            ));
        }
    })
}

pub fn extract_created_reference(
    kind: HostedSourceControlKind,
    output: &str,
) -> ChatResult<String> {
    if kind == HostedSourceControlKind::AzureDevops {
        let value: Value = serde_json::from_str(output).map_err(|_| protocol_error())?;
        return value
            .get("pullRequestId")
            .and_then(Value::as_u64)
            .map(|number| number.to_string())
            .ok_or_else(protocol_error);
    }
    output
        .split_whitespace()
        .find(|value| value.starts_with("https://") || value.starts_with("http://"))
        .map(|value| {
            value
                .trim_end_matches(|character: char| ",.;)".contains(character))
                .to_string()
        })
        .ok_or_else(protocol_error)
}

pub fn parse_change_request_list(
    kind: HostedSourceControlKind,
    output: &str,
) -> ChatResult<Vec<HostedChangeRequestRead>> {
    let value: Value = serde_json::from_str(output).map_err(|_| protocol_error())?;
    value
        .as_array()
        .ok_or_else(protocol_error)?
        .iter()
        .map(|entry| parse_change_request_value(kind, entry))
        .collect()
}

pub fn parse_change_request(
    kind: HostedSourceControlKind,
    output: &str,
) -> ChatResult<HostedChangeRequestRead> {
    let value: Value = serde_json::from_str(output).map_err(|_| protocol_error())?;
    parse_change_request_value(kind, &value)
}

pub async fn probe_cli(
    kind: HostedSourceControlKind,
    label: &str,
    executable: &str,
    auth_arguments: &[&str],
    install_hint: &str,
    repository: Option<&(HostedSourceControlKind, String, String)>,
) -> HostedSourceControlRead {
    let version = run_probe(kind, executable, &["--version"]).await;
    let auth = if version.is_ok() {
        run_probe(kind, executable, auth_arguments).await
    } else {
        Err(ProbeFailure::Missing)
    };
    let (status, unavailable_reason) = match (&version, &auth) {
        (Ok(_), Ok(_)) => ("available", None),
        (Ok(_), Err(_)) => (
            "authentication_required",
            Some(format!("{label} CLI is not authenticated")),
        ),
        (Err(ProbeFailure::Missing), _) => (
            "missing",
            Some(format!("{label} CLI is not available on PATH")),
        ),
        (Err(_), _) => ("unavailable", Some(format!("{label} CLI probe failed"))),
    };
    HostedSourceControlRead {
        kind,
        label: label.to_string(),
        detected_for_repository: repository.is_some(),
        remote_name: repository.map(|entry| entry.1.clone()),
        repository_slug: repository.map(|entry| entry.2.clone()),
        status: status.to_string(),
        version: version
            .ok()
            .and_then(|output| output.lines().next().map(str::to_string)),
        unavailable_reason,
        configuration_hint: (status != "available").then(|| install_hint.to_string()),
    }
}

#[derive(Clone, Copy)]
enum ProbeFailure {
    Missing,
    Failed,
}

async fn run_probe(
    kind: HostedSourceControlKind,
    executable: &str,
    arguments: &[&str],
) -> Result<String, ProbeFailure> {
    let directory = EmptyWorkingDirectory::create().map_err(|_| ProbeFailure::Failed)?;
    let mut command = std::process::Command::new(executable);
    command.args(arguments).current_dir(&directory.0);
    isolate_python(kind, &mut command);
    let output = hardened::run_program(command, PROBE_TIMEOUT, MAX_PROBE_OUTPUT_BYTES)
        .await
        .map_err(|error| match error {
            GitRunError::Missing => ProbeFailure::Missing,
            _ => ProbeFailure::Failed,
        })?;
    drop(directory);
    if !output.status.success() || output.stderr.len() > MAX_PROBE_OUTPUT_BYTES {
        return Err(ProbeFailure::Failed);
    }
    let output = if output.stdout.is_empty() {
        output.stderr
    } else {
        output.stdout
    };
    String::from_utf8(output).map_err(|_| ProbeFailure::Failed)
}

/// Builds a hosted CLI command with the Git policy carried in
/// `git_environment`. Azure CLI runs Python, so the working directory and
/// user site stay off its import path.
pub fn hosted_cli_command(
    kind: HostedSourceControlKind,
    arguments: &[String],
    directory: &Path,
    git_environment: &GitChildEnvironment,
) -> ChatResult<std::process::Command> {
    let mut command = std::process::Command::new(provider_executable(kind)?);
    command.args(arguments).current_dir(directory);
    git_environment.apply(&mut command);
    isolate_python(kind, &mut command);
    Ok(command)
}

fn isolate_python(kind: HostedSourceControlKind, command: &mut std::process::Command) {
    if kind == HostedSourceControlKind::AzureDevops {
        command
            .env("PYTHONSAFEPATH", "1")
            .env("PYTHONNOUSERSITE", "1");
    }
}

pub async fn run_cli_operation(
    kind: HostedSourceControlKind,
    arguments: &[String],
    folder: GitFolder<'_>,
    operation: HostedCliOperation,
) -> ChatResult<String> {
    let executable = provider_executable(kind)?;
    let isolated = if operation.runs_in_repository(kind) {
        None
    } else {
        Some(EmptyWorkingDirectory::create()?)
    };
    let directory = isolated
        .as_ref()
        .map_or_else(|| folder.root.to_path_buf(), |isolated| isolated.0.clone());
    let git_environment =
        hardened::child_environment(directory.clone(), operation.git_policy(folder.trust)).await?;
    let command = hosted_cli_command(kind, arguments, &directory, &git_environment)?;
    let output = hardened::run_program(command, OPERATION_TIMEOUT, MAX_OPERATION_OUTPUT_BYTES)
        .await
        .map_err(|error| cli_run_error(executable, error))?;
    drop(isolated);
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        let normalized = detail.to_ascii_lowercase();
        let code = if normalized.contains("auth")
            || normalized.contains("login")
            || normalized.contains("credential")
        {
            ChatErrorCode::AuthenticationRequired
        } else {
            ChatErrorCode::DriverUnavailable
        };
        let message = detail.trim();
        return Err(ChatError::new(
            code,
            if message.is_empty() {
                "Source-control command failed".to_string()
            } else {
                message.chars().take(2_000).collect()
            },
            true,
        ));
    }
    String::from_utf8(output.stdout).map_err(|_| protocol_error())
}

fn cli_run_error(executable: &str, error: GitRunError) -> ChatError {
    match error {
        GitRunError::Missing => ChatError::new(
            ChatErrorCode::ExecutableMissing,
            format!("{executable} is not available on PATH"),
            true,
        ),
        GitRunError::TimedOut => ChatError::new(
            ChatErrorCode::Timeout,
            "Source-control command timed out",
            true,
        ),
        GitRunError::OutputTooLarge => ChatError::new(
            ChatErrorCode::Protocol,
            "Source-control command output exceeded the supported limit",
            true,
        ),
        GitRunError::Cancelled => ChatError::from(error),
        GitRunError::Unavailable | GitRunError::Configuration => ChatError::new(
            ChatErrorCode::DriverUnavailable,
            "Source-control command could not be started",
            true,
        ),
    }
}

/// Private empty directory for hosted CLI runs that name their repository
/// explicitly, so nothing in the working folder is on their configuration or
/// import path. Removed on drop.
pub(super) struct EmptyWorkingDirectory(pub(super) PathBuf);

impl EmptyWorkingDirectory {
    pub(super) fn create() -> ChatResult<Self> {
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos());
        let path = std::env::temp_dir().join(format!(
            "ganbaru-source-control-{}-{}-{nanos}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(&path).map_err(|_| {
            ChatError::new(
                ChatErrorCode::DriverUnavailable,
                "Source-control command could not be started",
                true,
            )
        })?;
        Ok(Self(path))
    }
}

impl Drop for EmptyWorkingDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn required_azure(azure: Option<&AzureRepository>) -> ChatResult<&AzureRepository> {
    azure.ok_or_else(|| {
        ChatError::validation(
            "repositorySlug",
            "The Azure DevOps remote was not found in this repository",
        )
    })
}

/// Host and path of a remote URL or scp-style address, with any user name
/// removed and the host lowercased.
fn remote_host_and_path(remote: &str) -> Option<(String, String)> {
    let normalized = remote.trim_end_matches(".git");
    let (host, path) = if let Ok(url) = reqwest::Url::parse(normalized) {
        (
            url.host_str()?.to_ascii_lowercase(),
            url.path().trim_matches('/').to_string(),
        )
    } else {
        let (host, path) = normalized.split_once(':')?;
        (
            host.rsplit_once('@')
                .map_or(host, |(_, host)| host)
                .to_ascii_lowercase(),
            path.trim_matches('/').to_string(),
        )
    };
    Some((host, path))
}

pub fn detect_remote(remote: &str) -> Option<(HostedSourceControlKind, String)> {
    let (host, path) = remote_host_and_path(remote)?;
    let kind = if host == "github.com" {
        HostedSourceControlKind::Github
    } else if host == "gitlab.com" || host.contains("gitlab") {
        HostedSourceControlKind::Gitlab
    } else if host == "dev.azure.com"
        || host == "ssh.dev.azure.com"
        || host.ends_with("visualstudio.com")
    {
        HostedSourceControlKind::AzureDevops
    } else if host == "bitbucket.org" || host.contains("bitbucket") {
        HostedSourceControlKind::Bitbucket
    } else {
        return None;
    };
    (!path.is_empty()).then_some((kind, path))
}

/// Parses the Azure Repos remote shapes: HTTPS on `dev.azure.com` and
/// `<organization>.visualstudio.com` (with or without a collection), and SSH
/// `v3` addresses on either host.
pub fn azure_repository(remote: &str) -> Option<AzureRepository> {
    let (host, path) = remote_host_and_path(remote)?;
    let segments = path
        .split('/')
        .map(percent_decode)
        .collect::<Option<Vec<_>>>()?;
    let segments = segments.iter().map(String::as_str).collect::<Vec<_>>();
    let (organization_url, project, repository) = match (host.as_str(), segments.as_slice()) {
        ("dev.azure.com", [organization, project, "_git", repository])
        | (
            "ssh.dev.azure.com" | "vs-ssh.visualstudio.com",
            ["v3", organization, project, repository],
        ) => {
            if !azure_organization_is_valid(organization) {
                return None;
            }
            (
                format!("https://dev.azure.com/{organization}"),
                project,
                repository,
            )
        }
        (host, [project, "_git", repository] | [_, project, "_git", repository])
            if host.ends_with(".visualstudio.com")
                && azure_organization_is_valid(host.trim_end_matches(".visualstudio.com")) =>
        {
            (format!("https://{host}"), project, repository)
        }
        _ => return None,
    };
    let valid = |value: &str| {
        !value.is_empty() && !value.starts_with('-') && !value.chars().any(char::is_control)
    };
    (valid(project) && valid(repository)).then(|| AzureRepository {
        organization_url,
        project: (*project).to_string(),
        repository: (*repository).to_string(),
    })
}

fn azure_organization_is_valid(organization: &str) -> bool {
    !organization.is_empty()
        && !organization.starts_with('-')
        && organization
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

/// Decodes `%XX` escapes in one URL path segment.
fn percent_decode(segment: &str) -> Option<String> {
    let bytes = segment.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = segment.get(index + 1..index + 3)?;
            decoded.push(u8::from_str_radix(hex, 16).ok()?);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}
