use super::parsing::*;
use super::*;

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

pub fn list_arguments(
    kind: HostedSourceControlKind,
    repository_slug: &str,
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
        HostedSourceControlKind::AzureDevops => vec![
            "repos".into(),
            "pr".into(),
            "list".into(),
            "--detect".into(),
            "true".into(),
            "--status".into(),
            "all".into(),
            "--top".into(),
            limit.to_string(),
            "--only-show-errors".into(),
            "--output".into(),
            "json".into(),
        ],
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
            "--detect".into(),
            "true".into(),
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

pub fn create_arguments(request: &CreateHostedChangeRequest) -> ChatResult<Vec<String>> {
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
            vec![
                "repos".into(),
                "pr".into(),
                "create".into(),
                "--detect".into(),
                "true".into(),
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
            ]
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
    let version = run_probe(executable, &["--version"]).await;
    let auth = if version.is_ok() {
        run_probe(executable, auth_arguments).await
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

async fn run_probe(executable: &str, arguments: &[&str]) -> Result<String, ProbeFailure> {
    let mut child = Command::new(executable)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                ProbeFailure::Missing
            } else {
                ProbeFailure::Failed
            }
        })?;
    let stdout = child.stdout.take().ok_or(ProbeFailure::Failed)?;
    let stderr = child.stderr.take().ok_or(ProbeFailure::Failed)?;
    let stdout_task = tokio::spawn(async move {
        let mut bytes = Vec::new();
        stdout
            .take((MAX_PROBE_OUTPUT_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .await
            .map(|_| bytes)
    });
    let stderr_task = tokio::spawn(async move {
        let mut bytes = Vec::new();
        stderr
            .take((MAX_PROBE_OUTPUT_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .await
            .map(|_| bytes)
    });
    let status = tokio::time::timeout(PROBE_TIMEOUT, child.wait())
        .await
        .map_err(|_| ProbeFailure::Failed)?
        .map_err(|_| ProbeFailure::Failed)?;
    let stdout = stdout_task
        .await
        .map_err(|_| ProbeFailure::Failed)?
        .map_err(|_| ProbeFailure::Failed)?;
    let stderr = stderr_task
        .await
        .map_err(|_| ProbeFailure::Failed)?
        .map_err(|_| ProbeFailure::Failed)?;
    if !status.success()
        || stdout.len() > MAX_PROBE_OUTPUT_BYTES
        || stderr.len() > MAX_PROBE_OUTPUT_BYTES
    {
        return Err(ProbeFailure::Failed);
    }
    let output = if stdout.is_empty() { stderr } else { stdout };
    String::from_utf8(output).map_err(|_| ProbeFailure::Failed)
}

pub async fn run_cli_operation(
    executable: &str,
    arguments: &[String],
    root: &Path,
) -> ChatResult<String> {
    let mut child = Command::new(executable)
        .args(arguments)
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                ChatError::new(
                    ChatErrorCode::ExecutableMissing,
                    format!("{executable} is not available on PATH"),
                    true,
                )
            } else {
                ChatError::new(
                    ChatErrorCode::DriverUnavailable,
                    "Source-control command could not be started",
                    true,
                )
            }
        })?;
    let stdout = child.stdout.take().ok_or_else(protocol_error)?;
    let stderr = child.stderr.take().ok_or_else(protocol_error)?;
    let stdout_task = tokio::spawn(async move {
        let mut bytes = Vec::new();
        stdout
            .take((MAX_OPERATION_OUTPUT_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .await
            .map(|_| bytes)
    });
    let stderr_task = tokio::spawn(async move {
        let mut bytes = Vec::new();
        stderr
            .take((MAX_OPERATION_OUTPUT_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .await
            .map(|_| bytes)
    });
    let status = tokio::time::timeout(OPERATION_TIMEOUT, child.wait())
        .await
        .map_err(|_| {
            ChatError::new(
                ChatErrorCode::Timeout,
                "Source-control command timed out",
                true,
            )
        })?
        .map_err(|_| {
            ChatError::new(
                ChatErrorCode::DriverUnavailable,
                "Source-control command failed",
                true,
            )
        })?;
    let stdout = stdout_task
        .await
        .map_err(|_| protocol_error())?
        .map_err(|_| protocol_error())?;
    let stderr = stderr_task
        .await
        .map_err(|_| protocol_error())?
        .map_err(|_| protocol_error())?;
    if stdout.len() > MAX_OPERATION_OUTPUT_BYTES || stderr.len() > MAX_OPERATION_OUTPUT_BYTES {
        return Err(ChatError::new(
            ChatErrorCode::Protocol,
            "Source-control command output exceeded the supported limit",
            true,
        ));
    }
    if !status.success() {
        let detail = String::from_utf8_lossy(&stderr);
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
    String::from_utf8(stdout).map_err(|_| protocol_error())
}

pub fn detect_remote(remote: &str) -> Option<(HostedSourceControlKind, String)> {
    let normalized = remote.trim_end_matches(".git");
    let (host, path) = if let Ok(url) = reqwest::Url::parse(normalized) {
        (
            url.host_str()?.to_ascii_lowercase(),
            url.path().trim_matches('/').to_string(),
        )
    } else {
        let (host, path) = normalized.split_once(':')?;
        (
            host.trim_start_matches("git@").to_ascii_lowercase(),
            path.trim_matches('/').to_string(),
        )
    };
    let kind = if host == "github.com" {
        HostedSourceControlKind::Github
    } else if host == "gitlab.com" || host.contains("gitlab") {
        HostedSourceControlKind::Gitlab
    } else if host == "dev.azure.com" || host.ends_with("visualstudio.com") {
        HostedSourceControlKind::AzureDevops
    } else if host == "bitbucket.org" || host.contains("bitbucket") {
        HostedSourceControlKind::Bitbucket
    } else {
        return None;
    };
    (!path.is_empty()).then_some((kind, path))
}
