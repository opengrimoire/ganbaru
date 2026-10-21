//! Bounded Git process execution and typed porcelain parsing.

pub mod hardened;

use super::models::{ChatError, ChatErrorCode, ChatResult};
use hardened::{GIT_NETWORK_DEADLINE, GIT_READ_DEADLINE, GitFolderTrust, GitPolicy, OwnedGitRun};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// A folder Git runs in, with this device's trust in its configuration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GitFolder<'a> {
    pub root: &'a Path,
    pub trust: GitFolderTrust,
}

impl<'a> GitFolder<'a> {
    pub const fn new(root: &'a Path, trust: GitFolderTrust) -> Self {
        Self { root, trust }
    }

    /// A folder whose repository programs and hooks never run.
    pub const fn untrusted(root: &'a Path) -> Self {
        Self::new(root, GitFolderTrust::Untrusted)
    }
}

const MAX_GIT_OUTPUT_BYTES: usize = 8 * 1024 * 1024;
const MAX_REVIEW_OUTPUT_BYTES: usize = 64 * 1024 * 1024;
/// Longest git or hosted CLI error text returned to the webview, in characters.
pub(crate) const MAX_DIAGNOSTIC_CHARS: usize = 2_048;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitChangedPathRead {
    pub relative_path: String,
    pub original_relative_path: Option<String>,
    pub index_status: String,
    pub worktree_status: String,
    pub untracked: bool,
    pub ignored: bool,
    pub conflicted: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitStatusRead {
    pub branch: Option<String>,
    pub detached: bool,
    pub upstream: Option<String>,
    pub ahead: u64,
    pub behind: u64,
    pub files: Vec<GitChangedPathRead>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitRemoteRead {
    pub name: String,
    pub fetch_url: Option<String>,
    pub push_url: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitBranchRead {
    pub name: String,
    pub current: bool,
    pub upstream: Option<String>,
    pub ahead: u64,
    pub behind: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitWorktreeRead {
    pub path: String,
    pub head: String,
    pub branch: Option<String>,
    pub bare: bool,
    pub detached: bool,
    pub locked: bool,
    pub prunable: bool,
}

struct GitOutput {
    stdout: Vec<u8>,
    stdout_bytes: u64,
}

#[derive(Clone, Copy, Default)]
struct GitStorage<'a> {
    index_file: Option<&'a Path>,
    object_directory: Option<&'a Path>,
    alternate_object_directory: Option<&'a Path>,
}

pub async fn status(folder: GitFolder<'_>) -> ChatResult<GitStatusRead> {
    let output = run(
        folder,
        &[
            "status",
            "--porcelain=v2",
            "--branch",
            "--untracked-files=all",
            "-z",
        ],
        GitCall::Internal,
    )
    .await?;
    parse_status(&output.stdout)
}

pub async fn diff(
    folder: GitFolder<'_>,
    staged: bool,
    relative_path: Option<&str>,
) -> ChatResult<String> {
    let mut arguments = vec!["diff", "--no-ext-diff", "--no-color"];
    if staged {
        arguments.push("--cached");
    }
    if let Some(path) = relative_path {
        arguments.extend(["--", path]);
    }
    let output = run(folder, &arguments, GitCall::Internal).await?;
    String::from_utf8(output.stdout)
        .map_err(|_| git_protocol_error("Git diff output is not valid UTF-8"))
}

pub async fn stage(folder: GitFolder<'_>, paths: &[String]) -> ChatResult<()> {
    let mut arguments = vec!["add", "--"];
    arguments.extend(paths.iter().map(String::as_str));
    run(folder, &arguments, GitCall::User).await.map(|_| ())
}

pub async fn unstage(folder: GitFolder<'_>, paths: &[String]) -> ChatResult<()> {
    let source = match run(
        folder,
        &["rev-parse", "--verify", "HEAD^{tree}"],
        GitCall::Internal,
    )
    .await
    {
        Ok(output) => String::from_utf8(output.stdout)
            .map_err(|_| git_protocol_error("Git tree ID is not valid UTF-8"))?
            .trim()
            .to_string(),
        Err(_) => String::from_utf8(
            run_bounded(
                folder,
                &["mktree"],
                GitCall::Internal,
                Some(&[]),
                MAX_GIT_OUTPUT_BYTES,
            )
            .await?
            .stdout,
        )
        .map_err(|_| git_protocol_error("Git empty tree ID is not valid UTF-8"))?
        .trim()
        .to_string(),
    };
    let source_argument = format!("--source={source}");
    let mut arguments = vec!["restore", "--staged", source_argument.as_str(), "--"];
    arguments.extend(paths.iter().map(String::as_str));
    run(folder, &arguments, GitCall::User).await.map(|_| ())
}

pub async fn commit(folder: GitFolder<'_>, message: &str) -> ChatResult<()> {
    run(folder, &["commit", "-m", message], GitCall::User)
        .await
        .map(|_| ())
}

pub async fn fetch(folder: GitFolder<'_>, remote: Option<&str>) -> ChatResult<()> {
    let mut arguments = vec!["fetch", "--prune"];
    if let Some(remote) = remote {
        require_configured_remote(folder, remote).await?;
        arguments.push(remote);
    }
    run(folder, &arguments, GitCall::UserNetwork)
        .await
        .map(|_| ())
}

pub async fn pull(
    folder: GitFolder<'_>,
    remote: Option<&str>,
    branch: Option<&str>,
) -> ChatResult<()> {
    let mut arguments = vec!["pull", "--ff-only"];
    if let Some(remote) = remote {
        require_configured_remote(folder, remote).await?;
        arguments.push(remote);
    }
    if let Some(branch) = branch {
        if remote.is_none() {
            return Err(ChatError::validation(
                "remote",
                "A Git branch needs its remote",
            ));
        }
        validate_branch_name(branch)?;
        arguments.push(branch);
    }
    run(folder, &arguments, GitCall::UserNetwork)
        .await
        .map(|_| ())
}

pub async fn push(
    folder: GitFolder<'_>,
    remote: Option<&str>,
    branch: Option<&str>,
    force_with_lease: bool,
) -> ChatResult<()> {
    let mut arguments = vec!["push"];
    if force_with_lease {
        arguments.push("--force-with-lease");
    }
    if let Some(remote) = remote {
        require_configured_remote(folder, remote).await?;
        arguments.push(remote);
    }
    if let Some(branch) = branch {
        if remote.is_none() {
            return Err(ChatError::validation(
                "remote",
                "A Git branch needs its remote",
            ));
        }
        validate_branch_name(branch)?;
        arguments.push(branch);
    }
    run(folder, &arguments, GitCall::UserNetwork)
        .await
        .map(|_| ())
}

pub async fn remotes(folder: GitFolder<'_>) -> ChatResult<Vec<GitRemoteRead>> {
    let output = run(folder, &["remote", "-v"], GitCall::Internal).await?;
    let text = String::from_utf8(output.stdout)
        .map_err(|_| git_protocol_error("Git remote output is not valid UTF-8"))?;
    let mut remotes = Vec::<GitRemoteRead>::new();
    for line in text.lines() {
        let Some((name, remainder)) = line.split_once(char::is_whitespace) else {
            continue;
        };
        let mut parts = remainder.split_whitespace();
        let Some(url) = parts.next() else { continue };
        let kind = parts.next().unwrap_or_default();
        let remote = match remotes.iter_mut().find(|remote| remote.name == name) {
            Some(remote) => remote,
            None => {
                remotes.push(GitRemoteRead {
                    name: name.to_string(),
                    fetch_url: None,
                    push_url: None,
                });
                remotes.last_mut().expect("remote was just inserted")
            }
        };
        match kind {
            "(fetch)" => remote.fetch_url = Some(url.to_string()),
            "(push)" => remote.push_url = Some(url.to_string()),
            _ => {}
        }
    }
    Ok(remotes)
}

pub async fn branches(folder: GitFolder<'_>) -> ChatResult<Vec<GitBranchRead>> {
    let output = run(
        folder,
        &[
            "for-each-ref",
            "--format=%(HEAD)%00%(refname:short)%00%(upstream:short)%00%(upstream:track,nobracket)%00",
            "refs/heads/",
        ],
        GitCall::Internal,
    )
    .await?;
    let text = String::from_utf8(output.stdout)
        .map_err(|_| git_protocol_error("Git branch output is not valid UTF-8"))?;
    let fields = text.split('\0').collect::<Vec<_>>();
    let mut branches = Vec::new();
    for chunk in fields.chunks(4) {
        if chunk.len() < 4 || chunk[1].is_empty() {
            continue;
        }
        let (ahead, behind) = parse_track(chunk[3]);
        branches.push(GitBranchRead {
            name: chunk[1].to_string(),
            current: chunk[0] == "*",
            upstream: (!chunk[2].is_empty()).then(|| chunk[2].to_string()),
            ahead,
            behind,
        });
    }
    Ok(branches)
}

pub async fn worktrees(folder: GitFolder<'_>) -> ChatResult<Vec<GitWorktreeRead>> {
    let output = run(
        folder,
        &["worktree", "list", "--porcelain", "-z"],
        GitCall::Internal,
    )
    .await?;
    let text = String::from_utf8(output.stdout)
        .map_err(|_| git_protocol_error("Git worktree output is not valid UTF-8"))?;
    let mut reads = Vec::new();
    let mut current: Option<GitWorktreeRead> = None;
    for field in text.split('\0') {
        if field.is_empty() {
            if let Some(read) = current.take() {
                reads.push(read);
            }
            continue;
        }
        if let Some(path) = field.strip_prefix("worktree ") {
            if let Some(read) = current.take() {
                reads.push(read);
            }
            current = Some(GitWorktreeRead {
                path: path.to_string(),
                head: String::new(),
                branch: None,
                bare: false,
                detached: false,
                locked: false,
                prunable: false,
            });
        } else if let Some(read) = current.as_mut() {
            if let Some(head) = field.strip_prefix("HEAD ") {
                read.head = head.to_string();
            } else if let Some(branch) = field.strip_prefix("branch refs/heads/") {
                read.branch = Some(branch.to_string());
            } else {
                match field.split_once(' ').map_or(field, |(kind, _)| kind) {
                    "bare" => read.bare = true,
                    "detached" => read.detached = true,
                    "locked" => read.locked = true,
                    "prunable" => read.prunable = true,
                    _ => {}
                }
            }
        }
    }
    if let Some(read) = current {
        reads.push(read);
    }
    Ok(reads)
}

pub async fn common_directory(folder: GitFolder<'_>) -> ChatResult<PathBuf> {
    let output = run(
        folder,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        GitCall::Internal,
    )
    .await?;
    let value = String::from_utf8(output.stdout)
        .map_err(|_| git_protocol_error("Git common directory is not valid UTF-8"))?;
    let value = value.trim();
    if value.is_empty() {
        return Err(git_protocol_error("Git common directory is missing"));
    }
    Ok(PathBuf::from(value))
}

pub async fn add_worktree(
    folder: GitFolder<'_>,
    worktree_path: &str,
    branch: &str,
    base_reference: &str,
) -> ChatResult<()> {
    run(
        folder,
        &[
            "worktree",
            "add",
            "-b",
            branch,
            worktree_path,
            base_reference,
        ],
        GitCall::UserNetwork,
    )
    .await
    .map(|_| ())
}

pub async fn remove_worktree(folder: GitFolder<'_>, worktree_path: &str) -> ChatResult<()> {
    run(
        folder,
        &["worktree", "remove", worktree_path],
        GitCall::Internal,
    )
    .await
    .map(|_| ())
}

pub async fn head_object_id(folder: GitFolder<'_>) -> ChatResult<String> {
    let output = run(folder, &["rev-parse", "HEAD"], GitCall::Internal).await?;
    String::from_utf8(output.stdout)
        .map(|value| value.trim().to_string())
        .map_err(|_| git_protocol_error("Git object ID is not valid UTF-8"))
}

pub async fn initialize(folder: GitFolder<'_>, initial_branch: Option<&str>) -> ChatResult<()> {
    let mut arguments = vec!["init"];
    if let Some(branch) = initial_branch {
        arguments.extend(["--initial-branch", branch]);
    }
    run(folder, &arguments, GitCall::User).await.map(|_| ())
}

pub async fn clone_into(
    folder: GitFolder<'_>,
    remote_url: &str,
    remote_name: &str,
) -> ChatResult<()> {
    validate_git_argument(remote_url)?;
    validate_git_argument(remote_name)?;
    run(
        folder,
        &["clone", "--origin", remote_name, "--", remote_url, "."],
        GitCall::UserNetwork,
    )
    .await
    .map(|_| ())
}

pub async fn discard_paths(
    folder: GitFolder<'_>,
    tracked_paths: &[String],
    untracked_paths: &[String],
) -> ChatResult<()> {
    if !tracked_paths.is_empty() {
        let mut arguments = vec!["restore", "--worktree", "--"];
        arguments.extend(tracked_paths.iter().map(String::as_str));
        run(folder, &arguments, GitCall::User).await?;
    }
    if !untracked_paths.is_empty() {
        let mut arguments = vec!["clean", "-f", "--"];
        arguments.extend(untracked_paths.iter().map(String::as_str));
        run(folder, &arguments, GitCall::User).await?;
    }
    Ok(())
}

pub async fn delete_branch(folder: GitFolder<'_>, branch: &str, force: bool) -> ChatResult<()> {
    validate_git_argument(branch)?;
    run(
        folder,
        &["branch", if force { "-D" } else { "-d" }, branch],
        GitCall::User,
    )
    .await
    .map(|_| ())
}

pub async fn checkout_bitbucket_pull_request(
    folder: GitFolder<'_>,
    remote_name: &str,
    reference: &str,
) -> ChatResult<()> {
    require_configured_remote(folder, remote_name).await?;
    if reference.is_empty()
        || reference.len() > 64
        || !reference
            .chars()
            .all(|character| character.is_ascii_digit())
    {
        return Err(ChatError::validation(
            "reference",
            "Bitbucket pull request reference is invalid",
        ));
    }
    let remote_tracking_ref = format!("refs/remotes/{remote_name}/pull-request/{reference}");
    let refspec = format!("refs/pull-requests/{reference}/from:{remote_tracking_ref}");
    run(
        folder,
        &["fetch", remote_name, &refspec],
        GitCall::UserNetwork,
    )
    .await?;
    let local_branch = format!("pull-request-{reference}");
    run(
        folder,
        &["checkout", "-b", &local_branch, &remote_tracking_ref],
        GitCall::User,
    )
    .await
    .map(|_| ())
}

/// Longest branch name accepted from the webview.
const MAX_BRANCH_NAME_BYTES: usize = 1_024;

/// Validates a branch name with `git check-ref-format --branch` rules, and
/// additionally refuses refspec syntax so a name can never force-push, map to
/// another ref, or match several refs.
pub fn validate_branch_name(value: &str) -> ChatResult<()> {
    let invalid = || ChatError::validation("branch", "Git branch name is invalid");
    if value.is_empty()
        || value.len() > MAX_BRANCH_NAME_BYTES
        || value == "@"
        || value.starts_with(['-', '+', '/'])
        || value.ends_with(['/', '.'])
        || value.ends_with(".lock")
        || value.contains("..")
        || value.contains("//")
        || value.contains("@{")
        || value.chars().any(|character| {
            character.is_control()
                || character.is_whitespace()
                || matches!(character, '~' | '^' | ':' | '?' | '*' | '[' | '\\')
        })
        || value
            .split('/')
            .any(|component| component.starts_with('.') || component.ends_with(".lock"))
    {
        return Err(invalid());
    }
    Ok(())
}

/// Accepts only the name of a remote already configured in the repository, so
/// a URL or path cannot redirect a push, pull, or fetch.
async fn require_configured_remote(folder: GitFolder<'_>, remote: &str) -> ChatResult<()> {
    validate_git_argument(remote)?;
    if remotes(folder)
        .await?
        .iter()
        .any(|configured| configured.name == remote)
    {
        Ok(())
    } else {
        Err(ChatError::validation(
            "remote",
            "Git remote is not configured in this repository",
        ))
    }
}
fn validate_git_argument(value: &str) -> ChatResult<()> {
    if value.is_empty()
        || value.len() > 1_024
        || value.starts_with('-')
        || value.chars().any(char::is_whitespace)
        || value.chars().any(char::is_control)
    {
        return Err(ChatError::validation(
            "gitArgument",
            "Git argument is invalid",
        ));
    }
    Ok(())
}

/// How a Git call is classified for hardening and deadlines.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GitCall {
    /// Started by Ganbaru for reads, review, or bookkeeping.
    Internal,
    /// Requested by the user and local to the repository.
    User,
    /// Requested by the user and reaching a remote.
    UserNetwork,
}

impl GitCall {
    fn policy(self, trust: GitFolderTrust) -> GitPolicy {
        match self {
            Self::Internal => GitPolicy::internal(trust),
            Self::User | Self::UserNetwork => GitPolicy::user(trust),
        }
    }

    fn deadline(self) -> Duration {
        match self {
            Self::Internal | Self::User => GIT_READ_DEADLINE,
            Self::UserNetwork => GIT_NETWORK_DEADLINE,
        }
    }
}

async fn run(folder: GitFolder<'_>, arguments: &[&str], call: GitCall) -> ChatResult<GitOutput> {
    run_bounded(folder, arguments, call, None, MAX_GIT_OUTPUT_BYTES).await
}

pub async fn review_output(
    folder: GitFolder<'_>,
    arguments: &[&str],
    input: Option<&[u8]>,
) -> ChatResult<Vec<u8>> {
    run_bounded(
        folder,
        arguments,
        GitCall::Internal,
        input,
        MAX_REVIEW_OUTPUT_BYTES,
    )
    .await
    .map(|output| output.stdout)
}

/// Runs a user-requested review mutation (stage, unstage, or discard) with
/// the same configuration the user's terminal would apply.
pub async fn review_action_output(
    folder: GitFolder<'_>,
    arguments: &[&str],
    input: Option<&[u8]>,
) -> ChatResult<Vec<u8>> {
    run_bounded(
        folder,
        arguments,
        GitCall::User,
        input,
        MAX_REVIEW_OUTPUT_BYTES,
    )
    .await
    .map(|output| output.stdout)
}

pub async fn review_output_in_storage(
    folder: GitFolder<'_>,
    arguments: &[&str],
    input: Option<&[u8]>,
    index_file: Option<&Path>,
    object_directory: Option<&Path>,
    alternate_object_directory: Option<&Path>,
) -> ChatResult<Vec<u8>> {
    run_bounded_with_storage(
        folder,
        arguments,
        GitCall::Internal,
        input,
        MAX_REVIEW_OUTPUT_BYTES,
        None,
        GitStorage {
            index_file,
            object_directory,
            alternate_object_directory,
        },
    )
    .await
    .map(|output| output.stdout)
}

pub async fn review_output_to_file_in_storage(
    folder: GitFolder<'_>,
    arguments: &[&str],
    output_path: &Path,
    maximum_output_bytes: usize,
    object_directory: Option<&Path>,
    alternate_object_directory: Option<&Path>,
) -> ChatResult<u64> {
    run_bounded_with_storage(
        folder,
        arguments,
        GitCall::Internal,
        None,
        maximum_output_bytes,
        Some(output_path),
        GitStorage {
            index_file: None,
            object_directory,
            alternate_object_directory,
        },
    )
    .await
    .map(|output| output.stdout_bytes)
}

async fn run_bounded(
    folder: GitFolder<'_>,
    arguments: &[&str],
    call: GitCall,
    input: Option<&[u8]>,
    maximum_output_bytes: usize,
) -> ChatResult<GitOutput> {
    run_bounded_with_storage(
        folder,
        arguments,
        call,
        input,
        maximum_output_bytes,
        None,
        GitStorage::default(),
    )
    .await
}

async fn run_bounded_with_storage(
    folder: GitFolder<'_>,
    arguments: &[&str],
    call: GitCall,
    input: Option<&[u8]>,
    maximum_output_bytes: usize,
    stdout_file: Option<&Path>,
    storage: GitStorage<'_>,
) -> ChatResult<GitOutput> {
    let output = hardened::run(OwnedGitRun {
        root: folder.root.to_path_buf(),
        arguments: arguments.iter().map(OsString::from).collect(),
        policy: call.policy(folder.trust),
        deadline: call.deadline(),
        input: input.map(<[u8]>::to_vec),
        maximum_output_bytes,
        stdout_file: stdout_file.map(Path::to_path_buf),
        environment: storage_environment(storage)?,
    })
    .await?;
    if !output.status.success() {
        let detail = diagnostic_text(&output.stderr);
        return Err(ChatError {
            code: ChatErrorCode::Conflict,
            message: "Git operation failed safely".to_string(),
            field: None,
            recoverable: true,
            details: (!detail.is_empty()).then(|| Box::new(json!({ "stderr": detail }))),
        });
    }
    Ok(GitOutput {
        stdout: output.stdout,
        stdout_bytes: output.stdout_bytes,
    })
}

/// Turns tool error output into text safe to show: control and bidirectional formatting
/// characters are removed (line breaks and tabs stay), credentials in `scheme://user@host`
/// URLs are dropped, and the result is capped at [`MAX_DIAGNOSTIC_CHARS`] with a visible marker.
pub(crate) fn diagnostic_text(raw: &[u8]) -> String {
    let lossy = String::from_utf8_lossy(raw);
    let visible: String = lossy
        .chars()
        .filter(|character| {
            matches!(character, '\n' | '\t')
                || !(character.is_control()
                    || matches!(character, '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'))
        })
        .collect();
    let redacted = without_url_userinfo(visible.trim());
    let mut characters = redacted.chars();
    let mut text: String = characters.by_ref().take(MAX_DIAGNOSTIC_CHARS).collect();
    if characters.next().is_some() {
        text.push_str(" [truncated]");
    }
    text
}

/// Removes the `user[:password]@` part after every `://` in `text`.
fn without_url_userinfo(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(index) = rest.find("://") {
        let (before, after_scheme) = rest.split_at(index + 3);
        output.push_str(before);
        let authority_end = after_scheme
            .find(|character: char| {
                matches!(character, '/' | '?' | '#') || character.is_whitespace()
            })
            .unwrap_or(after_scheme.len());
        let authority = &after_scheme[..authority_end];
        output.push_str(
            authority
                .rsplit_once('@')
                .map_or(authority, |(_, host)| host),
        );
        rest = &after_scheme[authority_end..];
    }
    output.push_str(rest);
    output
}

fn storage_environment(storage: GitStorage<'_>) -> ChatResult<Vec<(String, OsString)>> {
    let mut environment = Vec::new();
    if let Some(index_file) = storage.index_file {
        environment.push(("GIT_INDEX_FILE".to_string(), index_file.into()));
    }
    if let Some(object_directory) = storage.object_directory {
        environment.push(("GIT_OBJECT_DIRECTORY".to_string(), object_directory.into()));
    }
    if let Some(alternate) = storage.alternate_object_directory {
        let alternates = std::env::join_paths([alternate]).map_err(|_| {
            ChatError::new(
                ChatErrorCode::Validation,
                "Git object directory path is unsupported",
                false,
            )
        })?;
        environment.push(("GIT_ALTERNATE_OBJECT_DIRECTORIES".to_string(), alternates));
    }
    Ok(environment)
}

fn parse_status(bytes: &[u8]) -> ChatResult<GitStatusRead> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| git_protocol_error("Git status output is not valid UTF-8"))?;
    let fields = text.split('\0').collect::<Vec<_>>();
    let mut status = GitStatusRead {
        branch: None,
        detached: false,
        upstream: None,
        ahead: 0,
        behind: 0,
        files: Vec::new(),
    };
    let mut index = 0;
    while index < fields.len() {
        let field = fields[index];
        index += 1;
        if let Some(head) = field.strip_prefix("# branch.head ") {
            status.detached = head == "(detached)";
            if !status.detached && head != "(unknown)" {
                status.branch = Some(head.to_string());
            }
        } else if let Some(upstream) = field.strip_prefix("# branch.upstream ") {
            status.upstream = Some(upstream.to_string());
        } else if let Some(ahead_behind) = field.strip_prefix("# branch.ab ") {
            for value in ahead_behind.split_whitespace() {
                if let Some(ahead) = value.strip_prefix('+') {
                    status.ahead = ahead.parse().unwrap_or(0);
                } else if let Some(behind) = value.strip_prefix('-') {
                    status.behind = behind.parse().unwrap_or(0);
                }
            }
        } else if let Some(path) = field.strip_prefix("? ") {
            status
                .files
                .push(changed_path(path, None, "?", "?", true, false, false));
        } else if let Some(path) = field.strip_prefix("! ") {
            status
                .files
                .push(changed_path(path, None, "!", "!", false, true, false));
        } else if field.starts_with("1 ") {
            let parts = field.splitn(9, ' ').collect::<Vec<_>>();
            if parts.len() != 9 || parts[1].len() != 2 {
                return Err(git_protocol_error("Git status entry is invalid"));
            }
            status.files.push(changed_path(
                parts[8],
                None,
                &parts[1][..1],
                &parts[1][1..],
                false,
                false,
                false,
            ));
        } else if field.starts_with("2 ") {
            let parts = field.splitn(10, ' ').collect::<Vec<_>>();
            let original = fields.get(index).copied();
            index += usize::from(original.is_some());
            if parts.len() != 10 || parts[1].len() != 2 || original.is_none() {
                return Err(git_protocol_error("Git rename status entry is invalid"));
            }
            status.files.push(changed_path(
                parts[9],
                original.map(str::to_string),
                &parts[1][..1],
                &parts[1][1..],
                false,
                false,
                false,
            ));
        } else if field.starts_with("u ") {
            let parts = field.splitn(11, ' ').collect::<Vec<_>>();
            if parts.len() != 11 || parts[1].len() != 2 {
                return Err(git_protocol_error("Git conflict status entry is invalid"));
            }
            status.files.push(changed_path(
                parts[10],
                None,
                &parts[1][..1],
                &parts[1][1..],
                false,
                false,
                true,
            ));
        }
    }
    Ok(status)
}

