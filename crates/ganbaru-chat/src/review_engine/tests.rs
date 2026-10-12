//! Focused review engine behavior and Git fixture tests.

use super::*;
use std::collections::BTreeMap;
use std::fs;
use std::process::Command;

struct TestRepository(PathBuf);

impl TestRepository {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("test clock should follow the Unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "ganbaru-chat-review-test-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("test repository should be created");
        let repository =
            Self(fs::canonicalize(path).expect("test repository path should canonicalize"));
        repository.run_git(&["init", "-q"]);
        repository.run_git(&["config", "user.name", "Ganbaru test"]);
        repository.run_git(&["config", "user.email", "test@ganbaru.invalid"]);
        repository.run_git(&["config", "commit.gpgsign", "false"]);
        repository
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn write(&self, relative_path: &str, contents: impl AsRef<[u8]>) {
        let path = self.0.join(relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("test file parent should be created");
        }
        fs::write(path, contents).expect("test file should be written");
    }

    fn run_git(&self, arguments: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(&self.0)
            .args(arguments)
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .expect("Git fixture command should start");
        assert!(
            output.status.success(),
            "Git fixture command failed: {arguments:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout)
            .expect("Git fixture output should be UTF-8")
            .trim()
            .to_string()
    }

    fn commit_all(&self) {
        self.run_git(&["add", "-A", "--", "."]);
        self.run_git(&["commit", "-q", "-m", "test: add review fixture"]);
    }
}

impl Drop for TestRepository {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn working_source(mode: ReviewWorkingTreeMode) -> ReviewDiffSource {
    ReviewDiffSource::WorkingTree { mode }
}

fn test_file(
    source: &ReviewDiffSource,
    file_id: &str,
    relative_path: &str,
    status: &str,
    additions: Option<u64>,
    deletions: Option<u64>,
    binary: bool,
) -> ReviewFileInternal {
    let (flags, capabilities, capability_reasons) =
        file_contract(source, status, additions, deletions, binary);
    ReviewFileInternal {
        read: ReviewFileRead {
            file_id: file_id.to_string(),
            relative_path: relative_path.to_string(),
            previous_relative_path: None,
            status: status.to_string(),
            additions,
            deletions,
            flags,
            capabilities,
            capability_reasons,
        },
    }
}

fn provider_snapshot(patch: &str) -> (Arc<ReviewSnapshot>, String) {
    let source = ReviewDiffSource::ProviderTurn {
        turn_id: ChatTurnId::new("turn:review-test").expect("turn ID should be valid"),
    };
    let revision = "a".repeat(64);
    let file_id = file_id(&revision, "src/lib.rs", None);
    let file = test_file(
        &source,
        &file_id,
        "src/lib.rs",
        "modified",
        Some(2),
        Some(2),
        false,
    );
    let snapshot = Arc::new(ReviewSnapshot {
        database_identity: "database:review-test".to_string(),
        snapshot_id: "review:test".to_string(),
        review_revision: revision,
        thread_id: Some(
            ChatThreadId::new("thread:review-test").expect("thread ID should be valid"),
        ),
        working_folder_id: ProjectWorkingFolderId::new("workspace:review-test")
            .expect("working folder ID should be valid"),
        environment_id: "environment:review-test".to_string(),
        root: PathBuf::new(),
        source,
        source_label: "Provider turn".to_string(),
        before_oid: None,
        after_oid: None,
        context_lines: 3,
        ignore_whitespace: false,
        files: vec![file],
        provider_patches: HashMap::from([(file_id.clone(), patch.to_string())]),
        patch_cache: tokio::sync::Mutex::new(HashMap::new()),
        object_store: None,
        created_at: Instant::now(),
    });
    (snapshot, file_id)
}

async fn working_snapshot(
    repository: &TestRepository,
    mode: ReviewWorkingTreeMode,
) -> Arc<ReviewSnapshot> {
    let source = working_source(mode);
    let material = working_tree_material(repository.path(), mode)
        .await
        .expect("working snapshot should capture");
    let mut files = git_files(
        repository.path(),
        &material.before_oid,
        &material.after_oid,
        &source,
        false,
        material.object_store.as_deref(),
    )
    .await
    .expect("working snapshot files should read");
    enrich_working_tree_files(&mut files, &material.status, &source);
    let revision = review_revision(
        None,
        "environment:review-test",
        &source,
        &material.before_oid,
        &material.after_oid,
        false,
        3,
    )
    .expect("working snapshot revision should build");
    for file in &mut files {
        file.read.file_id = file_id(
            &revision,
            &file.read.relative_path,
            file.read.previous_relative_path.as_deref(),
        );
    }
    Arc::new(ReviewSnapshot {
        database_identity: "database:review-test".to_string(),
        snapshot_id: format!("review:{mode:?}"),
        review_revision: revision,
        thread_id: None,
        working_folder_id: ProjectWorkingFolderId::new("workspace:review-test")
            .expect("working folder ID should be valid"),
        environment_id: "environment:review-test".to_string(),
        root: repository.path().to_path_buf(),
        source,
        source_label: material.label,
        before_oid: Some(material.before_oid),
        after_oid: Some(material.after_oid),
        context_lines: 3,
        ignore_whitespace: false,
        files,
        provider_patches: HashMap::new(),
        patch_cache: tokio::sync::Mutex::new(HashMap::new()),
        object_store: material.object_store,
        created_at: Instant::now(),
    })
}

#[test]
fn review_revision_is_stable_across_rendering_options_and_scoped_to_source() {
    let thread = ChatThreadId::new("thread:revision-test").expect("thread ID should be valid");
    let source = working_source(ReviewWorkingTreeMode::All);
    let baseline = review_revision(
        Some(&thread),
        "environment:one",
        &source,
        "before",
        "after",
        false,
        3,
    )
    .expect("review revision should be created");
    let different_rendering = review_revision(
        Some(&thread),
        "environment:one",
        &source,
        "before",
        "after",
        true,
        MAX_CONTEXT_LINES,
    )
    .expect("review revision should be created");

    assert_eq!(baseline, different_rendering);
    assert_eq!(baseline.len(), 64);
    assert_ne!(
        baseline,
        review_revision(
            Some(&thread),
            "environment:one",
            &working_source(ReviewWorkingTreeMode::Staged),
            "before",
            "after",
            false,
            3,
        )
        .expect("source-scoped revision should be created")
    );
    assert_ne!(
        baseline,
        review_revision(
            Some(&thread),
            "environment:two",
            &source,
            "before",
            "after",
            false,
            3,
        )
        .expect("environment-scoped revision should be created")
    );
    assert_eq!(
        file_id(&baseline, "src/lib.rs", None),
        file_id(&different_rendering, "src/lib.rs", None)
    );
    assert_ne!(
        file_id(&baseline, "src/lib.rs", None),
        file_id(&baseline, "src/lib.rs", Some("src/old.rs"))
    );
    assert_ne!(
        snapshot_id("database:one", Some(&thread), "environment:one", &baseline),
        snapshot_id("database:two", Some(&thread), "environment:one", &baseline)
    );
    assert_ne!(
        completed_operation_key("database:one", "operation:shared"),
        completed_operation_key("database:two", "operation:shared")
    );
}

#[test]
fn patch_parser_preserves_complete_hunks_and_stable_change_ids() {
    let with_context = concat!(
        "diff --git a/src/lib.rs b/src/lib.rs\n",
        "--- a/src/lib.rs\n",
        "+++ b/src/lib.rs\n",
        "@@ -10,3 +10,3 @@ fn value()\n",
        " keep\n",
        "-old\n",
        "+new\n",
        " tail\n",
        "@@ -30 +30 @@\n",
        "-before\n",
        "+after\n"
    );
    let without_context = "@@ -11 +11 @@\n-old\n+new\n";
    let parsed = parse_patch(with_context, "file:stable").expect("complete Git patch should parse");
    let compact =
        parse_patch(without_context, "file:stable").expect("compact Git patch should parse");

    assert_eq!(parsed.hunks.len(), 2);
    assert_eq!(parsed.hunks[0].read.old_start, 10);
    assert_eq!(parsed.hunks[0].read.old_count, 3);
    assert_eq!(parsed.hunks[0].read.new_start, 10);
    assert_eq!(parsed.hunks[0].read.new_count, 3);
    assert_eq!(parsed.hunks[1].read.old_count, 1);
    assert_eq!(parsed.hunks[1].read.new_count, 1);
    assert_eq!(parsed.hunks[0].read.hunk_id, compact.hunks[0].read.hunk_id);
    assert_ne!(
        parsed.hunks[0].read.hunk_id,
        hunk_id("file:stable", "@@ -11 +11 @@\n-old\n+other\n", 11, 11)
    );
    assert!(parsed.preamble.starts_with("diff --git "));
    assert!(parsed.hunks.iter().all(|hunk| {
        hunk.text.starts_with("@@ ") && hunk.read.state == ReviewPatchState::Complete
    }));
}

#[tokio::test]
async fn provider_turn_prefers_a_unified_patch_over_later_raw_file_contents() {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("review fixture database should open");
    sqlx::query(
        "CREATE TABLE chat_events (
           thread_id TEXT NOT NULL,
           turn_id TEXT,
           sequence INTEGER NOT NULL,
           event_type TEXT NOT NULL,
           payload_data TEXT NOT NULL
         )",
    )
    .execute(&pool)
    .await
    .expect("review fixture table should be created");
    let thread_id = ChatThreadId::new("thread:provider-patch").expect("thread ID is valid");
    let turn_id = ChatTurnId::new("turn:provider-patch").expect("turn ID is valid");
    let summary = ChangedFileSummary {
        relative_path: "hello.py".to_string(),
        previous_relative_path: None,
        additions: Some(1),
        deletions: Some(0),
        binary: false,
        status: "added".to_string(),
    };
    let unified = CanonicalEvent::DiffUpdated(crate::events::DiffUpdatedEvent {
        source: "provider".to_string(),
        files: vec![summary],
        provider_diff: Some(
            "diff --git a/hello.py b/hello.py\nnew file mode 100644\n@@ -0,0 +1 @@\n+print('hello')\n"
                .to_string(),
        ),
    });
    let raw = CanonicalEvent::DiffUpdated(crate::events::DiffUpdatedEvent {
        source: "provider_file_change".to_string(),
        files: vec![ChangedFileSummary {
            relative_path: "/workspace/hello.py".to_string(),
            previous_relative_path: None,
            additions: Some(0),
            deletions: Some(0),
            binary: false,
            status: "modified".to_string(),
        }],
        provider_diff: Some("print('hello')\n".to_string()),
    });
    for (sequence, event) in [(1_i64, unified), (2_i64, raw)] {
        sqlx::query(
            "INSERT INTO chat_events
               (thread_id, turn_id, sequence, event_type, payload_data)
             VALUES (?, ?, ?, 'diff_updated', ?)",
        )
        .bind(thread_id.as_str())
        .bind(turn_id.as_str())
        .bind(sequence)
        .bind(serde_json::to_string(&event).expect("event should serialize"))
        .execute(&pool)
        .await
        .expect("review fixture event should insert");
    }

    let (files, patch) = provider_turn_patch(&pool, &thread_id, &turn_id)
        .await
        .expect("unified provider patch should load");

    assert_eq!(files.len(), 1);
    assert_eq!(files[0].relative_path, "hello.py");
    assert!(patch.starts_with("diff --git a/hello.py b/hello.py"));
}

#[tokio::test]
async fn patch_pages_stop_only_between_hunks_and_resume_with_the_cursor() {
    let patch = concat!(
        "diff --git a/src/lib.rs b/src/lib.rs\n",
        "--- a/src/lib.rs\n",
        "+++ b/src/lib.rs\n",
        "@@ -1 +1 @@\n",
        "-first old\n",
        "+first new\n",
        "@@ -10 +10 @@\n",
        "-second old\n",
        "+second new\n"
    );
    let (snapshot, file_id) = provider_snapshot(patch);
    let parsed = parse_patch(patch, &file_id).expect("fixture patch should parse");
    let first_page_limit = parsed.preamble.len()
        + parsed.hunks[0].text.len()
        + parsed.hunks[0].read.hunk_id.len()
        + 256;

    let first = read_patch_page(&snapshot, &file_id, 0, first_page_limit)
        .await
        .expect("first patch page should read");
    assert_eq!(first.state, ReviewPatchState::Partial);
    assert_eq!(first.hunks.len(), 1);
    assert_eq!(first.hunks[0].hunk_id, parsed.hunks[0].read.hunk_id);
    let expected_first_patch = format!("{}{}", parsed.preamble, parsed.hunks[0].text);
    assert_eq!(first.patch.as_deref(), Some(expected_first_patch.as_str()));
    let expected_cursor = format!("{file_id}/1");
    assert_eq!(
        first.continuation_cursor.as_deref(),
        Some(expected_cursor.as_str())
    );

    let second = read_patch_page(&snapshot, &file_id, 1, patch.len() + 4_096)
        .await
        .expect("second patch page should read");
    assert_eq!(second.state, ReviewPatchState::Partial);
    assert_eq!(second.hunks.len(), 1);
    assert_eq!(second.hunks[0].hunk_id, parsed.hunks[1].read.hunk_id);
    assert!(second.continuation_cursor.is_none());
    let expected_second_patch = format!("{}{}", parsed.preamble, parsed.hunks[1].text);
    assert_eq!(
        second.patch.as_deref(),
        Some(expected_second_patch.as_str())
    );
}

#[tokio::test]
async fn oversized_hunk_advances_without_returning_partial_patch_text() {
    let patch = concat!(
        "diff --git a/src/lib.rs b/src/lib.rs\n",
        "--- a/src/lib.rs\n",
        "+++ b/src/lib.rs\n",
        "@@ -1 +1 @@\n",
        "-a very long first line\n",
        "+another very long first line\n",
        "@@ -10 +10 @@\n",
        "-small\n",
        "+tiny\n"
    );
    let (snapshot, file_id) = provider_snapshot(patch);
    let parsed = parse_patch(patch, &file_id).expect("fixture patch should parse");
    let too_small = parsed.preamble.len() + parsed.hunks[0].text.len() - 1;

    let oversized = read_patch_page(&snapshot, &file_id, 0, too_small)
        .await
        .expect("oversized patch page should be described");
    assert_eq!(oversized.state, ReviewPatchState::OversizedHunk);
    assert_eq!(oversized.hunks.len(), 1);
    assert_eq!(oversized.hunks[0].state, ReviewPatchState::OversizedHunk);
    assert!(oversized.patch.is_none());
    let expected_cursor = format!("{file_id}/1");
    assert_eq!(
        oversized.continuation_cursor.as_deref(),
        Some(expected_cursor.as_str())
    );

    let resumed = read_patch_page(&snapshot, &file_id, 1, patch.len() + 4_096)
        .await
        .expect("page after oversized hunk should read");
    assert_eq!(resumed.hunks.len(), 1);
    assert_eq!(resumed.hunks[0].hunk_id, parsed.hunks[1].read.hunk_id);
    assert!(resumed.patch.as_deref().is_some_and(|value| {
        value.contains("-small\n+tiny\n") && !value.contains("very long first line")
    }));
}

#[test]
fn canonical_relative_paths_accept_unicode_and_reject_ambiguous_components() {
    for valid in [
        "src/lib.rs",
        "src/café file.rs",
        ".config/settings.json",
        "データ/結果.txt",
    ] {
        validate_path(valid).expect("canonical relative path should be accepted");
    }
    for invalid in [
        "",
        "/src/lib.rs",
        "src//lib.rs",
        "src/./lib.rs",
        "src/../lib.rs",
        "../src/lib.rs",
        "src\\lib.rs",
        "src/\0lib.rs",
        "src/\nlib.rs",
    ] {
        let error = validate_path(invalid).expect_err("unsafe path should be rejected");
        assert_eq!(error.code, ChatErrorCode::Validation);
        assert_eq!(error.field.as_deref(), Some("relativePath"));
    }
}

#[test]
fn unicode_review_selection_uses_character_columns_and_bounded_lines() {
    let contents = "αβγ\ncafé\n終";
    assert_eq!(
        select_text_range(contents, 1, 2, 2, 5).expect("multiline Unicode range should select"),
        "βγ\ncafé"
    );
    assert_eq!(
        select_text_range(contents, 2, 1, 3, 1).expect("whole-line Unicode range should select"),
        "café\n終"
    );
    let invalid = select_text_range(contents, 1, 5, 1, 6)
        .expect_err("column beyond Unicode content should be rejected");
    assert_eq!(invalid.code, ChatErrorCode::Validation);
    assert_eq!(invalid.field.as_deref(), Some("range"));
}

#[test]
fn provider_selection_treats_marker_shaped_hunk_lines_as_content() {
    let patch = concat!(
        "diff --git a/src/lib.rs b/src/lib.rs\n",
        "--- a/src/lib.rs\n",
        "+++ b/src/lib.rs\n",
        "@@ -4 +4 @@\n",
        "---old marker\n",
        "+++new marker\n"
    );
    assert_eq!(
        select_provider_patch_lines(patch, "old", 4, 1, 4, 1)
            .expect("old marker-shaped content should select"),
        "--old marker"
    );
    assert_eq!(
        select_provider_patch_lines(patch, "new", 4, 1, 4, 1)
            .expect("new marker-shaped content should select"),
        "++new marker"
    );
}

#[test]
fn patch_requests_require_an_explicit_bounded_file_selection() {
    let request = ReadChatReviewPatchesRequest {
        thread_id: None,
        working_folder_id: ProjectWorkingFolderId::new("workspace:review-test")
            .expect("working folder ID should be valid"),
        execution_environment_id: Some("environment:review-test".to_string()),
        snapshot_id: "review:test".to_string(),
        review_revision: "a".repeat(64),
        file_ids: Vec::new(),
        continuation_cursor: None,
        byte_limit: None,
    };
    let error = validate_patch_request(&request)
        .expect_err("implicit all-file patch reads should be rejected");
    assert_eq!(error.code, ChatErrorCode::Validation);
    assert_eq!(error.field.as_deref(), Some("fileIds"));
}

#[test]
fn name_status_and_status_metadata_cover_rename_binary_untracked_and_conflict() {
    let source = working_source(ReviewWorkingTreeMode::Unstaged);
    let stats = BTreeMap::from([
        ("src/new.rs".to_string(), (Some(0), Some(0))),
        ("assets/image.bin".to_string(), (None, None)),
        ("src/untracked.rs".to_string(), (Some(1), Some(0))),
        ("src/conflict.rs".to_string(), (Some(2), Some(2))),
    ]);
    let names = b"R100\0src/old.rs\0src/new.rs\0M\0assets/image.bin\0A\0src/untracked.rs\0U\0src/conflict.rs\0";
    let mut files = parse_name_status(names, &stats, &source).expect("name status should parse");
    let status = git::GitStatusRead {
        branch: Some("main".to_string()),
        detached: false,
        upstream: None,
        ahead: 0,
        behind: 0,
        files: vec![git::GitChangedPathRead {
            relative_path: "src/conflict.rs".to_string(),
            original_relative_path: None,
            index_status: "U".to_string(),
            worktree_status: "U".to_string(),
            untracked: false,
            ignored: false,
            conflicted: true,
        }],
    };
    enrich_working_tree_files(&mut files, &status, &source);

    let renamed = files
        .iter()
        .find(|file| file.read.relative_path == "src/new.rs")
        .expect("rename should exist");
    assert_eq!(
        renamed.read.previous_relative_path.as_deref(),
        Some("src/old.rs")
    );
    assert_eq!(renamed.read.status, "renamed");
    assert!(renamed.read.flags.pure_rename);
    let binary = files
        .iter()
        .find(|file| file.read.relative_path == "assets/image.bin")
        .expect("binary change should exist");
    assert!(binary.read.flags.binary);
    assert!(!binary.read.capabilities.comment);
    let untracked = files
        .iter()
        .find(|file| file.read.relative_path == "src/untracked.rs")
        .expect("untracked change should exist");
    assert!(untracked.read.flags.untracked);
    let conflict = files
        .iter()
        .find(|file| file.read.relative_path == "src/conflict.rs")
        .expect("conflict should exist");
    assert!(conflict.read.flags.conflict);
    assert!(!conflict.read.capabilities.stage);
    assert!(!conflict.read.capabilities.unstage);
    assert!(!conflict.read.capabilities.discard);
}

#[test]
fn conflict_status_is_added_even_when_head_and_worktree_contents_match() {
    let source = working_source(ReviewWorkingTreeMode::All);
    let mut files = Vec::new();
    let status = git::GitStatusRead {
        branch: Some("main".to_string()),
        detached: false,
        upstream: None,
        ahead: 0,
        behind: 0,
        files: vec![git::GitChangedPathRead {
            relative_path: "src/conflict.rs".to_string(),
            original_relative_path: None,
            index_status: "U".to_string(),
            worktree_status: "U".to_string(),
            untracked: false,
            ignored: false,
            conflicted: true,
        }],
    };

    enrich_working_tree_files(&mut files, &status, &source);

    assert_eq!(files.len(), 1);
    assert_eq!(files[0].read.relative_path, "src/conflict.rs");
    assert_eq!(files[0].read.status, "modified");
    assert!(files[0].read.flags.conflict);
    assert!(!files[0].read.capabilities.stage);
    assert!(!files[0].read.capabilities.unstage);
    assert!(!files[0].read.capabilities.discard);
}

#[tokio::test]
async fn immutable_git_snapshot_observes_rename_binary_and_untracked_files() {
    let repository = TestRepository::new();
    repository.write(
        "src/original.rs",
        "fn stable_name() {\n    println!(\"stable\");\n}\n",
    );
    repository.write("assets/data.bin", [0_u8, 1, 2, 3]);
    repository.commit_all();
    fs::rename(
        repository.path().join("src/original.rs"),
        repository.path().join("src/renamed.rs"),
    )
    .expect("fixture file should rename");
    repository.write("assets/data.bin", [0_u8, 1, 9, 3]);
    repository.write("src/untracked.rs", "pub fn new_file() {}\n");

    let source = working_source(ReviewWorkingTreeMode::All);
    let material = working_tree_material(repository.path(), ReviewWorkingTreeMode::All)
        .await
        .expect("immutable worktree tree should be captured");
    let mut files = git_files(
        repository.path(),
        &material.before_oid,
        &material.after_oid,
        &source,
        false,
        material.object_store.as_deref(),
    )
    .await
    .expect("immutable Git diff should read");
    let status = git::status(repository.path())
        .await
        .expect("Git status should read");
    enrich_working_tree_files(&mut files, &status, &source);

    let renamed = files
        .iter()
        .find(|file| file.read.relative_path == "src/renamed.rs")
        .expect("renamed file should be present");
    assert_eq!(
        renamed.read.previous_relative_path.as_deref(),
        Some("src/original.rs")
    );
    assert!(renamed.read.flags.pure_rename);
    assert!(
        files
            .iter()
            .any(|file| { file.read.relative_path == "assets/data.bin" && file.read.flags.binary })
    );
    assert!(files.iter().any(|file| {
        file.read.relative_path == "src/untracked.rs" && file.read.flags.untracked
    }));
}

#[tokio::test]
async fn whole_scope_actions_do_not_materialize_large_patch_text() {
    let repository = TestRepository::new();
    repository.write("tracked.txt", "before\n");
    repository.commit_all();
    repository.write("tracked.txt", "after\n");
    repository.write("new.txt", "new\n");

    let unstaged = working_snapshot(&repository, ReviewWorkingTreeMode::Unstaged).await;
    let mut request = ApplyChatReviewActionRequest {
        thread_id: None,
        snapshot_id: unstaged.snapshot_id.clone(),
        working_folder_id: unstaged.working_folder_id.clone(),
        execution_environment_id: Some(unstaged.environment_id.clone()),
        expected_review_revision: unstaged.review_revision.clone(),
        file_id: None,
        hunk_ids: Vec::new(),
        operation: ReviewAction::Stage,
        confirmed: false,
        client_operation_id: "operation:stage-all".to_string(),
    };
    apply_action(&unstaged, &request, ReviewWorkingTreeMode::Unstaged)
        .await
        .expect("whole scope should stage without creating a patch payload");
    let staged_status = git::status(repository.path())
        .await
        .expect("staged status should read");
    assert!(
        staged_status
            .files
            .iter()
            .all(|file| file.index_status != "." && !file.untracked)
    );

    let staged = working_snapshot(&repository, ReviewWorkingTreeMode::Staged).await;
    request.snapshot_id = staged.snapshot_id.clone();
    request.expected_review_revision = staged.review_revision.clone();
    request.operation = ReviewAction::Unstage;
    request.client_operation_id = "operation:unstage-all".to_string();
    apply_action(&staged, &request, ReviewWorkingTreeMode::Staged)
        .await
        .expect("whole scope should unstage without creating a patch payload");
    let unstaged_status = git::status(repository.path())
        .await
        .expect("unstaged status should read");
    assert!(
        unstaged_status
            .files
            .iter()
            .all(|file| file.index_status == "." || file.untracked)
    );

    let discard = working_snapshot(&repository, ReviewWorkingTreeMode::Unstaged).await;
    request.snapshot_id = discard.snapshot_id.clone();
    request.expected_review_revision = discard.review_revision.clone();
    request.operation = ReviewAction::Discard;
    request.confirmed = true;
    request.client_operation_id = "operation:discard-all".to_string();
    apply_action(&discard, &request, ReviewWorkingTreeMode::Unstaged)
        .await
        .expect("whole scope should discard without creating a patch payload");
    assert!(
        git::status(repository.path())
            .await
            .expect("clean status should read")
            .files
            .is_empty()
    );
}

#[tokio::test]
async fn exact_discard_restores_unstaged_rename_with_untracked_destination() {
    let repository = TestRepository::new();
    repository.write("src/original.rs", "pub fn original() {}\n");
    repository.commit_all();
    repository.run_git(&["config", "status.renames", "false"]);
    fs::rename(
        repository.path().join("src/original.rs"),
        repository.path().join("src/renamed.rs"),
    )
    .expect("fixture file should rename");

    let snapshot = working_snapshot(&repository, ReviewWorkingTreeMode::Unstaged).await;
    let renamed = snapshot
        .files
        .iter()
        .find(|file| file.read.relative_path == "src/renamed.rs")
        .expect("synthetic review tree should detect the rename");
    assert_eq!(
        renamed.read.previous_relative_path.as_deref(),
        Some("src/original.rs")
    );
    assert!(renamed.read.flags.untracked);
    let request = ApplyChatReviewActionRequest {
        thread_id: None,
        snapshot_id: snapshot.snapshot_id.clone(),
        working_folder_id: snapshot.working_folder_id.clone(),
        execution_environment_id: Some(snapshot.environment_id.clone()),
        expected_review_revision: snapshot.review_revision.clone(),
        file_id: Some(renamed.read.file_id.clone()),
        hunk_ids: Vec::new(),
        operation: ReviewAction::Discard,
        confirmed: true,
        client_operation_id: "operation:discard-rename".to_string(),
    };

    apply_action(&snapshot, &request, ReviewWorkingTreeMode::Unstaged)
        .await
        .expect("exact rename discard should restore the original path");

    assert_eq!(
        fs::read_to_string(repository.path().join("src/original.rs"))
            .expect("original file should be restored"),
        "pub fn original() {}\n"
    );
    assert!(!repository.path().join("src/renamed.rs").exists());
    assert!(
        git::status(repository.path())
            .await
            .expect("discarded rename status should read")
            .files
            .is_empty()
    );
}

#[tokio::test]
async fn whole_scope_discard_restores_unstaged_rename_with_untracked_destination() {
    let repository = TestRepository::new();
    repository.write("src/original.rs", "pub fn original() {}\n");
    repository.write("src/modified.rs", "pub fn value() -> u8 { 1 }\n");
    repository.commit_all();
    repository.run_git(&["config", "status.renames", "false"]);
    fs::rename(
        repository.path().join("src/original.rs"),
        repository.path().join("src/renamed.rs"),
    )
    .expect("fixture file should rename");
    repository.write("src/modified.rs", "pub fn value() -> u8 { 2 }\n");

    let snapshot = working_snapshot(&repository, ReviewWorkingTreeMode::Unstaged).await;
    let renamed = snapshot
        .files
        .iter()
        .find(|file| file.read.relative_path == "src/renamed.rs")
        .expect("synthetic review tree should detect the rename");
    assert_eq!(
        renamed.read.previous_relative_path.as_deref(),
        Some("src/original.rs")
    );
    assert!(renamed.read.flags.untracked);
    let request = ApplyChatReviewActionRequest {
        thread_id: None,
        snapshot_id: snapshot.snapshot_id.clone(),
        working_folder_id: snapshot.working_folder_id.clone(),
        execution_environment_id: Some(snapshot.environment_id.clone()),
        expected_review_revision: snapshot.review_revision.clone(),
        file_id: None,
        hunk_ids: Vec::new(),
        operation: ReviewAction::Discard,
        confirmed: true,
        client_operation_id: "operation:discard-scope-with-rename".to_string(),
    };

    apply_action(&snapshot, &request, ReviewWorkingTreeMode::Unstaged)
        .await
        .expect("whole-scope discard should restore the original path");

    assert_eq!(
        fs::read_to_string(repository.path().join("src/original.rs"))
            .expect("original file should be restored"),
        "pub fn original() {}\n"
    );
    assert!(!repository.path().join("src/renamed.rs").exists());
    assert_eq!(
        fs::read_to_string(repository.path().join("src/modified.rs"))
            .expect("modified file should be restored"),
        "pub fn value() -> u8 { 1 }\n"
    );
    assert!(
        git::status(repository.path())
            .await
            .expect("discarded scope status should read")
            .files
            .is_empty()
    );
}

#[test]
fn action_validation_rejects_unsafe_scope_and_hunk_combinations() {
    let unstaged_source = working_source(ReviewWorkingTreeMode::Unstaged);
    let normal = test_file(
        &unstaged_source,
        "file:normal",
        "src/lib.rs",
        "modified",
        Some(1),
        Some(1),
        false,
    );
    require_action_allowed(
        ReviewWorkingTreeMode::Unstaged,
        ReviewAction::Stage,
        Some(&normal),
        &["hunk:one".to_string()],
    )
    .expect("unstaged text hunk should be stageable");
    let wrong_scope = require_action_allowed(
        ReviewWorkingTreeMode::Staged,
        ReviewAction::Discard,
        Some(&normal),
        &[],
    )
    .expect_err("staged changes should not discard directly");
    assert_eq!(wrong_scope.code, ChatErrorCode::CapabilityUnsupported);

    let mut rename = test_file(
        &unstaged_source,
        "file:rename",
        "src/new.rs",
        "renamed",
        Some(0),
        Some(0),
        false,
    );
    rename.read.previous_relative_path = Some("src/old.rs".to_string());
    let partial_rename = require_action_allowed(
        ReviewWorkingTreeMode::Unstaged,
        ReviewAction::Stage,
        Some(&rename),
        &["hunk:rename".to_string()],
    )
    .expect_err("pure rename should require a whole-file action");
    assert_eq!(partial_rename.code, ChatErrorCode::CapabilityUnsupported);

    let request = ApplyChatReviewActionRequest {
        thread_id: None,
        snapshot_id: "review:test".to_string(),
        working_folder_id: ProjectWorkingFolderId::new("workspace:review-test")
            .expect("working folder ID should be valid"),
        execution_environment_id: Some("environment:review-test".to_string()),
        expected_review_revision: "a".repeat(64),
        file_id: None,
        hunk_ids: vec!["hunk:without-file".to_string()],
        operation: ReviewAction::Stage,
        confirmed: false,
        client_operation_id: "operation:test".to_string(),
    };
    let invalid =
        validate_action_request(&request).expect_err("hunk action without file should be rejected");
    assert_eq!(invalid.code, ChatErrorCode::Validation);
    assert_eq!(invalid.field.as_deref(), Some("hunkIds"));
}

#[tokio::test]
async fn discard_requires_confirmation_before_any_workspace_access() {
    let source = working_source(ReviewWorkingTreeMode::Unstaged);
    let file = test_file(
        &source,
        "file:discard",
        "src/lib.rs",
        "modified",
        Some(1),
        Some(1),
        false,
    );
    let snapshot = Arc::new(ReviewSnapshot {
        database_identity: "database:review-test".to_string(),
        snapshot_id: "review:discard".to_string(),
        review_revision: "b".repeat(64),
        thread_id: None,
        working_folder_id: ProjectWorkingFolderId::new("workspace:review-test")
            .expect("working folder ID should be valid"),
        environment_id: "environment:review-test".to_string(),
        root: PathBuf::from("path-that-must-not-be-read"),
        source,
        source_label: "Unstaged changes".to_string(),
        before_oid: Some("before".to_string()),
        after_oid: Some("after".to_string()),
        context_lines: 3,
        ignore_whitespace: false,
        files: vec![file],
        provider_patches: HashMap::new(),
        patch_cache: tokio::sync::Mutex::new(HashMap::new()),
        object_store: None,
        created_at: Instant::now(),
    });
    let request = ApplyChatReviewActionRequest {
        thread_id: None,
        snapshot_id: snapshot.snapshot_id.clone(),
        working_folder_id: snapshot.working_folder_id.clone(),
        execution_environment_id: Some(snapshot.environment_id.clone()),
        expected_review_revision: snapshot.review_revision.clone(),
        file_id: None,
        hunk_ids: Vec::new(),
        operation: ReviewAction::Discard,
        confirmed: false,
        client_operation_id: "operation:discard".to_string(),
    };

    let error = apply_action(&snapshot, &request, ReviewWorkingTreeMode::Unstaged)
        .await
        .expect_err("unconfirmed discard should stop before workspace access");
    assert_eq!(error.code, ChatErrorCode::Permission);
}
