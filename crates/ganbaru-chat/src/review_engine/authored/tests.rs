use super::*;
use crate::review_engine::tests::TestRepository;
use chrono::SecondsFormat;
use std::process::Command;

const BASE_EPOCH: i64 = 1_000_000_000;
const CAPTURE_EPOCH: i64 = BASE_EPOCH + 1_000;
const AGENT_EPOCH: i64 = BASE_EPOCH + 2_000;

/// Commits every change with author and committer dates fixed to `epoch`.
fn commit_at(repository: &TestRepository, message: &str, epoch: i64) {
    repository.run_git(&["add", "-A", "--", "."]);
    let date = format!("@{epoch} +0000");
    let output = Command::new("git")
        .arg("-C")
        .arg(repository.path())
        .args(["commit", "-q", "-m", message])
        .env("GIT_AUTHOR_DATE", &date)
        .env("GIT_COMMITTER_DATE", &date)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .expect("Git fixture commit should start");
    assert!(
        output.status.success(),
        "Git fixture commit failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Captures the working tree as a checkpoint commit whose parent is HEAD.
fn checkpoint(repository: &TestRepository, id: &str) -> StoredCheckpoint {
    repository.run_git(&["add", "-A", "--", "."]);
    let tree = repository.run_git(&["write-tree"]);
    let head = repository.run_git(&["rev-parse", "HEAD"]);
    let commit = repository.run_git(&["commit-tree", &tree, "-p", &head, "-m", "checkpoint"]);
    StoredCheckpoint {
        id: ChatCheckpointId::new(id).unwrap(),
        thread_id: ChatThreadId::new("thread:authored").unwrap(),
        turn_count: 1,
        repository_identity: "repository".to_string(),
        hidden_ref_name: format!("refs/ganbaru/chat/{id}"),
        git_object_id: commit,
        index_tree_oid: tree.clone(),
        worktree_tree_oid: tree,
        head_oid: Some(head),
        head_ref: Some("refs/heads/main".to_string()),
    }
}

fn captured_at(epoch: i64) -> String {
    DateTime::from_timestamp(epoch, 0)
        .unwrap()
        .to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn computed(paths: &[&str]) -> AuthoredPaths {
    AuthoredPaths::Computed(paths.iter().map(|path| path.to_string()).collect())
}

fn base_repository() -> TestRepository {
    let repository = TestRepository::new();
    repository.write("README.md", "base\n");
    commit_at(&repository, "base", BASE_EPOCH);
    repository
}

#[tokio::test]
async fn an_unmoved_head_needs_no_authored_set() {
    let repository = base_repository();
    let pre = checkpoint(&repository, "checkpoint:pre");
    repository.write("src/work.rs", "work\n");
    let post = checkpoint(&repository, "checkpoint:post");

    let authored = authored_paths(
        repository.folder(),
        &pre,
        &post,
        &captured_at(CAPTURE_EPOCH),
    )
    .await;

    assert_eq!(authored, AuthoredPaths::NotNeeded);
    assert_eq!(authored.status(), ReviewAuthoredPathsStatus::NotNeeded);
}

#[tokio::test]
async fn commits_made_during_the_run_and_uncommitted_changes_are_authored() {
    let repository = base_repository();
    repository.write("notes/draft.md", "draft before the run\n");
    let pre = checkpoint(&repository, "checkpoint:pre");
    repository.write("src/agent.rs", "agent work\n");
    commit_at(&repository, "agent work", AGENT_EPOCH);
    repository.write("src/pending.rs", "uncommitted\n");
    let post = checkpoint(&repository, "checkpoint:post");

    let authored = authored_paths(
        repository.folder(),
        &pre,
        &post,
        &captured_at(CAPTURE_EPOCH),
    )
    .await;

    assert_eq!(
        authored,
        computed(&["notes/draft.md", "src/agent.rs", "src/pending.rs"])
    );
}

#[tokio::test]
async fn fast_forwarded_upstream_commits_from_before_the_run_are_not_authored() {
    let repository = base_repository();
    repository.run_git(&["branch", "-M", "main"]);
    repository.run_git(&["checkout", "-q", "-b", "upstream"]);
    repository.write("vendor/imported.rs", "imported\n");
    commit_at(&repository, "upstream work", BASE_EPOCH + 500);
    repository.run_git(&["checkout", "-q", "main"]);
    let pre = checkpoint(&repository, "checkpoint:pre");
    repository.run_git(&["merge", "-q", "--ff-only", "upstream"]);
    repository.write("src/agent.rs", "agent work\n");
    commit_at(&repository, "agent work", AGENT_EPOCH);
    let post = checkpoint(&repository, "checkpoint:post");

    let authored = authored_paths(
        repository.folder(),
        &pre,
        &post,
        &captured_at(CAPTURE_EPOCH),
    )
    .await;

    assert_eq!(authored, computed(&["src/agent.rs"]));
    assert!(!authored.is_authored("vendor/imported.rs", None, false));
}

#[tokio::test]
async fn commits_head_dropped_are_authored() {
    let repository = base_repository();
    repository.write("src/dropped.rs", "dropped\n");
    commit_at(&repository, "later reset away", BASE_EPOCH + 100);
    let pre = checkpoint(&repository, "checkpoint:pre");
    repository.run_git(&["reset", "-q", "--hard", "HEAD~1"]);
    let post = checkpoint(&repository, "checkpoint:post");

    let authored = authored_paths(
        repository.folder(),
        &pre,
        &post,
        &captured_at(CAPTURE_EPOCH),
    )
    .await;

    assert_eq!(authored, computed(&["src/dropped.rs"]));
}

#[tokio::test]
async fn output_over_the_cap_leaves_every_file_visible() {
    let repository = base_repository();
    let pre = checkpoint(&repository, "checkpoint:pre");
    repository.write("src/agent.rs", "agent work\n");
    commit_at(&repository, "agent work", AGENT_EPOCH);
    let post = checkpoint(&repository, "checkpoint:post");

    let authored = authored_paths_with_limit(
        repository.folder(),
        &pre,
        &post,
        &captured_at(CAPTURE_EPOCH),
        4,
    )
    .await;

    assert_eq!(
        authored,
        AuthoredPaths::Unavailable(AuthoredPathsUnavailable::OutputTooLarge)
    );
    assert_eq!(authored.status(), ReviewAuthoredPathsStatus::Unavailable);
    assert!(authored.is_authored("anything.rs", None, false));
}

#[tokio::test]
async fn an_unknown_object_id_marks_the_set_unavailable() {
    let repository = base_repository();
    let pre = checkpoint(&repository, "checkpoint:pre");
    repository.write("src/agent.rs", "agent work\n");
    commit_at(&repository, "agent work", AGENT_EPOCH);
    let mut post = checkpoint(&repository, "checkpoint:post");
    post.git_object_id = "f".repeat(40);

    let authored = authored_paths(
        repository.folder(),
        &pre,
        &post,
        &captured_at(CAPTURE_EPOCH),
    )
    .await;

    assert_eq!(
        authored,
        AuthoredPaths::Unavailable(AuthoredPathsUnavailable::GitFailed)
    );
}

#[tokio::test]
async fn an_unparseable_capture_time_marks_the_set_unavailable() {
    let repository = base_repository();
    let pre = checkpoint(&repository, "checkpoint:pre");
    repository.write("src/agent.rs", "agent work\n");
    commit_at(&repository, "agent work", AGENT_EPOCH);
    let post = checkpoint(&repository, "checkpoint:post");

    let authored = authored_paths(repository.folder(), &pre, &post, "yesterday").await;

    assert_eq!(
        authored,
        AuthoredPaths::Unavailable(AuthoredPathsUnavailable::GitFailed)
    );
}

#[test]
fn a_rename_counts_as_authored_through_its_previous_path() {
    let authored = computed(&["src/old.rs"]);

    assert!(authored.is_authored("src/new.rs", Some("src/old.rs"), false));
    assert!(!authored.is_authored("src/new.rs", Some("src/other.rs"), false));
}

#[test]
fn provider_reported_files_are_authored_outside_the_set() {
    let authored = computed(&["src/agent.rs"]);

    assert!(authored.is_authored("src/reported.rs", None, true));
    assert!(!authored.is_authored("src/reported.rs", None, false));
}

#[test]
fn name_lists_split_on_nul_and_skip_empty_fields() {
    let names = parse_name_list(b"src/a.rs\0src/b c.rs\0\0src/a.rs\0");

    assert_eq!(
        names,
        ["src/a.rs", "src/b c.rs"]
            .iter()
            .map(|name| name.to_string())
            .collect::<BTreeSet<_>>()
    );
}