fn changed_path(
    path: &str,
    original: Option<String>,
    index_status: &str,
    worktree_status: &str,
    untracked: bool,
    ignored: bool,
    conflicted: bool,
) -> GitChangedPathRead {
    GitChangedPathRead {
        relative_path: path.to_string(),
        original_relative_path: original,
        index_status: index_status.to_string(),
        worktree_status: worktree_status.to_string(),
        untracked,
        ignored,
        conflicted,
    }
}

fn parse_track(value: &str) -> (u64, u64) {
    let mut ahead = 0;
    let mut behind = 0;
    for part in value.split(',').map(str::trim) {
        if let Some(value) = part.strip_prefix("ahead ") {
            ahead = value.parse().unwrap_or(0);
        } else if let Some(value) = part.strip_prefix("behind ") {
            behind = value.parse().unwrap_or(0);
        }
    }
    (ahead, behind)
}

fn git_protocol_error(message: &str) -> ChatError {
    ChatError::new(ChatErrorCode::Protocol, message, true)
}

#[cfg(test)]
mod tests {
    use super::{
        GitCall, GitFolder, discard_paths, fetch, initialize, parse_status, pull, push, run,
        validate_branch_name,
    };
    use std::fs;
    use std::path::{Path, PathBuf};

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let unique = format!(
                "ganbaru-git-service-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .expect("system time should follow Unix epoch")
                    .as_nanos()
            );
            let path = std::env::temp_dir().join(unique);
            fs::create_dir(&path).expect("temporary directory should be created");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    #[test]
    fn diagnostic_text_strips_controls_and_credentials_and_marks_truncation() {
        let raw = "fatal: \u{1b}[31mremote\u{1b}[0m https://user:token@example.com/org/repo.git\r\n\u{202e}gnirts\ttab\u{7}";
        assert_eq!(
            super::diagnostic_text(raw.as_bytes()),
            "fatal: [31mremote[0m https://example.com/org/repo.git\ngnirts\ttab"
        );
        assert_eq!(
            super::diagnostic_text(b"see ssh://git@host:22/x and git@github.com:org/repo"),
            "see ssh://host:22/x and git@github.com:org/repo"
        );

        let at_limit = "y".repeat(super::MAX_DIAGNOSTIC_CHARS);
        assert_eq!(super::diagnostic_text(at_limit.as_bytes()), at_limit);
        let over_limit = "x".repeat(super::MAX_DIAGNOSTIC_CHARS + 1);
        assert_eq!(
            super::diagnostic_text(over_limit.as_bytes()),
            format!("{} [truncated]", "x".repeat(super::MAX_DIAGNOSTIC_CHARS))
        );
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn parses_porcelain_v2_branch_and_paths() {
        let status = parse_status(
            b"# branch.head feature\0# branch.upstream origin/feature\0# branch.ab +2 -1\0\
              1 M. N... 100644 100644 100644 abc def src/staged.rs\0\
              ? src/new.rs\0",
        )
        .expect("status should parse");
        assert_eq!(status.branch.as_deref(), Some("feature"));
        assert_eq!(status.upstream.as_deref(), Some("origin/feature"));
        assert_eq!((status.ahead, status.behind), (2, 1));
        assert_eq!(status.files.len(), 2);
        assert_eq!(status.files[0].relative_path, "src/staged.rs");
        assert_eq!(status.files[0].index_status, "M");
        assert!(status.files[1].untracked);
    }

    #[tokio::test]
    async fn discard_restores_tracked_files_and_removes_confirmed_untracked_files() {
        let directory = TestDirectory::new();
        let root = directory.path();
        let folder = GitFolder::untrusted(root);
        initialize(folder, Some("main"))
            .await
            .expect("repository should initialize");
        run(
            folder,
            &["config", "user.name", "Ganbaru test"],
            GitCall::User,
        )
        .await
        .expect("user name should configure");
        run(
            folder,
            &["config", "user.email", "test@example.invalid"],
            GitCall::User,
        )
        .await
        .expect("user email should configure");
        run(
            folder,
            &["config", "commit.gpgsign", "false"],
            GitCall::User,
        )
        .await
        .expect("test signing should configure");
        fs::write(root.join("tracked.txt"), "base\n").expect("tracked file should write");
        run(folder, &["add", "--", "tracked.txt"], GitCall::User)
            .await
            .expect("tracked file should stage");
        run(
            folder,
            &["commit", "-m", "test: add fixture"],
            GitCall::User,
        )
        .await
        .expect("fixture should commit");

        fs::write(root.join("tracked.txt"), "changed\n").expect("tracked file should change");
        fs::write(root.join("untracked.txt"), "temporary\n").expect("untracked file should write");
        discard_paths(
            folder,
            &["tracked.txt".to_string()],
            &["untracked.txt".to_string()],
        )
        .await
        .expect("selected changes should discard");

        assert_eq!(
            fs::read_to_string(root.join("tracked.txt")).expect("tracked file should read"),
            "base\n"
        );
        assert!(!root.join("untracked.txt").exists());
    }

    #[test]
    fn branch_names_follow_ref_rules_and_refuse_refspec_syntax() {
        for name in [
            "main",
            "feature/login",
            "release-1.2",
            "user/fix_bug",
            "v2",
            "c++-port",
            "été",
        ] {
            assert!(
                validate_branch_name(name).is_ok(),
                "{name} should be accepted"
            );
        }
        for name in [
            "",
            "+main",
            "main:other",
            "refs/*",
            "main^",
            "main~1",
            "a..b",
            "-delete",
            "/main",
            "main/",
            "main.",
            "main.lock",
            "feature/.hidden",
            "a//b",
            "@",
            "main@{1}",
            "a b",
            "a?b",
            "a[b",
            "a\\b",
            "a\u{7}b",
        ] {
            assert!(
                validate_branch_name(name).is_err(),
                "{name:?} should be refused"
            );
        }
    }

    #[tokio::test]
    async fn network_operations_accept_only_configured_remotes() {
        let directory = TestDirectory::new();
        let root = directory.path();
        let folder = GitFolder::untrusted(root);
        initialize(folder, Some("main"))
            .await
            .expect("repository should initialize");
        let target = TestDirectory::new();
        let target_path = target
            .path()
            .to_str()
            .expect("temporary path should be UTF-8");

        for remote in [target_path, "https://example.invalid/repo.git", "origin"] {
            assert!(fetch(folder, Some(remote)).await.is_err(), "{remote} fetch");
            assert!(
                pull(folder, Some(remote), None).await.is_err(),
                "{remote} pull"
            );
            assert!(
                push(folder, Some(remote), Some("main"), false)
                    .await
                    .is_err(),
                "{remote} push"
            );
        }
        assert!(push(folder, None, Some("main"), false).await.is_err());
        assert!(pull(folder, None, Some("main")).await.is_err());
        assert!(
            fs::read_dir(target.path())
                .expect("target should list")
                .next()
                .is_none(),
            "nothing should reach the unconfigured target"
        );

        run(
            GitFolder::untrusted(target.path()),
            &["init", "--bare", "--quiet"],
            GitCall::User,
        )
        .await
        .expect("target repository should initialize");
        run(
            folder,
            &["remote", "add", "origin", target_path],
            GitCall::User,
        )
        .await
        .expect("remote should be configured");
        fetch(folder, Some("origin"))
            .await
            .expect("configured remotes should fetch");
    }
}
