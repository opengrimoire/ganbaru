//! The commit, remote, and destination branch a push would send, read without a network call,
//! and the push of exactly that commit.
//!
//! A teammate's push request records a [`GitPushTarget`]; a person confirms it, and the push
//! sends the recorded commit to the recorded destination only when a fresh read still names
//! the same target. The push names the commit object, never a local ref, so a branch that
//! moved after the person looked cannot change what reaches the remote, and it never forces.

use super::hardened::{empty_hooks_directory, requested_configuration, shell_quote};
use super::{GitCall, GitFolder, require_configured_remote, run, status, validate_branch_name};
use crate::models::{ChatError, ChatErrorCode, ChatResult};
use serde::{Deserialize, Serialize};

/// Prefix of the branch refs a push may update on the remote.
const BRANCH_REF_PREFIX: &str = "refs/heads/";
/// Longest push URL a target may name, in bytes.
const MAX_PUSH_URL_BYTES: usize = 2_048;
/// URL schemes a push may name; every other scheme is refused.
const SUPPORTED_URL_SCHEMES: &[&str] =
    &["https", "http", "ssh", "git", "git+ssh", "ssh+git", "file"];

/// What one push would send: the commit checked out on `local_branch`, to the branch
/// `destination_branch` of `remote`, whose push URL after configured rewrites is `push_url`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitPushTarget {
    pub local_branch: String,
    pub remote: String,
    pub destination_branch: String,
    pub push_url: String,
    pub object_id: String,
}

/// Why a folder has no push target.
#[derive(Debug)]
pub enum GitPushTargetError {
    /// No branch is checked out.
    Detached,
    /// The branch has no push or upstream remote and none was named, or the named remote is not
    /// configured.
    NoRemote,
    /// The remote has several push URLs, the push URL carries a password or names a transport
    /// other than HTTPS, HTTP, SSH, Git, or a local path, or a destination is not a plain branch.
    Unsupported,
    /// Repository configuration chooses how Git connects or authenticates (a credential helper,
    /// an SSH or proxy command, a proxy, or TLS trust), which a teammate could have written.
    RepositoryConnection,
    /// Git failed or printed something unexpected.
    Failed(ChatError),
}

impl From<ChatError> for GitPushTargetError {
    fn from(error: ChatError) -> Self {
        Self::Failed(error)
    }
}

/// Reads what a push of the checked out branch to `remote`, or to its configured push or
/// upstream remote when `remote` is `None`, would send. The destination is the branch's
/// configured push or upstream branch on that remote, else the branch of the same name.
///
/// # Errors
///
/// Returns a [`GitPushTargetError`] naming why there is no target.
pub async fn push_target(
    folder: GitFolder<'_>,
    remote: Option<&str>,
) -> Result<GitPushTarget, GitPushTargetError> {
    let local_branch = match status(folder).await? {
        status if status.detached => return Err(GitPushTargetError::Detached),
        status => status.branch.ok_or(GitPushTargetError::Detached)?,
    };
    validate_branch_name(&local_branch).map_err(|_| GitPushTargetError::Unsupported)?;
    let tracking = branch_tracking(folder, &local_branch).await?;
    let (remote, destination_ref) = match remote {
        Some(remote) => {
            require_configured_remote(folder, remote)
                .await
                .map_err(|_| GitPushTargetError::NoRemote)?;
            (remote.to_string(), tracking.destination_on(remote))
        }
        None => tracking
            .default_destination()
            .ok_or(GitPushTargetError::NoRemote)?,
    };
    let destination_branch = match destination_ref {
        Some(reference) => reference
            .strip_prefix(BRANCH_REF_PREFIX)
            .ok_or(GitPushTargetError::Unsupported)?
            .to_string(),
        None => local_branch.clone(),
    };
    validate_branch_name(&destination_branch).map_err(|_| GitPushTargetError::Unsupported)?;
    require_no_repository_connection(folder).await?;
    let push_url = single_push_url(folder, &remote).await?;
    let object_id = head_commit(folder).await?;
    Ok(GitPushTarget {
        local_branch,
        remote,
        destination_branch,
        push_url,
        object_id,
    })
}

