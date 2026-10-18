//! Bounded Git process execution and typed porcelain parsing.

pub mod hardened;

use super::models::{ChatError, ChatErrorCode, ChatResult};
use hardened::{
    GIT_NETWORK_DEADLINE, GIT_READ_DEADLINE, GitPolicy, OwnedGitRun, PROVISIONAL_FOLDER_TRUST,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

const MAX_GIT_OUTPUT_BYTES: usize = 8 * 1024 * 1024;
const MAX_REVIEW_OUTPUT_BYTES: usize = 64 * 1024 * 1024;

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

pub async fn status(root: &Path) -> ChatResult<GitStatusRead> {
    let output = run(
        root,
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

pub async fn diff(root: &Path, staged: bool, relative_path: Option<&str>) -> ChatResult<String> {
    let mut arguments = vec!["diff", "--no-ext-diff", "--no-color"];
    if staged {
        arguments.push("--cached");
    }
    if let Some(path) = relative_path {
        arguments.extend(["--", path]);
    }
    let output = run(root, &arguments, GitCall::Internal).await?;
    String::from_utf8(output.stdout)
        .map_err(|_| git_protocol_error("Git diff output is not valid UTF-8"))
}

pub async fn stage(root: &Path, paths: &[String]) -> ChatResult<()> {
    let mut arguments = vec!["add", "--"];
    arguments.extend(paths.iter().map(String::as_str));
    run(root, &arguments, GitCall::User).await.map(|_| ())
}

pub async fn unstage(root: &Path, paths: &[String]) -> ChatResult<()> {
    let source = match run(
        root,
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
                root,
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
    run(root, &arguments, GitCall::User).await.map(|_| ())
}

pub async fn commit(root: &Path, message: &str) -> ChatResult<()> {
    run(root, &["commit", "-m", message], GitCall::User)
        .await
        .map(|_| ())
}

pub async fn fetch(root: &Path, remote: Option<&str>) -> ChatResult<()> {
    let mut arguments = vec!["fetch", "--prune"];
    if let Some(remote) = remote {
        arguments.push(remote);
    }
    run(root, &arguments, GitCall::UserNetwork)
        .await
        .map(|_| ())
}

pub async fn pull(root: &Path, remote: Option<&str>, branch: Option<&str>) -> ChatResult<()> {
    let mut arguments = vec!["pull", "--ff-only"];
    if let Some(remote) = remote {
        arguments.push(remote);
    }
    if let Some(branch) = branch {
        arguments.push(branch);
    }
    run(root, &arguments, GitCall::UserNetwork)
        .await
        .map(|_| ())
}

pub async fn push(
    root: &Path,
    remote: Option<&str>,
    branch: Option<&str>,
    force_with_lease: bool,
) -> ChatResult<()> {
    let mut arguments = vec!["push"];
    if force_with_lease {
        arguments.push("--force-with-lease");
    }
    if let Some(remote) = remote {
        arguments.push(remote);
    }
    if let Some(branch) = branch {
        arguments.push(branch);
    }
    run(root, &arguments, GitCall::UserNetwork)
        .await
        .map(|_| ())
}

pub async fn remotes(root: &Path) -> ChatResult<Vec<GitRemoteRead>> {
    let output = run(root, &["remote", "-v"], GitCall::Internal).await?;
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

pub async fn branches(root: &Path) -> ChatResult<Vec<GitBranchRead>> {
    let output = run(
        root,
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

pub async fn worktrees(root: &Path) -> ChatResult<Vec<GitWorktreeRead>> {
    let output = run(
        root,
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

pub async fn common_directory(root: &Path) -> ChatResult<PathBuf> {
    let output = run(
        root,
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
    root: &Path,
    worktree_path: &str,
    branch: &str,
    base_reference: &str,
) -> ChatResult<()> {
    run(
        root,
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

pub async fn remove_worktree(root: &Path, worktree_path: &str) -> ChatResult<()> {
    run(
        root,
        &["worktree", "remove", worktree_path],
        GitCall::Internal,
    )
    .await
    .map(|_| ())
}

pub async fn head_object_id(root: &Path) -> ChatResult<String> {
    let output = run(root, &["rev-parse", "HEAD"], GitCall::Internal).await?;
    String::from_utf8(output.stdout)
        .map(|value| value.trim().to_string())
        .map_err(|_| git_protocol_error("Git object ID is not valid UTF-8"))
}

pub async fn initialize(root: &Path, initial_branch: Option<&str>) -> ChatResult<()> {
    let mut arguments = vec!["init"];
    if let Some(branch) = initial_branch {
        arguments.extend(["--initial-branch", branch]);
    }
    run(root, &arguments, GitCall::User).await.map(|_| ())
}

pub async fn clone_into(root: &Path, remote_url: &str, remote_name: &str) -> ChatResult<()> {
    validate_git_argument(remote_url)?;
    validate_git_argument(remote_name)?;
    run(
        root,
        &["clone", "--origin", remote_name, "--", remote_url, "."],
        GitCall::UserNetwork,
    )
    .await
    .map(|_| ())
}

pub async fn discard_paths(
    root: &Path,
    tracked_paths: &[String],
    untracked_paths: &[String],
) -> ChatResult<()> {
    if !tracked_paths.is_empty() {
        let mut arguments = vec!["restore", "--worktree", "--"];
        arguments.extend(tracked_paths.iter().map(String::as_str));
        run(root, &arguments, GitCall::User).await?;
    }
    if !untracked_paths.is_empty() {
        let mut arguments = vec!["clean", "-f", "--"];
        arguments.extend(untracked_paths.iter().map(String::as_str));
        run(root, &arguments, GitCall::User).await?;
    }
    Ok(())
}

pub async fn delete_branch(root: &Path, branch: &str, force: bool) -> ChatResult<()> {
    validate_git_argument(branch)?;
    run(
        root,
        &["branch", if force { "-D" } else { "-d" }, branch],
        GitCall::User,
    )
    .await
    .map(|_| ())
}

pub async fn checkout_bitbucket_pull_request(
    root: &Path,
    remote_name: &str,
    reference: &str,
) -> ChatResult<()> {
    validate_git_argument(remote_name)?;
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
        root,
        &["fetch", remote_name, &refspec],
        GitCall::UserNetwork,
    )
    .await?;
    let local_branch = format!("pull-request-{reference}");
    run(
        root,
        &["checkout", "-b", &local_branch, &remote_tracking_ref],
        GitCall::User,
    )
    .await
    .map(|_| ())
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
    fn policy(self) -> GitPolicy {
        match self {
            Self::Internal => GitPolicy::internal(PROVISIONAL_FOLDER_TRUST),
            Self::User | Self::UserNetwork => GitPolicy::user(PROVISIONAL_FOLDER_TRUST),
        }
    }

    fn deadline(self) -> Duration {
        match self {
            Self::Internal | Self::User => GIT_READ_DEADLINE,
            Self::UserNetwork => GIT_NETWORK_DEADLINE,
        }
    }
}

async fn run(root: &Path, arguments: &[&str], call: GitCall) -> ChatResult<GitOutput> {
    run_bounded(root, arguments, call, None, MAX_GIT_OUTPUT_BYTES).await
}

pub async fn review_output(
    root: &Path,
    arguments: &[&str],
    input: Option<&[u8]>,
) -> ChatResult<Vec<u8>> {
    run_bounded(
        root,
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
    root: &Path,
    arguments: &[&str],
    input: Option<&[u8]>,
) -> ChatResult<Vec<u8>> {
    run_bounded(
        root,
        arguments,
        GitCall::User,
        input,
        MAX_REVIEW_OUTPUT_BYTES,
    )
    .await
    .map(|output| output.stdout)
}

pub async fn review_output_in_storage(
    root: &Path,
    arguments: &[&str],
    input: Option<&[u8]>,
    index_file: Option<&Path>,
    object_directory: Option<&Path>,
    alternate_object_directory: Option<&Path>,
) -> ChatResult<Vec<u8>> {
    run_bounded_with_storage(
        root,
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
    root: &Path,
    arguments: &[&str],
    output_path: &Path,
    maximum_output_bytes: usize,
    object_directory: Option<&Path>,
    alternate_object_directory: Option<&Path>,
) -> ChatResult<u64> {
    run_bounded_with_storage(
        root,
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
    root: &Path,
    arguments: &[&str],
    call: GitCall,
    input: Option<&[u8]>,
    maximum_output_bytes: usize,
) -> ChatResult<GitOutput> {
    run_bounded_with_storage(
        root,
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
    root: &Path,
    arguments: &[&str],
    call: GitCall,
    input: Option<&[u8]>,
    maximum_output_bytes: usize,
    stdout_file: Option<&Path>,
    storage: GitStorage<'_>,
) -> ChatResult<GitOutput> {
    let output = hardened::run(OwnedGitRun {
        root: root.to_path_buf(),
        arguments: arguments.iter().map(OsString::from).collect(),
        policy: call.policy(),
        deadline: call.deadline(),
        input: input.map(<[u8]>::to_vec),
        maximum_output_bytes,
        stdout_file: stdout_file.map(Path::to_path_buf),
        environment: storage_environment(storage)?,
    })
    .await?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
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
    use super::{GitCall, discard_paths, initialize, parse_status, run};
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
        initialize(root, Some("main"))
            .await
            .expect("repository should initialize");
        run(
            root,
            &["config", "user.name", "Ganbaru test"],
            GitCall::User,
        )
        .await
        .expect("user name should configure");
        run(
            root,
            &["config", "user.email", "test@example.invalid"],
            GitCall::User,
        )
        .await
        .expect("user email should configure");
        run(root, &["config", "commit.gpgsign", "false"], GitCall::User)
            .await
            .expect("test signing should configure");
        fs::write(root.join("tracked.txt"), "base\n").expect("tracked file should write");
        run(root, &["add", "--", "tracked.txt"], GitCall::User)
            .await
            .expect("tracked file should stage");
        run(root, &["commit", "-m", "test: add fixture"], GitCall::User)
            .await
            .expect("fixture should commit");

        fs::write(root.join("tracked.txt"), "changed\n").expect("tracked file should change");
        fs::write(root.join("untracked.txt"), "temporary\n").expect("untracked file should write");
        discard_paths(
            root,
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
}
