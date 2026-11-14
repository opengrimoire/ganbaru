//! The commit, remote, and destination branch a push would send, read without a network call,
//! and the push of exactly that commit.
//!
//! A teammate's push request records a [`GitPushTarget`]; a person confirms it, and the push
//! sends the recorded commit to the recorded destination only when a fresh read still names
//! the same target. The push names the commit object, never a local ref, so a branch that
//! moved after the person looked cannot change what reaches the remote, and it never forces.

use super::{GitCall, GitFolder, require_configured_remote, run, status, validate_branch_name};
use crate::models::{ChatError, ChatResult};
use serde::{Deserialize, Serialize};

/// Prefix of the branch refs a push may update on the remote.
const BRANCH_REF_PREFIX: &str = "refs/heads/";
/// Longest push URL a target may name, in bytes.
const MAX_PUSH_URL_BYTES: usize = 2_048;

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
    /// The push URL carries a password, or a destination is not a plain branch.
    Unsupported,
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
    let push_url = push_url(folder, &remote).await?;
    if !is_supported_push_url(&push_url) {
        return Err(GitPushTargetError::Unsupported);
    }
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
/// # Errors
///
/// Returns a validation error for a malformed target, and a conflict carrying Git's output
/// when the push fails, as when the remote branch moved past the commit.
pub async fn push_object(folder: GitFolder<'_>, target: &GitPushTarget) -> ChatResult<()> {
    validate_branch_name(&target.destination_branch)?;
    if !is_object_id(&target.object_id) {
        return Err(ChatError::validation(
            "objectId",
            "Git object ID is invalid",
        ));
    }
    require_configured_remote(folder, &target.remote).await?;
    let refspec = format!(
        "{}:{BRANCH_REF_PREFIX}{}",
        target.object_id, target.destination_branch
    );
    run(
        folder,
        &[
            "push",
            "--no-follow-tags",
            "--recurse-submodules=no",
            "--",
            &target.remote,
            &refspec,
        ],
        GitCall::UserNetwork,
    )
    .await
    .map(|_| ())
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

/// The URL a push to `remote` reaches, after `pushurl`, `insteadOf`, and `pushInsteadOf`.
async fn push_url(folder: GitFolder<'_>, remote: &str) -> ChatResult<String> {
    let output = run(
        folder,
        &["remote", "get-url", "--push", "--", remote],
        GitCall::Internal,
    )
    .await?;
    let text = String::from_utf8(output.stdout).map_err(|_| unexpected_output())?;
    let mut lines = text.lines();
    match (lines.next(), lines.next()) {
        (Some(url), None) if !url.is_empty() => Ok(url.to_string()),
        _ => Err(unexpected_output()),
    }
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

/// Whether a push may reach `url`: it is bounded, printable, and carries no password, so a
/// push uses only credentials Git finds in the person's own configuration. A user name alone,
/// as in `ssh://git@host/repo`, is allowed.
pub fn is_supported_push_url(url: &str) -> bool {
    if url.is_empty()
        || url.len() > MAX_PUSH_URL_BYTES
        || url
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
    {
        return false;
    }
    let Some((_, after_scheme)) = url.split_once("://") else {
        return true;
    };
    let authority_end = after_scheme
        .find(['/', '?', '#'])
        .unwrap_or(after_scheme.len());
    match after_scheme[..authority_end].rsplit_once('@') {
        Some((userinfo, _)) => !userinfo.contains(':'),
        None => true,
    }
}

fn unexpected_output() -> ChatError {
    super::git_protocol_error("Git printed an unexpected push target")
}

#[cfg(test)]
mod tests;