/// Pushes the commit `target` names to its destination branch, without forcing, tags, or
/// submodules. The remote is named, never a URL, and the destination is a full branch ref.
///
/// The push runs as a confirmed teammate request (see [`super::hardened::GitOrigin::Requested`]):
/// repository configuration and hooks never apply, whatever this device's trust record says,
/// and only standard transports are allowed. A local remote runs its `receive-pack` on this
/// device, so it runs without the remote repository's hooks and programs as well.
///
/// # Errors
///
/// Returns a validation error for a malformed target, a conflict when repository configuration
/// chooses how Git connects or the push URL is no longer the recorded one, and a conflict
/// carrying Git's output when the push fails, as when the remote branch moved past the commit.
pub async fn push_object(folder: GitFolder<'_>, target: &GitPushTarget) -> ChatResult<()> {
    validate_branch_name(&target.destination_branch)?;
    if !is_object_id(&target.object_id) {
        return Err(ChatError::validation(
            "objectId",
            "Git object ID is invalid",
        ));
    }
    require_configured_remote(folder, &target.remote).await?;
    let push_url = match single_push_url(folder, &target.remote).await {
        Ok(url) if url == target.push_url => url,
        Ok(_) | Err(GitPushTargetError::Unsupported) => {
            return Err(push_conflict("Git push URL changed"));
        }
        Err(GitPushTargetError::Failed(error)) => return Err(error),
        Err(_) => return Err(push_conflict("Git push target changed")),
    };
    match require_no_repository_connection(folder).await {
        Ok(()) => {}
        Err(GitPushTargetError::Failed(error)) => return Err(error),
        Err(_) => {
            return Err(push_conflict(
                "Repository configuration chooses how Git connects",
            ));
        }
    }
    let refspec = format!(
        "{}:{BRANCH_REF_PREFIX}{}",
        target.object_id, target.destination_branch
    );
    let receive_pack = is_local_push_url(&push_url).then(local_receive_pack);
    let mut arguments = vec!["push", "--no-follow-tags", "--recurse-submodules=no"];
    if let Some(receive_pack) = &receive_pack {
        arguments.push(receive_pack);
    }
    arguments.extend(["--", &target.remote, &refspec]);
    run(folder, &arguments, GitCall::Requested)
        .await
        .map(|_| ())
}

/// The `--receive-pack` option for a remote on this device: its repository's hooks, fsmonitor,
/// alternate refs command, automatic maintenance, and checked out worktree updates never run.
/// Git clears `-c` values before starting the local `receive-pack`, so they are part of the
/// command. The command is run by the shell.
fn local_receive_pack() -> String {
    let hooks = shell_quote(empty_hooks_directory().as_os_str());
    format!(
        "--receive-pack=git -c core.hooksPath={} -c core.fsmonitor= -c core.alternateRefsCommand= -c receive.autogc=false -c receive.denyCurrentBranch=refuse receive-pack",
        hooks.to_string_lossy()
    )
}

/// Refuses a folder whose repository configuration chooses how Git connects or authenticates.
async fn require_no_repository_connection(folder: GitFolder<'_>) -> Result<(), GitPushTargetError> {
    let configuration = requested_configuration(folder.root.to_path_buf())
        .await
        .map_err(ChatError::from)?;
    if configuration.repository_connection {
        Err(GitPushTargetError::RepositoryConnection)
    } else {
        Ok(())
    }
}

/// The one URL a push to `remote` reaches; a remote with several push URLs is unsupported,
/// because a push reaches all of them.
async fn single_push_url(
    folder: GitFolder<'_>,
    remote: &str,
) -> Result<String, GitPushTargetError> {
    let mut urls = push_urls(folder, remote).await?;
    match (urls.pop(), urls.is_empty()) {
        (Some(url), true) if is_supported_push_url(&url) => Ok(url),
        _ => Err(GitPushTargetError::Unsupported),
    }
}

fn push_conflict(message: &str) -> ChatError {
    ChatError::new(ChatErrorCode::Conflict, message, true)
}

/// The push and upstream remotes and remote refs Git resolves for one branch; empty fields are
/// unset.
#[derive(Debug, Default, Eq, PartialEq)]
struct BranchTracking {
    push_remote: String,
    push_ref: String,
    upstream_remote: String,
    upstream_ref: String,
}

impl BranchTracking {
    /// The configured remote and destination ref when no remote is named: the push remote, else
    /// the upstream remote.
    fn default_destination(&self) -> Option<(String, Option<String>)> {
        [&self.push_remote, &self.upstream_remote]
            .into_iter()
            .find(|remote| !remote.is_empty())
            .map(|remote| (remote.clone(), self.destination_on(remote)))
    }

    /// The configured destination ref on `remote`: the push ref Git resolved there, else the
    /// upstream branch there, or `None` when neither names one. Git leaves the push ref empty
    /// under some `push.default` modes even when the branch tracks a branch of another name.
    fn destination_on(&self, remote: &str) -> Option<String> {
        [
            (&self.push_remote, &self.push_ref),
            (&self.upstream_remote, &self.upstream_ref),
        ]
        .into_iter()
        .find(|(configured, reference)| configured.as_str() == remote && !reference.is_empty())
        .map(|(_, reference)| reference.clone())
    }

    fn parse(output: &str) -> Option<Self> {
        let mut fields = output.trim_end_matches('\n').split('\0');
        let tracking = Self {
            push_remote: fields.next()?.to_string(),
            push_ref: fields.next()?.to_string(),
            upstream_remote: fields.next()?.to_string(),
            upstream_ref: fields.next()?.to_string(),
        };
        fields.next().is_none().then_some(tracking)
    }
}

