//! Hosted source-control discovery with recoverable local CLI states.

use super::credentials::{CredentialStore, SecretValue};
use super::git;
use super::models::{
    ChatError, ChatErrorCode, ChatResult, CredentialReferenceId, ProjectWorkingFolderId,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::process::Command;

mod bitbucket;
mod cli;
mod parsing;

use bitbucket::{
    bitbucket_authenticated_request, bitbucket_create_change_request,
    bitbucket_credential_reference, bitbucket_list_change_requests, credential_error,
    probe_bitbucket,
};
#[cfg(test)]
use bitbucket::{parse_bitbucket_change_request, percent_encode_segment};
use cli::{
    checkout_arguments, create_arguments, detect_remote, extract_created_reference, list_arguments,
    parse_change_request, parse_change_request_list, probe_cli, provider_executable,
    run_cli_operation, view_arguments,
};
use parsing::validate_text;

const PROBE_TIMEOUT: Duration = Duration::from_secs(10);
const OPERATION_TIMEOUT: Duration = Duration::from_secs(45);
const MAX_PROBE_OUTPUT_BYTES: usize = 32 * 1024;
const MAX_OPERATION_OUTPUT_BYTES: usize = 2 * 1024 * 1024;
const MAX_CHANGE_REQUESTS: u32 = 100;
const BITBUCKET_API_ROOT: &str = "https://api.bitbucket.org/2.0";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HostedSourceControlKind {
    Github,
    Gitlab,
    AzureDevops,
    Bitbucket,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostedSourceControlRead {
    pub kind: HostedSourceControlKind,
    pub label: String,
    pub detected_for_repository: bool,
    pub remote_name: Option<String>,
    pub repository_slug: Option<String>,
    pub status: String,
    pub version: Option<String>,
    pub unavailable_reason: Option<String>,
    pub configuration_hint: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostedChangeRequestRead {
    pub provider_kind: HostedSourceControlKind,
    pub number: u64,
    pub title: String,
    pub url: String,
    pub state: String,
    pub base_branch: String,
    pub head_branch: String,
    pub author: Option<String>,
    pub draft: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateHostedChangeRequest {
    pub working_folder_id: ProjectWorkingFolderId,
    pub execution_environment_id: Option<String>,
    pub provider_kind: HostedSourceControlKind,
    pub repository_slug: String,
    pub title: String,
    pub body: String,
    pub base_branch: String,
    pub head_branch: String,
    pub draft: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct BitbucketCredential {
    username: String,
    token: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigureBitbucketCredential {
    pub repository_slug: String,
    pub username: String,
    pub token: String,
}

pub async fn discover_source_control(
    root: &Path,
    credential_store: &dyn CredentialStore,
) -> ChatResult<Vec<HostedSourceControlRead>> {
    let remotes = git::remotes(root).await?;
    let detected = remotes
        .iter()
        .filter_map(|remote| {
            remote
                .fetch_url
                .as_deref()
                .and_then(detect_remote)
                .map(|(kind, slug)| (kind, remote.name.clone(), slug))
        })
        .collect::<Vec<_>>();
    let mut reads = Vec::with_capacity(4);
    for (kind, label, executable, install_hint, auth_arguments) in [
        (
            HostedSourceControlKind::Github,
            "GitHub",
            Some("gh"),
            "Install the GitHub CLI, then run gh auth login.",
            &["auth", "status"] as &[&str],
        ),
        (
            HostedSourceControlKind::Gitlab,
            "GitLab",
            Some("glab"),
            "Install the GitLab CLI, then run glab auth login.",
            &["auth", "status"],
        ),
        (
            HostedSourceControlKind::AzureDevops,
            "Azure DevOps",
            Some("az"),
            "Install the Azure CLI and Azure DevOps extension, then run az login.",
            &["account", "show", "--output", "json"],
        ),
        (
            HostedSourceControlKind::Bitbucket,
            "Bitbucket",
            None,
            "Store a Bitbucket access token in the operating-system credential store.",
            &[],
        ),
    ] {
        let repository = detected.iter().find(|entry| entry.0 == kind);
        reads.push(match executable {
            Some(executable) => {
                probe_cli(
                    kind,
                    label,
                    executable,
                    auth_arguments,
                    install_hint,
                    repository,
                )
                .await
            }
            None => probe_bitbucket(repository, install_hint, credential_store).await,
        });
    }
    reads.sort_by_key(|entry| !entry.detected_for_repository);
    Ok(reads)
}

pub async fn list_hosted_change_requests(
    root: &Path,
    credential_store: &dyn CredentialStore,
    provider_kind: HostedSourceControlKind,
    repository_slug: String,
    limit: u32,
) -> ChatResult<Vec<HostedChangeRequestRead>> {
    validate_text(&repository_slug, "repositorySlug", 2_048)?;
    let limit = limit.clamp(1, MAX_CHANGE_REQUESTS);
    if provider_kind == HostedSourceControlKind::Bitbucket {
        return bitbucket_list_change_requests(&repository_slug, limit, credential_store).await;
    }
    let arguments = list_arguments(provider_kind, &repository_slug, limit)?;
    let output = run_cli_operation(provider_executable(provider_kind)?, &arguments, root).await?;
    parse_change_request_list(provider_kind, &output)
}

pub async fn create_hosted_change_request(
    root: &Path,
    credential_store: &dyn CredentialStore,
    request: CreateHostedChangeRequest,
) -> ChatResult<HostedChangeRequestRead> {
    validate_text(&request.repository_slug, "repositorySlug", 2_048)?;
    validate_text(&request.title, "title", 1_024)?;
    validate_text(&request.base_branch, "baseBranch", 1_024)?;
    validate_text(&request.head_branch, "headBranch", 1_024)?;
    if request.body.len() > 262_144 || request.body.contains('\0') {
        return Err(ChatError::validation(
            "body",
            "Change request body is invalid",
        ));
    }
    if request.provider_kind == HostedSourceControlKind::Bitbucket {
        return bitbucket_create_change_request(&request, credential_store).await;
    }
    let arguments = create_arguments(&request)?;
    let output = run_cli_operation(
        provider_executable(request.provider_kind)?,
        &arguments,
        root,
    )
    .await?;
    let reference = extract_created_reference(request.provider_kind, &output)?;
    let view_arguments =
        view_arguments(request.provider_kind, &request.repository_slug, &reference)?;
    let detail = run_cli_operation(
        provider_executable(request.provider_kind)?,
        &view_arguments,
        root,
    )
    .await?;
    parse_change_request(request.provider_kind, &detail)
}

pub async fn checkout_hosted_change_request(
    root: &Path,
    provider_kind: HostedSourceControlKind,
    reference: String,
    remote_name: Option<String>,
) -> ChatResult<git::GitStatusRead> {
    validate_text(&reference, "reference", 2_048)?;
    if let Some(remote_name) = remote_name.as_deref() {
        validate_text(remote_name, "remoteName", 240)?;
    }
    if provider_kind == HostedSourceControlKind::Bitbucket {
        let remote_name = remote_name.as_deref().unwrap_or("origin");
        git::checkout_bitbucket_pull_request(root, remote_name, &reference).await?;
        return git::status(root).await;
    }
    let arguments = checkout_arguments(provider_kind, &reference, remote_name.as_deref())?;
    run_cli_operation(provider_executable(provider_kind)?, &arguments, root).await?;
    git::status(root).await
}

pub async fn configure_bitbucket_credential(
    credential_store: &dyn CredentialStore,
    request: ConfigureBitbucketCredential,
) -> ChatResult<()> {
    validate_text(&request.repository_slug, "repositorySlug", 2_048)?;
    validate_text(&request.username, "username", 512)?;
    if request.token.is_empty() || request.token.len() > 8_192 || request.token.contains('\0') {
        return Err(ChatError::validation(
            "token",
            "Bitbucket access token is invalid",
        ));
    }
    let credential = BitbucketCredential {
        username: request.username,
        token: request.token,
    };
    bitbucket_authenticated_request(reqwest::Method::GET, "/user", &credential, None).await?;
    let encoded = serde_json::to_string(&credential).map_err(|_| credential_error())?;
    let secret = SecretValue::new(encoded).map_err(|_| credential_error())?;
    credential_store
        .replace(
            &bitbucket_credential_reference(&request.repository_slug)?,
            &secret,
        )
        .map_err(|_| credential_error())?;
    Ok(())
}

pub fn remove_bitbucket_credential(
    credential_store: &dyn CredentialStore,
    repository_slug: String,
) -> ChatResult<bool> {
    validate_text(&repository_slug, "repositorySlug", 2_048)?;
    credential_store
        .remove(&bitbucket_credential_reference(&repository_slug)?)
        .map_err(|_| credential_error())
}

#[cfg(test)]
mod tests;
