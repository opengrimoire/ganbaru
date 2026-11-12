use super::super::{GitCall, GitFolder, commit, initialize, run, stage};
use super::{
    COMMIT_DRAFT_MAX_FILES, COMMIT_DRAFT_MAX_PATCH_BYTES, COMMIT_DRAFT_RECENT_SUBJECTS,
    change_request_draft_context, commit_draft_context, truncate_patch,
};
use crate::models::ChatErrorCode;
use std::fs;
use std::path::{Path, PathBuf};

struct TestRepository(PathBuf);

impl TestRepository {
    async fn new() -> Self {
        let unique = format!(
            "ganbaru-git-draft-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time should follow Unix epoch")
                .as_nanos()
        );
        let path = std::env::temp_dir().join(unique);
        fs::create_dir(&path).expect("temporary directory should be created");
        let repository = Self(path);
        let folder = repository.folder();
        initialize(folder, Some("main"))
            .await
            .expect("repository should initialize");
        for (key, value) in [
            ("user.name", "Ganbaru test"),
            ("user.email", "test@example.invalid"),
            ("commit.gpgsign", "false"),
        ] {
            run(folder, &["config", key, value], GitCall::User)
                .await
                .expect("configuration should apply");
        }
        repository
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn folder(&self) -> GitFolder<'_> {
        GitFolder::untrusted(&self.0)
    }

    async fn commit_file(&self, name: &str, text: &str, message: &str) {
        fs::write(self.path().join(name), text).expect("file should write");
        stage(self.folder(), &[name.to_string()])
            .await
            .expect("file should stage");
        commit(self.folder(), message)
            .await
            .expect("commit should succeed");
    }
}

impl Drop for TestRepository {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn draft_patch_truncation_cuts_at_a_character_boundary() {
    let short = truncate_patch("abc".to_string(), 3);
    assert_eq!((short.text.as_str(), short.truncated), ("abc", false));
    let cut = truncate_patch("aé".to_string(), 2);
    assert_eq!((cut.text.as_str(), cut.truncated), ("a", true));
}

#[tokio::test]
async fn commit_draft_context_refuses_an_empty_stage_and_reads_an_unborn_branch() {
    let repository = TestRepository::new().await;
    let empty = commit_draft_context(repository.folder())
        .await
        .expect_err("an empty stage should be refused");
    assert_eq!(empty.code, ChatErrorCode::Validation);
    assert_eq!(empty.field.as_deref(), Some("staged"));

    fs::write(repository.path().join("first.txt"), "hello\n").expect("file should write");
    stage(repository.folder(), &["first.txt".to_string()])
        .await
        .expect("file should stage");
    let context = commit_draft_context(repository.folder())
        .await
        .expect("an unborn branch should read");
    assert_eq!(context.staged_paths, ["first.txt"]);
    assert!(context.patch.contains("+hello"));
    assert!(!context.patch_truncated);
    assert_eq!(context.omitted_files, 0);
    assert!(context.recent_subjects.is_empty());
}

#[tokio::test]
async fn commit_draft_context_marks_a_cut_patch_and_omitted_files_and_keeps_recent_subjects() {
    let repository = TestRepository::new().await;
    for index in 0..=COMMIT_DRAFT_RECENT_SUBJECTS {
        repository
            .commit_file(
                "log.txt",
                &format!("{index}\n"),
                &format!("chore: step {index}"),
            )
            .await;
    }
    let mut names = Vec::new();
    for index in 0..COMMIT_DRAFT_MAX_FILES + 3 {
        let name = format!("file-{index:03}.txt");
        fs::write(repository.path().join(&name), format!("{index}\n")).expect("file should write");
        names.push(name);
    }
    fs::write(
        repository.path().join("large.txt"),
        "x".repeat(COMMIT_DRAFT_MAX_PATCH_BYTES),
    )
    .expect("large file should write");
    names.push("large.txt".to_string());
    stage(repository.folder(), &names)
        .await
        .expect("files should stage");

    let context = commit_draft_context(repository.folder())
        .await
        .expect("staged changes should read");
    assert_eq!(context.staged_paths.len(), COMMIT_DRAFT_MAX_FILES);
    assert_eq!(context.omitted_files, 4);
    assert!(context.patch_truncated);
    assert!(context.patch.len() <= COMMIT_DRAFT_MAX_PATCH_BYTES);
    assert_eq!(context.recent_subjects.len(), COMMIT_DRAFT_RECENT_SUBJECTS);
    assert_eq!(
        context.recent_subjects[0],
        format!("chore: step {COMMIT_DRAFT_RECENT_SUBJECTS}")
    );
}

#[tokio::test]
async fn change_request_draft_context_reads_the_branch_range() {
    let repository = TestRepository::new().await;
    repository
        .commit_file("base.txt", "base\n", "chore: base")
        .await;
    run(
        repository.folder(),
        &["switch", "-c", "feature"],
        GitCall::User,
    )
    .await
    .expect("feature branch should start");
    repository
        .commit_file("feature.txt", "one\n", "feat: add feature")
        .await;
    repository
        .commit_file("feature.txt", "two\n", "fix: adjust feature")
        .await;

    let context = change_request_draft_context(repository.folder(), "main", "feature")
        .await
        .expect("the range should read");
    assert_eq!(context.staged_paths, ["feature.txt"]);
    assert!(context.patch.contains("+two"));
    assert!(!context.patch.contains("base"));
    assert_eq!(
        context.recent_subjects,
        ["fix: adjust feature", "feat: add feature"]
    );
}

#[tokio::test]
async fn change_request_draft_context_refuses_option_names_and_unknown_branches() {
    let repository = TestRepository::new().await;
    repository
        .commit_file("base.txt", "base\n", "chore: base")
        .await;
    let option = change_request_draft_context(repository.folder(), "-x", "main")
        .await
        .expect_err("an option should be refused");
    assert_eq!(option.field.as_deref(), Some("base"));
    let range = change_request_draft_context(repository.folder(), "main", "main..main")
        .await
        .expect_err("a range should be refused");
    assert_eq!(range.field.as_deref(), Some("head"));
    let missing = change_request_draft_context(repository.folder(), "main", "missing")
        .await
        .expect_err("an unknown branch should be refused");
    assert_eq!(missing.code, ChatErrorCode::Validation);
    assert_eq!(missing.field.as_deref(), Some("head"));
}