async fn branch_tracking(folder: GitFolder<'_>, branch: &str) -> ChatResult<BranchTracking> {
    let reference = format!("{BRANCH_REF_PREFIX}{branch}");
    let output = run(
        folder,
        &[
            "for-each-ref",
            "--format=%(push:remotename)%00%(push:remoteref)%00%(upstream:remotename)%00%(upstream:remoteref)",
            "--",
            &reference,
        ],
        GitCall::Internal,
    )
    .await?;
    let text = String::from_utf8(output.stdout).map_err(|_| unexpected_output())?;
    if text.is_empty() {
        return Ok(BranchTracking::default());
    }
    BranchTracking::parse(&text).ok_or_else(unexpected_output)
}

/// Every URL a push to `remote` reaches, after `pushurl`, `insteadOf`, and `pushInsteadOf`.
async fn push_urls(folder: GitFolder<'_>, remote: &str) -> ChatResult<Vec<String>> {
    let output = run(
        folder,
        &["remote", "get-url", "--push", "--all", "--", remote],
        GitCall::Requested,
    )
    .await?;
    let text = String::from_utf8(output.stdout).map_err(|_| unexpected_output())?;
    let urls = text.lines().map(str::to_string).collect::<Vec<_>>();
    if urls.is_empty() || urls.iter().any(String::is_empty) {
        return Err(unexpected_output());
    }
    Ok(urls)
}

async fn head_commit(folder: GitFolder<'_>) -> ChatResult<String> {
    let output = run(
        folder,
        &["rev-parse", "--verify", "HEAD^{commit}"],
        GitCall::Internal,
    )
    .await?;
    let text = String::from_utf8(output.stdout).map_err(|_| unexpected_output())?;
    let object_id = text.trim();
    if is_object_id(object_id) {
        Ok(object_id.to_string())
    } else {
        Err(unexpected_output())
    }
}

/// Whether `value` is a full SHA-1 or SHA-256 object id in lowercase hexadecimal.
pub fn is_object_id(value: &str) -> bool {
    matches!(value.len(), 40 | 64)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Whether a push may reach `url`: it is bounded, printable, carries no password, so a push uses
/// only credentials Git finds in the person's own configuration, and names HTTPS, HTTP, SSH, Git,
/// or a local path, never a remote helper (`<transport>::<address>`). A user name alone, as in
/// `ssh://git@host/repo`, is allowed.
pub fn is_supported_push_url(url: &str) -> bool {
    if url.is_empty()
        || url.len() > MAX_PUSH_URL_BYTES
        || url
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
    {
        return false;
    }
    let (scheme, separator) = url_scheme(url);
    match separator {
        Some("::") => return false,
        Some(_) if !SUPPORTED_URL_SCHEMES.contains(&scheme.to_ascii_lowercase().as_str()) => {
            return false;
        }
        Some(_) => {}
        None => return true,
    }
    let after_scheme = &url[scheme.len() + 3..];
    let authority_end = after_scheme
        .find(['/', '?', '#'])
        .unwrap_or(after_scheme.len());
    match after_scheme[..authority_end].rsplit_once('@') {
        Some((userinfo, _)) => !userinfo.contains(':'),
        None => true,
    }
}

/// Whether a push to `url` runs `receive-pack` on this device, following Git's rule: a
/// `file://` URL, or a path that is not `host:path` (no colon, a slash before the first colon,
/// or a drive prefix on Windows).
fn is_local_push_url(url: &str) -> bool {
    match url_scheme(url) {
        (scheme, Some("://")) => scheme.eq_ignore_ascii_case("file"),
        (_, Some(_)) => false,
        (_, None) => match url.find(':') {
            None => true,
            Some(colon) => {
                url.find('/').is_some_and(|slash| slash < colon) || has_drive_prefix(url)
            }
        },
    }
}

#[cfg(windows)]
fn has_drive_prefix(url: &str) -> bool {
    let bytes = url.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

#[cfg(not(windows))]
fn has_drive_prefix(_url: &str) -> bool {
    false
}

/// The leading scheme of `url` and the separator after it (`://` for a URL, `::` for a remote
/// helper), or no separator when `url` is a path or `host:path`.
fn url_scheme(url: &str) -> (&str, Option<&'static str>) {
    let length = url
        .bytes()
        .enumerate()
        .take_while(|(index, byte)| {
            byte.is_ascii_alphanumeric() || (*index > 0 && matches!(byte, b'+' | b'-' | b'.'))
        })
        .count();
    let scheme = &url[..length];
    if length == 0 {
        return (scheme, None);
    }
    let rest = &url[length..];
    if rest.starts_with("::") {
        (scheme, Some("::"))
    } else if rest.starts_with("://") {
        (scheme, Some("://"))
    } else {
        (scheme, None)
    }
}

fn unexpected_output() -> ChatError {
    super::git_protocol_error("Git printed an unexpected push target")
}

#[cfg(test)]
mod tests;
